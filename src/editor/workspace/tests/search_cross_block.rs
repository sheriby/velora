//! 跨块命中的高亮分段（阶段 3b）。
//!
//! 高亮原来有一道墙：命中必须**整条被一根块包住**才画得出来
//! （`tree_sync.rs` 里那句 `if hit.start < block_start || hit.end > block_end { continue }`），
//! 于是跨块的命中一根块都不沾——结果列表里有它、跳转能落到它，正文里却什么都看不见。
//! 现在按块裁段：一根块拿到属于它那截，盖到几块就画几段。

use super::super::{Editor, WorkspaceSearchScope, WorkspaceTab};
use crate::editor::source_mapping::{clip_hit_to_span, hit_overlaps};
use crate::editor::ViewMode;
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

/// 起一个文档范围的正则搜索。
fn search_document_regex(editor: &gpui::Entity<Editor>, query: &str, cx: &mut TestAppContext) {
    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = WorkspaceTab::Search;
        editor.workspace.search_scope = WorkspaceSearchScope::Document;
        editor.workspace.search_use_regex = true;
        editor.workspace.search_query = query.to_string();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
}

/// 每根顶层块的显示高亮区间数，按树的顺序返回。
fn highlight_counts(editor: &Editor, cx: &gpui::App) -> Vec<usize> {
    editor
        .document
        .root_blocks()
        .iter()
        .map(|block| block.read(cx).search_highlight_ranges.len())
        .collect()
}

/// 每根顶层块有没有活动命中标记。
fn active_marks(editor: &Editor, cx: &gpui::App) -> Vec<bool> {
    editor
        .document
        .root_blocks()
        .iter()
        .map(|block| block.read(cx).search_active_range.is_some())
        .collect()
}

#[test]
fn hit_segmentation_clips_at_the_block_boundary() {
    // 裁段规则本身：跨块命中在左右两块上各拿到自己那截，不沾的块拿不到。
    let hit = 2..14;
    let left = 0..8;
    let right = 8..15;
    assert!(hit_overlaps(&hit, &left));
    assert!(hit_overlaps(&hit, &right));
    assert_eq!(clip_hit_to_span(&hit, left.start, left.end), Some(2..8));
    assert_eq!(clip_hit_to_span(&hit, right.start, right.end), Some(8..14));

    // 整条在一块里时不能有任何变化（阶段 1 的行为保持）。
    let inside = 9..12;
    assert_eq!(clip_hit_to_span(&inside, right.start, right.end), Some(9..12));
    assert!(!hit_overlaps(&inside, &left));

    // 只碰到块的右边界、一个字节都不在块内的，算不沾（块的区间是左闭右开）。
    let touches = 15..20;
    assert!(!hit_overlaps(&touches, &right));
    assert_eq!(clip_hit_to_span(&touches, right.start, right.end), None);
    // 同一条命中相对下一块就是正常 overlap——交界处的字节归右边那块。
    let next = 15..24;
    assert!(hit_overlaps(&touches, &next));
    assert_eq!(clip_hit_to_span(&touches, next.start, next.end), Some(15..20));

    // 零宽命中按起点归块，两块都可能有份（起点正好在块交界时归右块也算合法）。
    let zero = Range { start: 8, end: 8 };
    assert!(hit_overlaps(&zero, &left));
    assert!(hit_overlaps(&zero, &right));
    assert_eq!(clip_hit_to_span(&zero, left.start, left.end), Some(8..8));
    assert_eq!(clip_hit_to_span(&zero, right.start, right.end), Some(8..8));

    // 零宽区间整个落在块外面时不沾。取 max/min 会算出 20..20 落在 8..15 之外的
    // 反向区间（`15..20` 那样的端点相减就下溢），所以这一支必须退回 None。
    let outside = Range { start: 20, end: 20 };
    assert!(!hit_overlaps(&outside, &right));
    assert_eq!(clip_hit_to_span(&outside, right.start, right.end), None);
}

