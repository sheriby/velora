//! 搜索链路的性能闸门（方案文档 §7.4），笔法沿用 `editor/tests/perf_budgets.rs`：
//! 夹具缺失就跳过、墙钟取最小值、机制断言用计数器差值。
//!
//! 数字都量自 **debug profile**（`cargo test` 跑的那一档），闸门设在实测值的两倍
//! 上下——先把退步挡住，不把最优值钉死。release 档还要快一到两个数量级。
//! 实测（10 MiB 夹具 / 53 227 条命中）：整篇扫描 50–175 ms；第一次「下一个」要走
//! 一遍全量高亮同步（9.95 s，这一档的账登记在方案 §11 缺陷 #9），之后每次 125 ms。
//! 1 MiB / 5 322 条命中：一次全量 317 ms，之后每次 18.8 ms。
//! 对照换引擎**之前**的口径：旧实现每按一次都重扫整篇再线性找下一个，10 MiB 中文
//! 查询在 release 探针里实测 61 ms 一次按键（§2.1），需要回绕时等于扫两遍。

use super::super::{Editor, WorkspaceSearchScope, WorkspaceTab};
use gpui::TestAppContext;
use std::time::{Duration, Instant};

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

/// 打开夹具文档、把搜索面板切到文档范围并跑一遍查询。
fn open_document(
    cx: &mut TestAppContext,
    fixture: &str,
    query: &str,
) -> Option<(gpui::Entity<Editor>, usize)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/perf")
        .join(fixture);
    if !path.is_file() {
        eprintln!(
            "skipping: generate fixtures with \
             `node scripts/generate-fixtures.mjs tests/fixtures/perf`"
        );
        return None;
    }
    init(cx);
    let markdown = std::fs::read_to_string(&path).expect("read fixture");
    let bytes = markdown.len();
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    let deadline = Instant::now() + Duration::from_secs(600);
    while editor.read_with(cx, |editor, _| editor.document.pending_tail().is_some()) {
        assert!(Instant::now() < deadline, "续建未完成");
        cx.run_until_parked();
    }
    for _ in 0..4 {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }
    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = WorkspaceTab::Search;
        editor.workspace.search_scope = WorkspaceSearchScope::Document;
        editor.workspace.search_query = query.to_string();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    Some((editor, bytes))
}

/// 10 MiB 文档：整篇扫描与循环跳转各自的预算。
#[gpui::test]
async fn searching_a_ten_mib_document_stays_within_budget(cx: &mut TestAppContext) {
    let Some((editor, bytes)) = open_document(cx, "ten-mib.md", "段落") else {
        return;
    };

    // 整篇扫描：三个新查询各付一次（缓存键含查询，所以每次都是真扫）。
    let mut scans = Vec::new();
    for query in ["第一项", "English", "待办项"] {
        let started = Instant::now();
        editor.update(cx, |editor, cx| {
            editor.workspace.search_query = query.to_string();
            editor.document_matches(cx)
        });
        scans.push(started.elapsed());
    }
    let scan = *scans.iter().min().expect("至少量到一次");

    // 循环跳转：先按一次把整篇高亮铺好，预算量的是**之后**每一次的代价——
    // 那才是用户连按 F3 时的手感。
    editor.update(cx, |editor, cx| editor.find_next_document_match(false, cx));
    let started = Instant::now();
    for _ in 0..20 {
        editor.update(cx, |editor, cx| editor.find_next_document_match(false, cx));
    }
    let cycling = started.elapsed();
    let per_press = cycling / 20;
    eprintln!(
        "[measure] {bytes} 字节文档：整篇扫描三巡 {scans:?}（取最小 {scan:?}），\
         20 次跳转合计 {cycling:?}、单次平均 {per_press:?}"
    );

    assert!(
        scan <= Duration::from_millis(400),
        "10 MiB 整篇扫描取最小也有 {scan:?}，超过 400 ms 预算（实测 50–175 ms）"
    );
    assert!(
        per_press <= Duration::from_millis(250),
        "单次循环跳转平均 {per_press:?}，超过 250 ms 预算（实测 125 ms）"
    );
}

