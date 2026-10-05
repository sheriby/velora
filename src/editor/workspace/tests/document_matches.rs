//! 文档命中表的测试。
//!
//! 这张表是搜索链路收敛的中心：结果列表、高亮、循环跳转、全部替换四处都读它。
//! 于是有两类事情必须有人钉住——
//! 1. **失效**：改了内容、换了文档，绝不能继续用旧表；
//! 2. **同源**：四处给出的命中必须是同一份，不能各说各话。
//!
//! 其中 `switching_documents_does_not_reuse_the_previous_documents_hit_table`
//! 是一条真实回归的守卫：两张表的缓存键一开始只有 (缓冲区版本, 查询, 选项)，
//! 而**两份从没编辑过的文档版本号都是 0**，于是「A 搜完切到 B 搜同一个词」会把
//! A 的命中区间当成 B 的，实测表现是跳转后高亮整体消失。键里补了缓冲区身份号
//! 之后才修好，这条用例就是那段过程的记录。

use super::super::{Editor, WorkspaceSearchScope, WorkspaceTab};
use crate::components::UndoCaptureKind;
use gpui::TestAppContext;
use std::fs;
use std::time::Duration;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

/// 起一个文档范围的搜索：设置查询、调度、把去抖时钟推过去。
fn search_document(editor: &gpui::Entity<Editor>, query: &str, cx: &mut TestAppContext) {
    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = WorkspaceTab::Search;
        editor.workspace.search_scope = WorkspaceSearchScope::Document;
        editor.workspace.search_query = query.to_string();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
}

/// 表里的命中字节区间。
fn table_ranges(editor: &gpui::Entity<Editor>, cx: &mut TestAppContext) -> Vec<std::ops::Range<usize>> {
    editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .document_matches
            .as_ref()
            .map(|table| {
                table
                    .hits
                    .iter()
                    .map(|hit| hit.range.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
}

#[gpui::test]
async fn the_hit_table_drives_every_search_path(cx: &mut TestAppContext) {
    init(cx);
    // 命中散在三根块里，其中一根块里有两个命中——「四处同源」在两种形状下都要成。
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(
            cx,
            "# needle heading\n\nneedle here and needle again\n\nnothing to match\n\n最后一行 needle\n"
                .to_string(),
            None,
        )
    });
    cx.run_until_parked();
    search_document(&editor, "needle", cx);

    let ranges = table_ranges(&editor, cx);
    assert_eq!(ranges.len(), 4, "文档里一共四个命中：{ranges:?}");

    // ① 结果列表：区间必须与表逐项相等（顺序、个数、位置都一样）。
    let rows = editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .search_results
            .iter()
            .map(|hit| hit.source_range.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        rows.into_iter().flatten().collect::<Vec<_>>(),
        ranges,
        "侧栏结果列表与命中表不是同一份数据"
    );

    // ② 高亮：所有被标记块上的高亮区间个数之和 == 命中数。
    let highlighted = editor.read_with(cx, |editor, cx| {
        editor
            .search_highlighted_blocks
            .iter()
            .map(|entity| entity.read(cx).search_highlight_ranges.len())
            .sum::<usize>()
    });
    assert_eq!(
        highlighted,
        ranges.len(),
        "高亮画出来的命中数必须等于表里的命中数"
    );

    // ③ 循环跳转：按 F3 走一圈，落点序列必须正好是表的内容。
    let visited = editor.update(cx, |editor, cx| {
        let mut out = Vec::new();
        for _ in 0..ranges.len() {
            editor.find_next_document_match(false, cx);
            out.push(editor.workspace.document_active_range.clone());
        }
        out
    });
    assert_eq!(
        visited.into_iter().flatten().collect::<Vec<_>>(),
        ranges,
        "按「下一个」走过的落点必须与表的顺序逐项一致"
    );

    // ④ 全部替换：替换计数 == 命中数（旧实现里这条出过「虚报」的 bug）。
    let replaced = cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.workspace.replace_query = "x".into();
            editor.replace_all_document_matches(window, cx)
        })
    });
    assert_eq!(replaced, ranges.len(), "替换计数必须等于命中表的长度");
}