#[gpui::test]
async fn a_cross_block_match_highlights_every_block_it_covers(cx: &mut TestAppContext) {
    init(cx);
    // `# alpha` 与后面的 `alpha` 是两根顶层块，模式 `alpha\n\nalpha` 一条命中盖住两块。
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# alpha\n\nalpha\n".to_string(), None));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.document.root_count(), 2, "用例前提：两根顶层块");
    });

    search_document_regex(&editor, r"alpha\n\nalpha", cx);
    editor.read_with(cx, |editor, cx| {
        let table = editor
            .workspace
            .document_matches
            .as_ref()
            .expect("命中表要有");
        assert_eq!(
            table.hits.iter().map(|hit| hit.range.clone()).collect::<Vec<_>>(),
            vec![2..14],
            "这条命中本身要跨块"
        );
        assert_eq!(
            highlight_counts(editor, cx),
            vec![1, 1],
            "跨块命中要在两根块上各画一段"
        );

        // 把旧的判据套在这两根**真块**的实际源码区间上演示一遍：同一条命中，
        // 「整条被包住」两块都落空，「有交集」两块都沾到——这道墙就是跨块命中
        // 在正文里看不见的全部原因。
        let spans: Vec<Range<usize>> = editor
            .document
            .root_blocks()
            .iter()
            .filter_map(|block| editor.document.source_span_of(block.entity_id()))
            .collect();
        assert_eq!(spans.len(), 2);
        let covered_by_old_rule = spans
            .iter()
            .filter(|span| span.start <= 2 && 14 <= span.end)
            .count();
        let covered_by_new_rule = spans
            .iter()
            .filter(|span| hit_overlaps(&(2..14), span))
            .count();
        assert_eq!(covered_by_old_rule, 0, "旧的包含判据一块都沾不到");
        assert_eq!(covered_by_new_rule, 2, "新的交集判据两块都沾到");
    });
}

#[gpui::test]
async fn a_within_block_match_still_highlights_exactly_once(cx: &mut TestAppContext) {
    // 反向保证：裁段不能让行内命中的高亮翻倍或挪位。
    init(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# alpha\n\nalpha\n".to_string(), None));
    cx.run_until_parked();
    search_document_regex(&editor, "alpha", cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(highlight_counts(editor, cx), vec![1, 1]);
        for block in editor.document.root_blocks() {
            let ranges = &block.read(cx).search_highlight_ranges;
            assert_eq!(ranges.len(), 1, "每根块一条命中一条高亮：{ranges:?}");
            assert!(!ranges[0].is_empty(), "高亮区间不能是空段");
        }
    });
}

#[gpui::test]
async fn the_active_cross_block_hit_is_marked_on_every_block_it_covers(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# alpha\n\nalpha\n".to_string(), None));
    cx.run_until_parked();
    search_document_regex(&editor, r"alpha\n\nalpha", cx);
    // 跳到这条命中（它就是当前活动命中）。
    editor.update(cx, |editor, cx| {
        editor.find_next_document_match(false, cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.workspace.document_active_range,
            Some(2..14),
            "活动命中要落在跨块那条上"
        );
        assert_eq!(
            active_marks(editor, cx),
            vec![true, true],
            "跨块的活动命中在两根块上都要有活动标记"
        );
    });
}

#[gpui::test]
async fn a_cross_block_hit_still_highlights_after_switching_to_source_mode(
    cx: &mut TestAppContext,
) {
    init(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# alpha\n\nalpha\n".to_string(), None));
    cx.run_until_parked();
    search_document_regex(&editor, r"alpha\n\nalpha", cx);
    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert!(matches!(editor.view_mode, ViewMode::Source));
        let counts = highlight_counts(editor, cx);
        assert!(
            counts.iter().sum::<usize>() >= 1,
            "源码模式下这条跨块命中至少要有高亮，不能因为换视图就整条消失：{counts:?}"
        );
    });
}

/// 折叠某根顶层标题并让折叠状态生效（与 document_find 那批用例同一写法）。
fn fold_heading(editor: &mut Editor, index: usize, cx: &mut gpui::Context<Editor>) {
    let heading = editor.document.root_blocks()[index].clone();
    heading.update(cx, |block, _cx| block.folded = true);
    editor.fold_state_version = editor.fold_state_version.wrapping_add(1);
}

#[gpui::test]
async fn a_cross_block_hit_unfolds_the_section_its_tail_lands_in(cx: &mut TestAppContext) {
    init(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(
            cx,
            "# A\n\nneedle 起\n\n# B\n\nneedle 尾\n".to_string(),
            None,
        )
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        fold_heading(editor, 2, cx);
    });
    search_document_regex(&editor, r"needle 起[\s\S]*?needle 尾", cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.document.root_blocks()[2].read(cx).folded,
            "用例前提：搜索本身不该把 B 段展开"
        );
    });
    editor.update(cx, |editor, cx| {
        editor.find_next_document_match(false, cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        let active = editor.workspace.document_active_range.clone().expect("要有活动命中");
        assert!(!active.is_empty(), "命中要盖住两段");
        assert!(
            !editor.document.root_blocks()[2].read(cx).folded,
            "尾段落在全是折叠的那段内容里时，这一节必须跟着展开——只看命中起点做不到"
        );
    });
}

