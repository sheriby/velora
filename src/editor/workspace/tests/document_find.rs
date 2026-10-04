use super::super::{
    Editor, SearchMatcher, SearchOptions, find_document_match_from, search_document_source,
};
use super::search::plain_matcher;
use crate::components::UndoCaptureKind;
use gpui::{
    App,
    TestAppContext,
};
use std::fs;
use std::path::Path;
use std::time::Duration;


#[test]
fn document_search_finds_ascii_and_chinese_with_source_ranges() {
    let source = "# Hello\n\n你好 Hello\n";
    let hits = search_document_source(source, &SearchMatcher::new("hello", SearchOptions::default()), Path::new("note.md"), "note.md", 200);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].line, Some(1));
    assert_eq!(hits[1].line, Some(3));
    for hit in &hits {
        let range = hit.source_range.clone().unwrap();
        assert_eq!(&source[range], "Hello");
    }
    let chinese = search_document_source(source, &SearchMatcher::new("你好", SearchOptions::default()), Path::new("note.md"), "note.md", 1);
    assert_eq!(chinese.len(), 1);
    assert_eq!(&source[chinese[0].source_range.clone().unwrap()], "你好");
}

#[test]
fn document_match_navigation_can_search_forward_and_backward() {
    let source = "Alpha 你好 alpha";
    assert_eq!(
        find_document_match_from(source, &plain_matcher("alpha"), 0, false),
        Some(0..5)
    );
    assert_eq!(
        find_document_match_from(source, &plain_matcher("alpha"), 5, false),
        Some(13..18)
    );
    assert_eq!(
        find_document_match_from(source, &plain_matcher("alpha"), source.len(), true),
        Some(13..18)
    );
    assert_eq!(
        find_document_match_from(source, &plain_matcher("你好"), 0, false),
        Some(6..12)
    );
}

#[gpui::test]
async fn document_find_navigates_beyond_sidebar_result_limit(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let source = "a ".repeat(250);
    let (editor, cx) = cx.add_window_view(move |_, cx| Editor::from_markdown(cx, source, None));
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "a".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        assert_eq!(editor.workspace.search_results.len(), 200);
        for _ in 0..201 {
            editor.find_next_document_match(false, cx);
        }
        assert_eq!(editor.workspace.document_active_range, Some(400..401));
        assert_eq!(editor.workspace.search_active_index, None);
        editor.find_next_document_match(true, cx);
        assert_eq!(editor.workspace.document_active_range, Some(398..399));
        assert_eq!(editor.workspace.search_active_index, Some(199));
    });
}

#[gpui::test]
async fn document_find_searches_unsaved_edits_and_selects_matches(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# Alpha\n\nBeta".into(), None));
    editor.update(cx, |editor, cx| {
        let paragraph = editor.document.root_blocks()[1].clone();
        paragraph.update(cx, |paragraph, cx| {
            paragraph.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            paragraph.replace_text_in_visible_range(4..4, " alpha", None, false, cx);
        });
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "alpha".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        assert_eq!(editor.workspace.search_results.len(), 2);
        editor.find_next_document_match(false, cx);
        assert_eq!(editor.workspace.search_active_index, Some(0));
        let heading = editor.document.root_blocks()[0].read(cx);
        assert_eq!(heading.selected_range, 0..5);
        editor.find_next_document_match(false, cx);
        assert_eq!(editor.workspace.search_active_index, Some(1));
        let paragraph = editor.document.root_blocks()[1].read(cx);
        assert_eq!(paragraph.selected_range, 5..10);
    });
    editor.update(cx, |editor, cx| {
        let paragraph = editor.document.root_blocks()[1].clone();
        paragraph.update(cx, |paragraph, cx| {
            paragraph.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            paragraph.replace_text_in_visible_range(5..10, "", None, false, cx);
        });
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_results.len(), 1);
    });
}