#[gpui::test]
async fn switching_documents_does_not_reuse_the_previous_documents_hit_table(
    cx: &mut TestAppContext,
) {
    init(cx);
    let root = std::env::temp_dir().join(format!("velora-hits-table-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("mkdir");
    // 两份都是「打开后一个字没改」的状态，所以缓冲区版本号都是 0。
    fs::write(root.join("with.md"), "# 标题\n\n这里有测试命中\n").expect("write");
    fs::write(root.join("without.md"), "# 标题\n\n这一个词没有\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(root.join("with.md"), window, cx);
        });
    });
    cx.run_until_parked();
    search_document(&editor, "测试", cx);
    assert_eq!(
        table_ranges(&editor, cx).len(),
        1,
        "第一份文档里有一个命中"
    );

    // 换文档：版本号没变（两份都是 0），只有缓冲区身份变了。缓存必须认出来。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(root.join("without.md"), window, cx);
        });
    });
    cx.run_until_parked();
    search_document(&editor, "测试", cx);
    assert!(
        table_ranges(&editor, cx).is_empty(),
        "换成没有这个词的文档后，绝不能继续用上一份文档的命中表"
    );
    let stale_rows = editor.read_with(cx, |editor, _cx| editor.workspace.search_results.len());
    assert_eq!(stale_rows, 0, "结果列表里也不能留着上一份文档的命中");

    // 再换回来还得在。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(root.join("with.md"), window, cx);
        });
    });
    cx.run_until_parked();
    search_document(&editor, "测试", cx);
    assert_eq!(table_ranges(&editor, cx).len(), 1, "换回来命中要重新算出来");
    let _ = fs::remove_dir_all(&root);
}

#[gpui::test]
async fn editing_the_document_invalidates_the_hit_table(cx: &mut TestAppContext) {
    init(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "needle one\n".to_string(), None)
    });
    cx.run_until_parked();
    search_document(&editor, "needle", cx);
    assert_eq!(table_ranges(&editor, cx).len(), 1);

    // 在段落里再打一个 needle：表必须跟着缓冲区版本重算。
    editor.update(cx, |editor, cx| {
        let paragraph = editor.document.root_blocks()[0].clone();
        paragraph.update(cx, |paragraph, cx| {
            paragraph.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            paragraph.replace_text_in_visible_range(10..10, " and needle two", None, false, cx);
        });
    });
    cx.run_until_parked();
    search_document(&editor, "needle", cx);
    let ranges = table_ranges(&editor, cx);
    assert_eq!(ranges.len(), 2, "改过内容之后命中表必须重算：{ranges:?}");
    let source = editor.read_with(cx, |editor, cx| editor.current_document_source(cx));
    for range in &ranges {
        assert_eq!(&source[range.clone()], "needle", "命中区间必须对着当前文本");
    }
}

#[gpui::test]
async fn the_hit_table_is_reused_while_nothing_changes(cx: &mut TestAppContext) {
    init(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "needle a\nneedle b\n".to_string(), None)
    });
    cx.run_until_parked();
    search_document(&editor, "needle", cx);
    let first = editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .document_matches
            .as_ref()
            .map(|table| std::sync::Arc::as_ptr(&table.hits))
    });
    // 同一个查询、同一版内容再要一次：必须拿到同一份表（指针相同），不重扫文档。
    editor.update(cx, |editor, cx| {
        assert!(editor.document_matches(cx).is_some());
    });
    let second = editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .document_matches
            .as_ref()
            .map(|table| std::sync::Arc::as_ptr(&table.hits))
    });
    assert_eq!(first, second, "没有任何东西变化时不该重算命中表");

    // 换个开关就必须重算。
    editor.update(cx, |editor, _cx| {
        editor.workspace.search_match_case = true;
    });
    search_document(&editor, "needle", cx);
    let third = editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .document_matches
            .as_ref()
            .map(|table| std::sync::Arc::as_ptr(&table.hits))
    });
    assert_ne!(first, third, "搜索开关变了必须重算命中表");
}