#[gpui::test]
async fn a_cross_block_hit_unfolds_every_section_it_touches(cx: &mut TestAppContext) {
    init(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(
            cx,
            "# A\n\nneedle 起\n\n# B\n\nneedle 尾\n".to_string(),
            None,
        )
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        fold_heading(editor, 0, cx);
        fold_heading(editor, 2, cx);
    });
    search_document_regex(&editor, r"needle 起[\s\S]*?needle 尾", cx);
    editor.update(cx, |editor, cx| {
        editor.find_next_document_match(false, cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        let folded = editor
            .document
            .root_blocks()
            .iter()
            .filter(|block| block.read(cx).folded)
            .count();
        assert_eq!(folded, 0, "两端各自所在的折叠节都要展开：{folded}");
    });
}

/// 起一个文档范围的正则搜索并设好替换文本。
fn search_and_set_replacement(
    editor: &gpui::Entity<Editor>,
    query: &str,
    replacement: &str,
    cx: &mut TestAppContext,
) {
    editor.update(cx, |editor, _cx| {
        editor.workspace.replace_query = replacement.to_string();
    });
    search_document_regex(editor, query, cx);
}

#[gpui::test]
async fn replace_all_replaces_cross_block_hits_for_real(cx: &mut TestAppContext) {
    init(cx);
    // 两根命中各自跨过一道块界。改前映射换算装不下它们，`continue` 静默跳过——
    // 用户按「全部替换」，计数报 0、正文一个字没动。
    let source = "alpha\n\nmid\n\nalpha\n\nend\n";
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.to_string(), None));
    cx.run_until_parked();
    search_and_set_replacement(&editor, r"alpha\n\n\w+", "Z", cx);
    editor.read_with(cx, |editor, _cx| {
        let ranges = editor
            .workspace
            .document_matches
            .as_ref()
            .expect("命中表要有")
            .hits
            .iter()
            .map(|hit| hit.range.clone())
            .collect::<Vec<_>>();
        assert_eq!(ranges, vec![0..10, 12..22], "用例前提：两条都是跨块命中");
    });

    let count = editor.update_in(cx, |editor, window, cx| {
        editor.replace_all_document_matches(window, cx)
    });
    cx.run_until_parked();
    assert_eq!(count, 2, "两条跨块命中都要真换掉（改前这里是 0）");

    // 闸门一：替换后重扫，命中必须归零。
    search_document_regex(&editor, r"alpha\n\n\w+", cx);
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor
                .workspace
                .document_matches
                .as_ref()
                .map(|table| table.hits.is_empty())
                .unwrap_or(true),
            "替换完再搜同一条模式必须一条命中都没有"
        );
    });

    // 闸门二：整篇的字节差异恰好等于「按同序把这两段换成 Z」，不多不少。
    let after = editor.read_with(cx, |editor, cx| editor.current_document_source(cx));
    let mut expected = source.to_string();
    // 「alpha\n\nmid」是 0..10（10 个字节），「alpha\n\nend」是 12..22。
    for range in [12..22, 0..10] {
        expected.replace_range(range, "Z");
    }
    assert_eq!(after, expected, "只能改这两段字节：{after:?}");
}

#[gpui::test]
async fn undo_steps_a_cross_block_replace_all_back_whole(cx: &mut TestAppContext) {
    init(cx);
    // 一批跨块替换共用一次撤销捕获：撤销要整批退回，不能留下半替的文档。
    let source = "alpha\n\nmid\n\nalpha\n\nend\n";
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.to_string(), None));
    cx.run_until_parked();
    search_and_set_replacement(&editor, r"alpha\n\n\w+", "Z", cx);
    let count = editor.update_in(cx, |editor, window, cx| {
        editor.replace_all_document_matches(window, cx)
    });
    assert_eq!(count, 2);
    cx.run_until_parked();

    // 撤销要能把这一批退回原文：一段一条撤销记录，逐段退回，最后一笔回到初始文本。
    let mut restored = String::new();
    for step in 0..6 {
        editor.update(cx, |editor, cx| editor.undo_document(cx));
        cx.run_until_parked();
        restored = editor.read_with(cx, |editor, cx| editor.current_document_source(cx));
        assert!(
            restored.contains("Z") || restored == source,
            "第 {step} 次撤销之后不该出现替换项之外的内容：{restored:?}"
        );
        if restored == source {
            break;
        }
    }
    assert_eq!(restored, source, "撤销必须把这一批跨块替换逐段退回原文");
}

#[gpui::test]
async fn replace_all_mixes_block_and_cross_block_hits(cx: &mut TestAppContext) {
    init(cx);
    // 同一批里既有块内命中也有跨块命中：两条链路混用时偏移不能互相踩坏。
    let source = "alpha here\n\nalpha\n\nmid\n";
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.to_string(), None));
    cx.run_until_parked();
    search_and_set_replacement(&editor, r"alpha\n\n\w+|alpha here", "Q", cx);
    let count = editor.update_in(cx, |editor, window, cx| {
        editor.replace_all_document_matches(window, cx)
    });
    cx.run_until_parked();
    let after = editor.read_with(cx, |editor, cx| editor.current_document_source(cx));
    let mut expected = source.to_string();
    // 「alpha here」是块内命中（0..10），「alpha\n\nmid」是跨块命中（12..22）。
    expected.replace_range(12..22, "Q");
    expected.replace_range(0..10, "Q");
    assert_eq!(
        after, expected,
        "计数 {count}：块内与跨块两条链路混用时字节差异必须正好等于替换项"
    );
    assert_eq!(count, 2);
}

