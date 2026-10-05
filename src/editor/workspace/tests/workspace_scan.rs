//! 工作区范围内容扫描的测试：磁盘文件的编码形状与行形状。
//!
//! 工作区搜索读的是磁盘字节，而磁盘上的文本文件有三种要命的形状：UTF-8、
//! CRLF、以及中文 Windows 常见的 GBK/GB18030。这三种都必须搜得到内容，
//! 且**同一份文本用哪种编码写出来，给出的命中行号、行内区间、预览要一样**——
//! 否则「搜到了但跳过去位置不对」就回来了。

use super::super::{
    Editor, SearchMatcher, SearchOptions, TreeSortPreference, WorkspaceSearchScope, WorkspaceTab,
    is_likely_text_file, scan_workspace_dir, search_workspace_files,
};
use crate::editor::encoding::decode_document_bytes;
use gpui::TestAppContext;
use std::fs;
use std::ops::Range;
use std::time::Duration;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

fn gb18030(text: &str) -> Vec<u8> {
    encoding_rs::GB18030.encode(text).0.into_owned()
}

/// 工作区命中里能跨编码比较的那部分：文件标签、行号、行内区间、预览。
fn comparable_rows(
    hits: &[super::super::WorkspaceSearchHit],
) -> Vec<(String, Option<usize>, Option<Range<usize>>, String)> {
    hits.iter()
        .map(|hit| {
            (
                hit.label.clone(),
                hit.line,
                hit.match_range.clone(),
                hit.preview.clone(),
            )
        })
        .collect()
}