#[gpui::test]
async fn cycling_forward_and_backward_visits_every_hit_exactly_once(cx: &mut TestAppContext) {
    init(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "一 needle 二\nneedle 三\n四 needle 五\n".to_string(), None)
    });
    cx.run_until_parked();
    search_document(&editor, "needle", cx);
    let ranges = table_ranges(&editor, cx);
    assert_eq!(ranges.len(), 3);

    // 前进一整圈 + 额外一步（必须回到起点，不多不少）。
    let forward = editor.update(cx, |editor, cx| {
        let mut out = Vec::new();
        for _ in 0..ranges.len() + 1 {
            editor.find_next_document_match(false, cx);
            out.push(editor.workspace.document_active_range.clone().unwrap());
        }
        out
    });
    assert_eq!(
        forward,
        ranges
            .iter()
            .cloned()
            .chain(std::iter::once(ranges[0].clone()))
            .collect::<Vec<_>>(),
        "前进必须逐个走完再环绕回第一个"
    );

    // 后退一整圈。起点是前进循环收尾时停下的 ranges[0]，所以后退依次是
    // ranges[2] → ranges[1] → ranges[0]（环绕一次正好回到起点）。
    let backward = editor.update(cx, |editor, cx| {
        let mut out = Vec::new();
        for _ in 0..ranges.len() {
            editor.find_next_document_match(true, cx);
            out.push(editor.workspace.document_active_range.clone().unwrap());
        }
        out
    });
    assert_eq!(
        backward,
        vec![ranges[2].clone(), ranges[1].clone(), ranges[0].clone()],
        "后退必须逐个走完并环绕"
    );
}

#[gpui::test]
async fn a_zero_width_match_does_not_stall_navigation(cx: &mut TestAppContext) {
    // 零宽命中（`start == end`）在旧那套「按字节位置找下一个」的导航里会把自己
    // 卡住：from == range.start 时同一个命中被反复选中。改成表上的索引导航之后
    // 不存在这个问题，这条把这个性质钉住。
    init(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "ab\nab\n".to_string(), None)
    });
    cx.run_until_parked();
    editor.update(cx, |editor, _cx| {
        editor.workspace.search_use_regex = true;
    });
    search_document(&editor, "x*", cx);
    let ranges = table_ranges(&editor, cx);
    assert!(
        ranges.iter().all(|range| range.is_empty()),
        "这个查询应当全是零宽命中：{ranges:?}"
    );
    let mut visited = editor.update(cx, |editor, cx| {
        let mut out = Vec::new();
        for _ in 0..ranges.len() {
            editor.find_next_document_match(false, cx);
            out.push(editor.workspace.document_active_range.clone().unwrap());
        }
        out
    });
    visited.sort_by_key(|range| range.start);
    visited.dedup();
    assert_eq!(
        visited.len(),
        ranges.len(),
        "每个零宽命中都必须被走到且只走一次"
    );
}

#[gpui::test]
async fn a_query_that_fails_to_compile_clears_the_table(cx: &mut TestAppContext) {
    init(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "a{2, needle".to_string(), None)
    });
    cx.run_until_parked();
    editor.update(cx, |editor, _cx| {
        editor.workspace.search_use_regex = true;
    });
    search_document(&editor, "a{2,", cx);
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.workspace.search_error.is_some(),
            "非法正则必须把诊断留在状态里"
        );
        assert!(
            editor.workspace.document_matches.is_none(),
            "非法正则不得留下命中表"
        );
        assert!(editor.workspace.search_results.is_empty());
    });
    // 改成合法的模式之后诊断消失、命中重新算出来。
    search_document(&editor, "needle", cx);
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_error, None);
        assert!(editor.workspace.document_matches.is_some());
    });
    assert_eq!(table_ranges(&editor, cx).len(), 1);
}