#[gpui::test]
async fn jumping_to_a_cross_block_hit_centers_it_in_the_viewport(cx: &mut TestAppContext) {
    init(cx);
    // 阶段 3 闸门②的最后一项：跨块命中跳过去要能把视口滚到它居中。跨块命中的
    // 选区落在**两根**块上，居中用的边界必须由起点那根提供——这条钉的就是这件事。
    let mut md = String::from("# 标题\n\n开头 alpha\n");
    for index in 0..40 {
        md.push_str(&format!("\n填充段落 {}，撑开滚动空间。\n", index));
    }
    md.push_str("\nalpha\n\n收尾\n");
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, md.to_string(), None));
    cx.run_until_parked();
    search_document_regex(&editor, r"alpha\n\n收尾", cx);
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor
                .workspace
                .document_matches
                .as_ref()
                .expect("命中表要有")
                .hits
                .len(),
            1,
            "用例前提：一条跨块命中"
        );
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.advance_search_match(false, window, cx));
    });
    for _ in 0..16 {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }
    let drift = editor.read_with(cx, |editor, cx| {
        editor
            .active_entity_id
            .and_then(|id| {
                editor
                    .document
                    .visible_blocks()
                    .into_iter()
                    .find(|visible| visible.entity.entity_id() == id)
                    .map(|visible| visible.entity.clone())
            })
            .and_then(|target| target.read(cx).active_range_or_cursor_bounds())
            .map(|bounds| {
                let target_center = f32::from(bounds.top()) + f32::from(bounds.size.height) * 0.5;
                let viewport_center = f32::from(editor.scroll_handle.bounds().top())
                    + f32::from(editor.scroll_handle.bounds().size.height) * 0.5;
                (target_center - viewport_center).abs()
            })
    });
    let drift = drift.expect("跨块命中跳过去之后应有可测的边界");
    assert!(
        drift <= 2.0,
        "跨块命中必须被精确居中，实际偏差 {drift}px"
    );
}

#[gpui::test]
async fn clicking_a_cross_line_workspace_hit_broken_by_an_edit_falls_back(
    cx: &mut TestAppContext,
) {
    init(cx);
    // 阶段 3 闸门⑦：跨行命中所在文件被未保存的编辑破坏（中间那行没了），
    // ordinal 对位取不到那条命中——退回第一个仍然成立的命中，不 panic、
    // 不选到半个字符上。
    let root = std::env::temp_dir().join(format!(
        "velora-xline-broken-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).expect("create dir");
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("note.md");
    fs::write(&path, "alpha\n\nmid\n\nalpha\n\nlast\n").expect("write");
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(&root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.workspace.is_open = true;
            editor.open_workspace_file(path.clone(), window, cx);
        });
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.workspace.search_scope = WorkspaceSearchScope::Workspace;
        editor.workspace.search_use_regex = true;
        editor.workspace.search_query = r"alpha\n\n\w+".to_string();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    let second = editor.read_with(cx, |editor, _| {
        editor
            .workspace
            .search_results
            .iter()
            .position(|hit| hit.match_ordinal == Some(1))
            .expect("磁盘上第二条含命中行要有结果")
    });

    // 删掉第一处跨行命中用到的两段（"alpha" 与 "mid"）：缓冲区里只剩下
    // "alpha\n\nlast"，也就是只有一个命中组，ordinal 1 已经越界。
    editor.update(cx, |editor, cx| {
        for block in editor.document.root_blocks()[..2].to_vec() {
            block.update(cx, |block, cx| {
                block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
                block.replace_text_in_visible_range(0..block.visible_len(), "", None, false, cx);
            });
        }
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.open_search_hit(second, window, cx));
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        let range = editor
            .workspace
            .document_active_range
            .clone()
            .expect("命中被编辑破坏后也要落在一个区间上");
        let source = editor.current_document_source(cx);
        let text = source
            .get(range.clone())
            .unwrap_or_else(|| panic!("落点越界：{range:?} / 长度 {}", source.len()));
        assert!(
            text.contains("alpha"),
            "退回的落点必须仍然是命中文本的一部分：{text:?}"
        );
        assert!(
            source.is_char_boundary(range.start) && source.is_char_boundary(range.end),
            "退回的落点两端都必须在字符边界上：{range:?}"
        );
    });
}