#[gpui::test]
async fn document_find_enter_right_after_typing_still_jumps(cx: &mut TestAppContext) {
    // 用户报修：打完查询立刻回车（120ms 去抖窗口内）会静默什么都不做。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "# Alpha\n\nBeta alpha\n".into(), None)
    });
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "alpha".into();
        editor.schedule_workspace_search(cx);
        // 用户打完字立刻回车：后台搜索还没落地。
        editor.find_next_document_match(false, cx);
    });
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.document_active_range,
            Some(2..7),
            "回车应立刻跳到第一条命中，而不是静默什么都不做"
        );
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.document_active_range,
            Some(2..7),
            "结果落地后不能把刚跳到的命中清掉"
        );
    });
}

#[gpui::test]
async fn document_find_jump_keeps_the_query_field_focused(cx: &mut TestAppContext) {
    // 用户报修：跳转后焦点被抢进正文，继续敲字直接改写文档（数据损坏）。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let original = "# Alpha\n\nBeta alpha\n";
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, original.into(), None)
    });
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "alpha".into();
        editor.schedule_workspace_search(cx);
    });
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| {
        editor.read_with(cx, |editor, _| {
            let focus = editor.workspace.search_focus.as_ref().expect("find focus");
            assert!(focus.is_focused(window), "打开查找面板后焦点应在查询框");
        });
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.find_next_document_match(false, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| {
        editor.read_with(cx, |editor, _| {
            let focus = editor.workspace.search_focus.as_ref().expect("find focus");
            assert!(focus.is_focused(window), "跳转后焦点应留在查询框");
        });
    });
    // 继续敲字：必须进查询，不是进正文。
    let before = editor.read_with(cx, |editor, cx| editor.current_document_source(cx));
    cx.simulate_input("X");
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.workspace.search_query.contains('X'),
            "键入应进查询框，实际查询 = {:?}",
            editor.workspace.search_query
        );
        assert_eq!(
            editor.current_document_source(cx),
            before,
            "跳转后键入不许改写正文"
        );
    });
}

#[gpui::test]
async fn document_find_highlights_survive_a_view_mode_switch(cx: &mut TestAppContext) {
    // 用户报修：切渲染/源码模式后文档内搜索高亮全丢，直到改查询才回来。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "# Alpha\n\nBeta alpha\n".into(), None)
    });
    let highlighted_roots = |editor: &Editor, cx: &App| -> usize {
        editor
            .document
            .root_blocks()
            .iter()
            .filter(|block| !block.read(cx).search_highlight_ranges.is_empty())
            .count()
    };
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "alpha".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(highlighted_roots(editor, cx), 2, "渲染模式两个块各有一条命中");
    });
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(highlighted_roots(editor, cx), 1, "源码模式是单块文档，命中仍要高亮");
    });
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(highlighted_roots(editor, cx), 2, "切回渲染模式命中仍要高亮");
    });
}

#[gpui::test]
async fn document_find_jump_unfolds_the_section_containing_the_match(
    cx: &mut TestAppContext,
) {
    // 用户报修：折叠标题里的命中搜不到也看不到（块被过滤，不挂载也不滚动）。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "# A\n\nhidden needle\n\n# B\n\nneedle again\n".into(), None)
    });
    editor.update(cx, |editor, cx| {
        let heading = editor.document.root_blocks()[0].clone();
        heading.update(cx, |block, _cx| block.folded = true);
        editor.fold_state_version = editor.fold_state_version.wrapping_add(1);
        editor.open_document_find(cx);
        editor.workspace.search_query = "needle".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.find_next_document_match(false, cx);
    });
    editor.read_with(cx, |editor, cx| {
        assert!(
            !editor.document.root_blocks()[0].read(cx).folded,
            "命中在折叠章节内时应先展开标题"
        );
        assert_eq!(
            editor.workspace.document_active_range,
            Some(12..18),
            "应跳到折叠章节内的第一条命中"
        );
    });
}