#[gpui::test]
async fn workspace_search_finds_content_in_a_gb18030_file(cx: &mut TestAppContext) {
    // 缺陷 #5：内容搜索的解码口径比编辑器窄——`cached_file_source` 用
    // `String::from_utf8` 一把过，失败就整个文件跳过。中文 Windows 上的
    // GBK/GB18030 笔记因此**从来搜不到正文**。
    let background = cx.executor();
    let root = std::env::temp_dir().join(format!("velora-gb-search-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    fs::write(root.join("会议.md"), gb18030("# 会议记录\n\n中文正文与 English\n")).expect("write");

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
    let matches = search_workspace_files(
        &tree,
        &SearchMatcher::new("中文", SearchOptions::default()),
        200,
        &background,
    )
    .await;

    assert_eq!(
        matches.len(),
        1,
        "GB18030 文件的正文必须能搜到（当前是 0 条）"
    );
    assert_eq!(matches[0].label, "会议.md");
    assert_eq!(matches[0].line, Some(3), "命中的是第三行「中文正文与 English」");
    assert!(matches[0].preview.contains("中文正文"));

    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn the_same_text_searches_identically_whether_it_is_utf8_or_gb18030(
    cx: &mut TestAppContext,
) {
    // 这条是上面那条的真正闸门：不是「能搜到」就行，而是**同一份文本换一种编码
    // 写盘，行号、行内区间、预览要逐位一样**。区间按解码后的文本算，所以两种
    // 编码下的字节偏移本来就不同——对齐的依据是解码文本，不是磁盘字节。
    let background = cx.executor();
    let root = std::env::temp_dir().join(format!("velora-gb-twin-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    let text = "# 标题\n\n第一行 needle 正文\n第三行\nneedle 在行首\n";
    fs::write(root.join("utf8.md"), text).expect("write utf8");
    fs::write(root.join("gb.md"), gb18030(text)).expect("write gb");

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
    let hits = search_workspace_files(
        &tree,
        &SearchMatcher::new("needle", SearchOptions::default()),
        200,
        &background,
    )
    .await;

    let mut utf8_rows = Vec::new();
    let mut gb_rows = Vec::new();
    for row in comparable_rows(&hits) {
        if row.0 == "utf8.md" {
            utf8_rows.push((row.1, row.2, row.3));
        } else {
            gb_rows.push((row.1, row.2, row.3));
        }
    }
    assert_eq!(utf8_rows.len(), 2, "每个文件都该有两条命中：{hits:?}");
    assert_eq!(
        gb_rows, utf8_rows,
        "GB18030 的命中必须与同一份文本的 UTF-8 版本逐位一致"
    );
    // GB18030 解码回来就是同一份文本——这条断言的是「口径统一」的依据本身。
    assert_eq!(
        decode_document_bytes(gb18030(text)),
        text,
        "用例前提：两种编码解码出同一份文本"
    );

    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn a_crlf_file_keeps_the_same_lines_and_ranges_as_its_lf_twin(cx: &mut TestAppContext) {
    // 磁盘上的老 Windows 文件是 CRLF。行按 LF 数，行内区间不计入结尾的 LF——
    // 但 CR 留在行文本里（手写层是这个口径，引擎也是），所以换实现不能把行号或
    // 区间挪位。断言用的是「同一份内容两种行尾，结果逐位相同」。
    let background = cx.executor();
    let root = std::env::temp_dir().join(format!("velora-crlf-scan-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    fs::write(
        root.join("crlf.md"),
        "first needle line\r\nsecond\r\nneedle again\r\n",
    )
    .expect("write crlf");
    fs::write(root.join("lf.md"), "first needle line\nsecond\nneedle again\n").expect("write lf");

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
    let hits = search_workspace_files(
        &tree,
        &SearchMatcher::new("needle", SearchOptions::default()),
        200,
        &background,
    )
    .await;

    let rows = |label: &str| -> Vec<(Option<usize>, Option<Range<usize>>)> {
        hits.iter()
            .filter(|hit| hit.label == label)
            .map(|hit| (hit.line, hit.match_range.clone()))
            .collect()
    };
    let expected = vec![(Some(1), Some(6..12)), (Some(3), Some(0..6))];
    assert_eq!(rows("crlf.md"), expected, "CRLF 文件的行号与行内区间");
    assert_eq!(rows("lf.md"), expected, "两种行尾必须逐位一致");

    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn a_file_without_a_trailing_newline_still_reports_its_last_line(cx: &mut TestAppContext) {
    // 没写完的文件常常没有末行换行。手写层用 `split_inclusive('\n')` 会把这段
    // 当成一行，引擎的行策略也必须算它一行、且序号连续。
    let background = cx.executor();
    let root = std::env::temp_dir().join(format!("velora-no-eol-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    fs::write(root.join("note.md"), "needle 在第一行\n中间\nneedle 在末行").expect("write");

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
    let hits = search_workspace_files(
        &tree,
        &SearchMatcher::new("needle", SearchOptions::default()),
        200,
        &background,
    )
    .await;
    assert_eq!(
        hits.iter()
            .map(|hit| (hit.line, hit.match_ordinal))
            .collect::<Vec<_>>(),
        vec![(Some(1), Some(0)), (Some(3), Some(1))],
        "末行没有换行符也要算第三行，序号跟着含命中的行走"
    );

    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn one_file_contributes_every_matching_line_up_to_the_global_limit(
    cx: &mut TestAppContext,
) {
    // 单文件全量收集：曾经硬编码「每文件只报 3 条」被用户报修过「结果不全」。
    // 顺带钉住引擎侧的早停只在**收满全局 limit** 时发生，而不是每文件一个小组。
    let background = cx.executor();
    let root = std::env::temp_dir().join(format!("velora-per-file-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    let mut text = String::new();
    for index in 1..=30 {
        text.push_str(&format!("第 {index} 行 needle\n"));
    }
    fs::write(root.join("many.md"), &text).expect("write");

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
    let all = search_workspace_files(
        &tree,
        &SearchMatcher::new("needle", SearchOptions::default()),
        200,
        &background,
    )
    .await;
    assert_eq!(all.len(), 30, "30 行命中必须全报出来");
    assert_eq!(
        all.iter().map(|hit| hit.line).collect::<Vec<_>>(),
        (1..=30).map(Some).collect::<Vec<_>>()
    );

    let capped = search_workspace_files(
        &tree,
        &SearchMatcher::new("needle", SearchOptions::default()),
        5,
        &background,
    )
    .await;
    assert_eq!(capped.len(), 5, "全局上限 5 就只报 5 条");
    assert_eq!(
        capped
            .iter()
            .map(|hit| hit.match_ordinal)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1), Some(2), Some(3), Some(4)],
        "序号从 0 连续递增，跳转按它对位"
    );

    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn the_two_scopes_report_the_same_line_and_range_for_the_same_file(
    cx: &mut TestAppContext,
) {
    // 本方案的独有价值：工作区（读磁盘）与文档范围（读缓冲区）两条路必须给出
    // 同一个答案。同一个文件、同一个查询，逐位比对行号与行内区间。
    init(cx);
    let background = cx.executor();
    let root = std::env::temp_dir().join(format!("velora-scope-agree-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    let path = root.join("note.md");
    let text = "# 标题\n\nneedle 在这一行\n中间一行\n行首 needle 又一次\n";
    fs::write(&path, text).expect("write");

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
    let workspace_hits = search_workspace_files(
        &tree,
        &SearchMatcher::new("needle", SearchOptions::default()),
        200,
        &background,
    )
    .await;

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_file_source(cx, text.to_string(), Some(path.clone()))
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = WorkspaceTab::Search;
        editor.workspace.search_scope = WorkspaceSearchScope::Document;
        editor.workspace.search_query = "needle".to_string();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    let document_hits = editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .search_results
            .iter()
            .map(|hit| (hit.line, hit.match_range.clone()))
            .collect::<Vec<_>>()
    });

    let workspace_rows = workspace_hits
        .iter()
        .map(|hit| (hit.line, hit.match_range.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        workspace_rows,
        vec![(Some(3), Some(0..6)), (Some(5), Some(7..13))],
        "第五行「行首 needle 又一次」：行首两字 6 字节 + 空格 1 字节，needle 落在 7..13"
    );
    assert_eq!(
        workspace_rows, document_hits,
        "同一个文件在两个范围里必须给出同样的行号与行内区间"
    );

    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn workspace_scope_lists_one_row_per_line_and_document_scope_lists_every_hit(
    cx: &mut TestAppContext,
) {
    // 两套口径的差是**有意保留**的（方案文档 §1.2 #6）：工作区一行一条，文档范围
    // 一个命中一条；ordinal 的定义、跳转的对位都建在它上面。这里把它钉成断言，
    // 谁哪天「顺手统一一下」就会撞到这条。
    init(cx);
    let background = cx.executor();
    let root = std::env::temp_dir().join(format!("velora-row-shape-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    let path = root.join("note.md");
    let text = "needle 和 needle 在同一行\n另一行 needle\n";
    fs::write(&path, text).expect("write");

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
    let workspace_hits = search_workspace_files(
        &tree,
        &SearchMatcher::new("needle", SearchOptions::default()),
        200,
        &background,
    )
    .await;
    assert_eq!(
        workspace_hits.len(),
        2,
        "工作区：两根含命中的行 = 两条结果，同一行的第二个命中不单列"
    );
    assert_eq!(
        workspace_hits
            .iter()
            .map(|hit| (hit.line, hit.match_range.clone()))
            .collect::<Vec<_>>(),
        vec![(Some(1), Some(0..6)), (Some(2), Some(10..16))],
        "第二行「另一行 needle」：另一行 9 字节 + 空格 1 字节，needle 落在 10..16"
    );

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_file_source(cx, text.to_string(), Some(path.clone()))
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = WorkspaceTab::Search;
        editor.workspace.search_scope = WorkspaceSearchScope::Document;
        editor.workspace.search_query = "needle".to_string();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    let count = editor.read_with(cx, |editor, _cx| editor.workspace.search_results.len());
    assert_eq!(count, 3, "文档范围：三个命中就列三条");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn defect_the_text_sniff_still_refuses_a_gb18030_file() {
    // 本笔只统一了**搜索侧**的解码口径。打开侧还有一道闸：`is_likely_text_file`
    // 只看头 8 KiB 能不能按 UTF-8 解释，所以 GB18030 文件在工作区标签里仍然显示
    // 「无法使用文本编辑器预览该文件」——搜得到正文，却打不开来看。
    // `encoding.rs` 的模块说明写着 GB18030 笔记「可以正常打开编辑」，这道闸与它
    // 矛盾，是登记在册的缺陷（方案文档 §1.2 #5 的补充）。这条用例钉的是**现状**，
    // 谁修好了打开侧，它会红，届时把它改写成「修复后的期望」。
    let root = std::env::temp_dir().join(format!("velora-sniff-gb-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    let path = root.join("gb.md");
    fs::write(&path, gb18030("# 会议记录\n\n中文正文与 English\n")).expect("write");

    assert!(
        !is_likely_text_file(&path),
        "现状：8 KiB 窗口内有非法 UTF-8 就判成二进制，GB18030 文件打不开"
    );
    // 但同一份字节按文档解码路径是干净的文本——两道闸的口径差就在这里。
    assert_eq!(
        decode_document_bytes(fs::read(&path).expect("read")),
        "# 会议记录\n\n中文正文与 English\n"
    );

    let _ = fs::remove_dir_all(root);
}
