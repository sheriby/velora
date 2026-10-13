//! 大纲那趟扫围栏与块树同一判据：围栏行的缩进口径只有一处。

use super::tabs_sessions::init_review_workspace_app;
use gpui::TestAppContext;

/// 四空格缩进的 ``` 是代码块的**内容**，不是闭合栏。
///
/// 大纲那条扫法按 `trim_start` 之后的行判围栏：`    ``` ` 于是当场出栏，后面代码块里的
/// `#` 被拼成大纲条目，`[TOC]` 跟着生成错误的目录。块树那边（`document/parse.rs`）的
/// 判据是「行首最多三格缩进」，两边必须同一处。
#[gpui::test]
async fn review_outline_keeps_an_indented_fence_run_inside_the_code_block(
    cx: &mut TestAppContext,
) {
    init_review_workspace_app(cx);
    let source = "```text\n    ```\n# 标题甲\n```\n\ntail\n";
    let segment = crate::editor::workspace::outline_segment(0, source, None);
    assert!(
        segment.headings.is_empty(),
        "代码块里的 `#` 被当成了大纲条目：{:?}",
        segment.headings
    );
    assert!(segment.fence_out.is_none(), "整段读完该出栏：{:?}", segment.fence_out);
}

/// 反过来：真正的闭合栏（缩进不超过三格）照样收尾。
#[gpui::test]
async fn review_outline_accepts_a_closing_fence_within_three_spaces(cx: &mut TestAppContext) {
    init_review_workspace_app(cx);
    let source = "```text\n  ```\n# 标题甲\n";
    let segment = crate::editor::workspace::outline_segment(0, source, None);
    assert_eq!(
        segment.headings.len(),
        1,
        "围栏收尾之后那一行才是标题：{:?}",
        segment.headings
    );
    assert_eq!(segment.headings[0].label, "标题甲");
}