#[gpui::test]
async fn closing_the_sidebar_clears_document_find_highlights(cx: &mut TestAppContext) {
    // 用户报修：关侧栏后正文里的搜索高亮还留着，没有面板解释也清不掉。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "# Alpha\n\nBeta alpha\n".into(), None)
    });
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "alpha".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    let highlighted_roots = |editor: &Editor, cx: &App| -> usize {
        editor
            .document
            .root_blocks()
            .iter()
            .filter(|block| !block.read(cx).search_highlight_ranges.is_empty())
            .count()
    };
    editor.read_with(cx, |editor, cx| {
        assert_eq!(highlighted_roots(editor, cx), 2, "前置：两条命中都高亮");
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.toggle_workspace_drawer(window, cx));
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert!(!editor.workspace.is_open, "前置：侧栏已收起");
        assert_eq!(highlighted_roots(editor, cx), 0, "收起侧栏后不应残留搜索高亮");
    });
}

#[gpui::test]
async fn document_find_refreshes_after_switching_tabs(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-document-find-tabs-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let first = root.join("first.md");
    let second = root.join("second.md");
    fs::write(&first, "needle in first").unwrap();
    fs::write(&second, "needle in second").unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root, cx);
            editor.open_workspace_file(first.clone(), window, cx);
            editor.open_document_find(cx);
            editor.workspace.search_query = "needle".into();
            editor.schedule_workspace_search(cx);
        });
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_results.len(), 1);
        assert_eq!(editor.workspace.search_results[0].path, first);
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second.clone(), window, cx);
        });
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_results.len(), 1);
        assert_eq!(editor.workspace.search_results[0].path, second);
    });
}

#[gpui::test]
async fn cmd_f_opens_current_document_find_in_sidebar(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::app_menu::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "hello\n\nhello".into(), None)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.draw(cx).clear();
    });
    cx.simulate_keystrokes("cmd-f");
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.workspace.is_open);
        assert!(editor.workspace.active_tab == super::super::WorkspaceTab::Search);
        assert_eq!(
            editor.workspace.search_scope,
            super::super::WorkspaceSearchScope::Document
        );
    });
    cx.simulate_input("hello");
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_results.len(), 2);
    });
    cx.simulate_keystrokes("enter");
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_active_index, Some(0));
    });
    cx.simulate_keystrokes("cmd-g");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_active_index, Some(1));
    });
    cx.simulate_keystrokes("cmd-shift-g");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_active_index, Some(0));
    });
}


/// 读取侧的坐标必须是文件坐标：Search 面板显示的行号与跳转用的字节区间，
/// 说的都应该是磁盘上那份文本，而不是块树重新序列化出来的那份。
///
/// 夹具同时踩两种「序列化会改写形状」的写法：Setext 标题被压成 ATX（少一行），
/// 表格列宽被重新填充（字节数变了）。只要读的是重新序列化的结果，行号与
/// 字节区间就都会漂。
#[gpui::test]
async fn document_find_reports_the_file_position_of_a_lossy_shape_hit(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-find-file-position-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("note.md");
    let source = concat!(
        "标题\n",
        "=====\n",
        "\n",
        "| 名称 | 数量 |\n",
        "| ---- | ---- |\n",
        "| 甲   | 目标 |\n",
    );
    fs::write(&path, source).expect("write fixture");
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let document = crate::editor::encoding::load_document(&path).expect("read fixture");
    let open_path = path.clone();
    let (editor, cx) = cx.add_window_view(move |_, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    });
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "目标".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();

    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_results.len(), 1);
        let hit = &editor.workspace.search_results[0];
        assert_eq!(
            hit.line,
            Some(6),
            "命中在文件里的第 6 行；Setext 标题被重新序列化压成一行就会报成第 5 行"
        );
        let range = hit.source_range.clone().unwrap();
        assert_eq!(
            editor.buffer.slice(range),
            "目标",
            "字节区间是从重新序列化的文本里算出来的，落到缓冲区就错位"
        );
    });
}