/// 跳转路径一次都不许重扫整篇文档。
#[gpui::test]
async fn document_jump_does_not_rescan_the_buffer(cx: &mut TestAppContext) {
    let Some((editor, _bytes)) = open_document(cx, "ten-mib.md", "段落") else {
        return;
    };
    // 命中表只在「查询、开关、文档内容」任一变了时重算。跳转、来回切、
    // 反复量都不该再扫一遍整篇。
    let before = editor.read_with(cx, |editor, _| editor.document_match_scans.get());
    for _ in 0..50 {
        editor.update(cx, |editor, cx| {
            editor.find_next_document_match(false, cx);
        });
    }
    for _ in 0..20 {
        editor.update(cx, |editor, cx| {
            editor.find_next_document_match(true, cx);
        });
    }
    let after = editor.read_with(cx, |editor, _| editor.document_match_scans.get());
    assert_eq!(
        after - before,
        0,
        "70 次循环跳转期间命中表重扫了 {} 次，跳转必须只取表上索引",
        after - before
    );

    // 反向性质：一改查询就必须重算，不能拿着旧表给新查询用。
    editor.update(cx, |editor, cx| {
        editor.workspace.search_query = "English".to_string();
        editor.document_matches(cx);
    });
    let changed = editor.read_with(cx, |editor, _| editor.document_match_scans.get());
    assert_eq!(changed - after, 1, "换了查询应当正好重扫一次，实际 {changed}");
}

/// 连按「下一个」只许挪活动命中那一处的标记，不许把每根有命中的块重算一遍。
///
/// 1 MiB 夹具里这条查询有 5 322 处命中、铺在同样多的块上。改前每按一次都要为
/// 每根沾到命中的块重筛整张表并重算它自己的 source mapping（5 322 次映射重建，
/// 实测 317 ms 一次按键）；现在一次按键只碰活动命中盖住的那一两根块（18.8 ms）。
#[gpui::test]
async fn jumping_between_matches_leaves_the_other_blocks_alone(cx: &mut TestAppContext) {
    let Some((editor, _bytes)) = open_document(cx, "one-mib.md", "English") else {
        return;
    };
    let hits = editor.read_with(cx, |editor, _| {
        editor
            .workspace
            .document_matches
            .as_ref()
            .map(|table| table.hits.len())
            .unwrap_or(0)
    });
    assert!(hits > 4_000, "夹具应有几千条命中，实际 {hits}");

    // 先把全量高亮铺好（第一次同步付整篇的账，那是应有代价）。
    editor.update(cx, |editor, cx| editor.find_next_document_match(false, cx));

    let before = editor.read_with(cx, |editor, _| {
        (
            editor.source_mapping_builds.get(),
            editor.document_match_scans.get(),
        )
    });
    let mut markers = Vec::new();
    for _ in 0..10 {
        editor.update(cx, |editor, cx| editor.find_next_document_match(false, cx));
        markers.push(editor.read_with(cx, |editor, _| {
            editor
                .search_active_blocks
                .iter()
                .map(|entity| entity.entity_id())
                .collect::<Vec<_>>()
        }));
    }
    let after = editor.read_with(cx, |editor, _| {
        (
            editor.source_mapping_builds.get(),
            editor.document_match_scans.get(),
        )
    });
    eprintln!(
        "[measure] {hits} 条命中，10 次跳转期间单块 mapping 重建 {} 次、命中表重扫 {} 次",
        after.0 - before.0,
        after.1 - before.1
    );

    // 机制断言：一次跳转重算的单块 mapping 不随命中数长。实测每次按键 5 次
    // （活动命中盖住的块 1–2 次，加上落选区与滚动锚点那条路 3 次），
    // 改前是每根有命中的块一次，也就是 5 322 次。
    assert!(
        (after.0 - before.0) as usize <= 10 * 10,
        "10 次跳转重算了 {} 次单块 mapping，一次按键不该超过 10 次",
        after.0 - before.0
    );
    assert_eq!(after.1 - before.1, 0, "跳转期间命中表被重扫");

    // 快路径不许把活动标记弄丢：每次都得有块带着标记，而且标记在往前走。
    for (index, marker) in markers.iter().enumerate() {
        assert!(!marker.is_empty(), "第 {index} 次跳转后没有块带活动命中标记");
    }
    assert!(
        markers.windows(2).any(|pair| pair[0] != pair[1]),
        "连按 10 次「下一个」，活动命中标记一直停在同一批块上：{markers:?}"
    );
}
