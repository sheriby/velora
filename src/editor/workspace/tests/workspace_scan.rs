//! 工作区范围内容扫描的测试：磁盘文件的编码形状与行形状。
//!
//! 工作区搜索读的是磁盘字节，而磁盘上的文本文件有三种要命的形状：UTF-8、
//! CRLF、以及中文 Windows 常见的 GBK/GB18030。这三种都必须搜得到内容，
//! 且**同一份文本用哪种编码写出来，给出的命中行号、行内区间、预览要一样**——
//! 否则「搜到了但跳过去位置不对」就回来了。

use super::super::{
    SearchMatcher, SearchOptions, TreeSortPreference, is_likely_text_file, scan_workspace_dir,
    search_workspace_files,
};
use crate::editor::encoding::decode_document_bytes;
use gpui::TestAppContext;
use std::fs;
use std::ops::Range;

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