/// 查找下一个必须按**当前**文本算，不是后台搜索那一刻的快照。
///
/// 命中落地之后用户还能继续打字：在命中之前插几个字节，整段就往后移。这时
/// 「查找下一个」若拿旧快照算区间，跳过去的「命中」是改动之前的位置——光标落在
/// 别的词或空白上，替换也会改错地方。
#[gpui::test]
async fn document_find_navigates_the_edited_text_not_the_search_snapshot(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "标题段落\n\n正文一段。\n".into(), None)
    });
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "正文".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    editor.update(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_results.len(), 1, "夹具应该命中一次");
    });

    // 命中之后在它前面插入字节：整段往后移。
    editor.update(cx, |editor, cx| {
        let heading = editor.document.root_blocks()[0].clone();
        heading.update(cx, |heading, cx| {
            heading.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            heading.replace_text_in_visible_range(0..0, "插入的字", None, false, cx);
        });
    });
    cx.run_until_parked();

    editor.update(cx, |editor, cx| {
        editor.find_next_document_match(false, cx);
        let range = editor
            .workspace
            .document_active_range
            .clone()
            .expect("查找下一个应该落在命中上");
        let source = editor.current_document_source(cx);
        assert!(range.end <= source.len(), "命中区间越界：{:?} / 文本 {} 字节", range, source.len());
        assert_eq!(
            &source[range.clone()],
            "正文",
            "跳到了改动之前的位置：缓冲区现在是 {:?}",
            source.chars().take(40).collect::<String>()
        );
    });
}

/// 文档内搜索的高亮换算不该把整篇重拼一遍。
///
/// 命中的字节偏移要换算进块，靠的是**这一块自己**的映射：没命中的块连文本都不必取。
/// 旧实现先整篇重拼 source mapping、又把整篇文本复制出来扫一遍，于是查询没改、只是
/// 重算高亮也要付全文的钱（10 MiB 文档 = 每趟一次大搬运）。
#[gpui::test]
async fn document_find_highlights_map_only_the_blocks_with_hits(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(
            cx,
            concat!(
                "# 章一\n",
                "\n",
                "正文里有 alpha\n",
                "\n",
                "章二\n",
                "=====\n",
                "\n",
                "| 名称 | alpha |\n",
                "| ---- | ---- |\n",
                "| 甲   | 乙   |\n",
            )
            .to_string(),
            None,
        )
    });
    cx.update(|window, cx| window.draw(cx).clear());
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "alpha".into();
    });

    let builds_before = editor.read_with(cx, |editor, _| editor.source_mapping_full_builds.get());
    editor.update(cx, |editor, cx| editor.sync_document_search_highlights(cx));
    let builds_after = editor.read_with(cx, |editor, _| editor.source_mapping_full_builds.get());
    assert_eq!(
        builds_after - builds_before,
        0,
        "刷一次文档内搜索高亮重拼了整篇 source mapping"
    );

    let texts = editor.read_with(cx, |editor, cx| {
        editor
            .search_highlighted_blocks
            .iter()
            .map(|block| block.read_with(cx, |block, _cx| block.display_text().to_string()))
            .collect::<Vec<_>>()
    });
    assert!(
        texts.iter().any(|text| text == "正文里有 alpha"),
        "正文里的命中该有高亮：{texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "alpha"),
        "表格那一格里的命中也该有高亮：{texts:?}"
    );
}

/// 全文替换的计数不许虚报：映射在非规范前缀的块上会漂，替换不了的命中要保守
/// 跳过（换算出的可见切片必须等于搜到的原文），能替换的逐块直改、只付一次
/// 映射构建。
#[gpui::test]
async fn replace_all_replaces_exactly_the_hits_it_reports(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "> 引用甲\n\n正文甲、又是甲。\n".into(), None)
    });
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.open_document_find(cx);
        editor.workspace.search_query = "甲".into();
        editor.workspace.replace_query = "乙".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();

    let replaced = cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.replace_all_document_matches(window, cx))
    });
    cx.run_until_parked();

    assert_eq!(replaced, 3, "替换计数与命中数不符");
    let buffer = editor.read_with(cx, |editor, _cx| editor.buffer.text());
    assert_eq!(
        buffer, "> 引用乙\n\n正文乙、又是乙。\n",
        "替换后的文本不对：{buffer:?}"
    );
}
