use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    AnyWindowHandle, AppContext, ClickEvent, EntityInputHandler, KeyDownEvent, Keystroke,
    Modifiers, TestAppContext, VisualTestContext, WindowBounds, WindowHandle, px,
};

use super::{Editor, MountedRun, ViewMode};
use crate::components::{
    Block, BlockEvent, BlockKind, BlockRecord, CloseWindow, Delete, DeleteBack, FocusNext,
    ImageReferenceDefinitions, ImageResolvedSource, InlineTextTree, Newline, QuitApplication,
    SaveDocument, TableCellInlineImageSegment, TableColumnAlignment, UndoCaptureKind,
    parse_table_cell_inline_images, superscript_ordinal,
};
use crate::export::ExportFormat;
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::{Theme, ThemeManager};
fn init_editor_test_app(cx: &mut TestAppContext) {
    cx.update(|cx| {
        I18nManager::init(cx);
        ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

fn temp_markdown_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "velora-{test_name}-{}-{nanos}.md",
        std::process::id()
    ))
}

fn temp_export_path(test_name: &str, extension: &str) -> PathBuf {
    let mut path = temp_markdown_path(test_name);
    path.set_extension(extension);
    path
}

fn redraw(cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

fn activate_visual_window(cx: &mut VisualTestContext) -> AnyWindowHandle {
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    cx.cx
        .update(|cx| cx.active_window().expect("window should be active"))
}

/// 读取窗口 frame 为 (x, y, width, height)，用于窗口位置/大小断言。
fn windowed_rect(handle: &WindowHandle<Editor>, cx: &mut TestAppContext) -> (i32, i32, i32, i32) {
    handle
        .update(cx, |_editor, window, _cx| {
            let bounds = match window.window_bounds() {
                WindowBounds::Windowed(bounds)
                | WindowBounds::Maximized(bounds)
                | WindowBounds::Fullscreen(bounds) => bounds,
            };
            (
                f32::from(bounds.origin.x) as i32,
                f32::from(bounds.origin.y) as i32,
                f32::from(bounds.size.width) as i32,
                f32::from(bounds.size.height) as i32,
            )
        })
        .expect("window should be open")
}

#[gpui::test]
#[ignore = "手动大文件诊断；设置 VELORA_PERF_FILE 后单独运行"]
async fn manual_markdown_load_probe(cx: &mut TestAppContext) {
    let path = std::env::var("VELORA_PERF_FILE").expect("需要设置 VELORA_PERF_FILE");
    let markdown = fs::read_to_string(path).expect("性能样本必须是 UTF-8 文本");
    let bytes = markdown.len();
    init_editor_test_app(cx);

    let start = Instant::now();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        let start = Instant::now();
        let editor = Editor::from_markdown(cx, markdown, None);
        println!(
            "editor_constructor_ms={:.1}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        editor
    });
    let construct = start.elapsed();
    let (mode, rows) = editor.read_with(cx, |editor, _cx| {
        (editor.view_mode, editor.document.visible_blocks().len())
    });
    assert!(matches!(mode, ViewMode::Rendered));

    let start = Instant::now();
    redraw(cx);
    let first_draw = start.elapsed();
    let steady_count = std::env::var("VELORA_STEADY_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let mut steady_draws = Vec::with_capacity(steady_count);
    for _ in 0..steady_count {
        let start = Instant::now();
        redraw(cx);
        steady_draws.push(start.elapsed().as_secs_f64() * 1000.0);
        if steady_count > 100 {
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }
    steady_draws.sort_by(f64::total_cmp);
    let p95 = steady_draws[steady_draws.len() - 1];
    println!(
        "bytes={bytes} rows={rows} construct_ms={:.1} first_draw_ms={:.1} steady_p95_ms={p95:.1}",
        construct.as_secs_f64() * 1000.0,
        first_draw.as_secs_f64() * 1000.0
    );

    let start = Instant::now();
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        editor.active_entity_id = Some(first.entity_id());
        first.update(cx, |block, cx| {
            block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(0..0, "测", None, false, cx);
        });
    });
    let edit_update = start.elapsed();
    let start = Instant::now();
    redraw(cx);
    let edit_draw = start.elapsed();
    println!(
        "edit_update_ms={:.1} edit_draw_ms={:.1}",
        edit_update.as_secs_f64() * 1000.0,
        edit_draw.as_secs_f64() * 1000.0
    );
    let start = Instant::now();
    let source_bytes = editor.read_with(cx, |editor, cx| editor.current_document_source(cx).len());
    println!(
        "serialized_bytes={source_bytes} serialize_ms={:.1}",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

#[gpui::test]
#[ignore = "手动大文件诊断；设置 VELORA_PERF_FILE 后单独运行"]
async fn manual_code_load_probe(cx: &mut TestAppContext) {
    let path = std::env::var("VELORA_PERF_FILE").expect("需要设置 VELORA_PERF_FILE");
    let source_path = PathBuf::from(&path);
    let source = fs::read_to_string(&source_path).expect("性能样本必须是 UTF-8 文本");
    let bytes = source.len();
    init_editor_test_app(cx);

    let start = Instant::now();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        let start = Instant::now();
        let editor = Editor::from_file_source(cx, source, Some(source_path));
        println!(
            "editor_constructor_ms={:.1}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        editor
    });
    let construct = start.elapsed();
    let (mode, rows) = editor.read_with(cx, |editor, _cx| {
        (
            editor.view_mode,
            editor.document.visible_blocks().len(),
        )
    });
    assert!(matches!(mode, ViewMode::Source), "代码文件应进入 Source 模式");

    let start = Instant::now();
    redraw(cx);
    let first_draw = start.elapsed();
    let steady_count = std::env::var("VELORA_STEADY_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let mut steady_draws = Vec::with_capacity(steady_count);
    for _ in 0..steady_count {
        let start = Instant::now();
        redraw(cx);
        steady_draws.push(start.elapsed().as_secs_f64() * 1000.0);
        if steady_count > 100 {
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }
    steady_draws.sort_by(f64::total_cmp);
    let p95 = steady_draws[steady_draws.len() - 1];
    println!(
        "bytes={bytes} rows={rows} construct_ms={:.1} first_draw_ms={:.1} steady_p95_ms={p95:.1}",
        construct.as_secs_f64() * 1000.0,
        first_draw.as_secs_f64() * 1000.0,
    );

    let start = Instant::now();
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        editor.active_entity_id = Some(first.entity_id());
        first.update(cx, |block, cx| {
            block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(0..0, "x", None, false, cx);
        });
    });
    let edit_update = start.elapsed();
    let start = Instant::now();
    redraw(cx);
    let edit_draw = start.elapsed();
    println!(
        "edit_update_ms={:.1} edit_draw_ms={:.1}",
        edit_update.as_secs_f64() * 1000.0,
        edit_draw.as_secs_f64() * 1000.0
    );
    let start = Instant::now();
    let source_bytes = editor.read_with(cx, |editor, cx| editor.current_document_source(cx).len());
    println!(
        "serialized_bytes={source_bytes} serialize_ms={:.1}",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

#[gpui::test]
async fn code_source_chunks_round_trip_and_continue_line_numbers(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = String::new();
    for index in 0..1_200 {
        source.push_str(&format!("line-{index}\n"));
    }
    let path = std::env::temp_dir().join(format!("velora-chunk-roundtrip-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");
    let expected_source = source.clone();

    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_file_source(cx, source.clone(), Some(path.clone()))
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert!(matches!(editor.view_mode, ViewMode::Source));
        let blocks = editor.document.visible_blocks();
        assert_eq!(blocks.len(), 3, "1200 行 + 行尾换行应切成 512/512/177 三块");

        let mut expected_start = 1usize;
        for visible in blocks {
            visible.entity.read_with(cx, |block, _cx| {
                assert_eq!(block.source_line_start(), expected_start);
            });
            expected_start += visible.entity.read_with(cx, |block, _cx| {
                block.display_text().split('\n').count()
            });
        }

        let serialized = editor.current_document_source(cx);
        assert_eq!(serialized, expected_source, "分块序列化必须逐字节还原源码");
    });
}

#[gpui::test]
async fn code_source_with_crlf_round_trips_through_chunks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let crlf_source = "alpha\n\nbravo\n".to_string().replace('\n', "\r\n");
    let expected_source = crlf_source.clone();
    let path = std::env::temp_dir().join(format!("velora-chunk-crlf-{}.txt", std::process::id()));

    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_file_source(cx, crlf_source.clone(), Some(path.clone()))
    });
    editor.read_with(cx, |editor, cx| {
        assert!(editor.code_uses_crlf, "应记录 CRLF 标记");
        let serialized = editor.serialized_document_text(cx);
        assert_eq!(serialized, expected_source, "保存序列化必须还原 CRLF");
    });
}

#[gpui::test]
async fn small_code_files_stay_single_chunk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "one\ntwo\nthree\n".to_string();
    let path = std::env::temp_dir().join(format!("velora-chunk-small-{}.toml", std::process::id()));
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_file_source(cx, source, Some(path)));
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.document.visible_blocks().len(), 1);
    });
}

#[gpui::test]
async fn progressive_import_blocks_render_after_streaming(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 12000 块 > FIRST_CHUNK_ROOTS(2000)：首帧只建 2000，其余流式续建。
    let mut markdown = String::new();
    for index in 0..6_000 {
        markdown.push_str(&format!("# Heading {index}\n\nParagraph {index} body text.\n\n"));
    }
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    // 流式续建排空后，行计划必须反映全部块（此前 plan 键不含块数，
    // 续建只 append_roots + notify，渲染停留在首帧的 2000 行）。
    cx.run_until_parked();
    redraw(cx);
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().len();
        assert!(
            (11_900..=12_100).contains(&visible),
            "流式续建完成后可见块应全部就位，实际 {}",
            visible
        );
        let plan_rows = editor
            .rendered_row_plan
            .as_ref()
            .expect("行计划应在渲染后存在")
            .rows
            .len();
        assert_eq!(
            plan_rows, visible,
            "行计划必须随流式续建刷新，否则新块不渲染（用户可见大片空白）"
        );
    });
}

/// P7 预算守卫：1 MiB 级代码文档同步构造必须在预算内（当前 dev 实测
/// ~50ms，给 10x 余量），流式续建完成后序列化必须逐字节还原。
#[gpui::test]
async fn large_code_document_opens_within_budget(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = String::with_capacity(1 << 20);
    for index in 0..9_891 {
        source.push_str(&format!(
            "2026-09-28T12:00:00.000Z INFO  [mod{}::sub] request id={} duration={}ms status=OK\n",
            index % 7,
            index,
            index % 97
        ));
    }
    assert!(source.len() > 800_000 && source.len() < 1_100_000);
    let path = std::env::temp_dir().join(format!("velora-budget-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write budget fixture");
    let expected_source = source.clone();

    let start = Instant::now();
    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    let open_elapsed = start.elapsed();
    assert!(
        open_elapsed.as_millis() < 500,
        "1MiB 代码文档打开耗时 {}ms，超出 500ms 预算",
        open_elapsed.as_millis()
    );
    cx.run_until_parked();

    editor
        .read_with(cx, |editor, cx| {
            assert!(matches!(editor.view_mode, ViewMode::Source));
            let blocks = editor.document.visible_blocks().len();
            assert!(
                (17..=22).contains(&blocks),
                "1MiB 日志应切成约 20 块，实际 {}",
                blocks
            );
            assert_eq!(
                editor.current_document_source(cx),
                expected_source,
                "流式续建完成后必须逐字节还原"
            );
        })
        .expect("editor window should be open");
}

/// 700 行 + 行尾换行 → 701 个行片段 → 512/189 两块。
fn chunk_boundary_source() -> String {
    let mut source = String::new();
    for index in 0..700 {
        source.push_str(&format!("line-{index}\n"));
    }
    source
}

#[gpui::test]
async fn backspace_at_chunk_start_merges_previous_chunk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = chunk_boundary_source();
    let path = std::env::temp_dir().join(format!("velora-chunk-bs-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");
    let expected_source = source.clone();
    let expected_merged = source.replace("line-511\nline-512", "line-511line-512");

    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    cx.run_until_parked();
    editor
        .update(cx, |editor, window, cx| {
            let blocks = editor.document.flatten_visible_blocks();
            assert_eq!(blocks.len(), 2);
            let second = blocks[1].entity.clone();
            second.update(cx, |block, cx| {
                block.selected_range = 0..0;
                block.on_delete_back(&DeleteBack, window, cx);
            });
        })
        .expect("editor window should be open");
    cx.run_until_parked();

    editor
        .read_with(cx, |editor, cx| {
            assert_eq!(
                editor.document.visible_blocks().len(),
                1,
                "块首退格应并入前块"
            );
            assert_eq!(editor.current_document_source(cx), expected_merged);
        })
        .expect("editor window should be open");

    editor
        .update(cx, |editor, _window, cx| editor.undo_document(cx))
        .expect("editor window should be open");
    cx.run_until_parked();
    editor
        .read_with(cx, |editor, cx| {
            assert_eq!(editor.document.visible_blocks().len(), 2, "undo 恢复分块");
            assert_eq!(editor.current_document_source(cx), expected_source);
        })
        .expect("editor window should be open");
}

#[gpui::test]
async fn enter_at_chunk_end_inserts_boundary_chunk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = chunk_boundary_source();
    let path = std::env::temp_dir().join(format!("velora-chunk-enter-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");
    let expected_source = source.replace("line-511\nline-512", "line-511\n\nline-512");

    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    cx.run_until_parked();
    editor
        .update(cx, |editor, window, cx| {
            let blocks = editor.document.flatten_visible_blocks();
            assert_eq!(blocks.len(), 2);
            let first = blocks[0].entity.clone();
            let end = first.read(cx).display_text().len();
            first.update(cx, |block, cx| {
                block.selected_range = end..end;
                block.on_newline(&Newline, window, cx);
            });
        })
        .expect("editor window should be open");
    cx.run_until_parked();

    editor
        .read_with(cx, |editor, cx| {
            let blocks = editor.document.visible_blocks();
            assert_eq!(blocks.len(), 3, "块尾回车应插入一个空块");
            let starts: Vec<usize> = blocks
                .iter()
                .map(|visible| visible.entity.read(cx).source_line_start())
                .collect();
            assert_eq!(starts, vec![1, 513, 514], "行号续号应随插入刷新");
            assert_eq!(editor.current_document_source(cx), expected_source);
        })
        .expect("editor window should be open");
}

#[gpui::test]
async fn delete_at_chunk_end_merges_next_chunk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = chunk_boundary_source();
    let path = std::env::temp_dir().join(format!("velora-chunk-del-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");
    let expected_merged = source.replace("line-511\nline-512", "line-511line-512");

    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    cx.run_until_parked();
    editor
        .update(cx, |editor, window, cx| {
            let blocks = editor.document.flatten_visible_blocks();
            assert_eq!(blocks.len(), 2);
            let first = blocks[0].entity.clone();
            let end = first.read(cx).display_text().len();
            first.update(cx, |block, cx| {
                block.selected_range = end..end;
                block.on_delete(&Delete, window, cx);
            });
        })
        .expect("editor window should be open");
    cx.run_until_parked();

    editor
        .read_with(cx, |editor, cx| {
            assert_eq!(
                editor.document.visible_blocks().len(),
                1,
                "块尾前删应并入后块"
            );
            assert_eq!(editor.current_document_source(cx), expected_merged);
        })
        .expect("editor window should be open");
}

#[gpui::test]
async fn targeted_source_mapping_matches_later_blocks_and_table_cells(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "intro\n\n## heading\n\n| Name | Value |\n| --- | --- |\n| A | B |".into();
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));
    editor.read_with(cx, |editor, cx| {
        let all = editor.build_source_target_mappings(cx);
        assert!(all.len() >= 4);
        for expected in all.iter().skip(1) {
            let actual = editor
                .source_mapping_for_entity(expected.entity.entity_id(), cx)
                .expect("later block or table cell should have a source mapping");
            assert_eq!(actual.full_source_range, expected.full_source_range);
            assert_eq!(actual.content_to_source, expected.content_to_source);
            assert_eq!(actual.source_to_content, expected.source_to_content);
        }
    });
}

#[test]
fn centered_column_ratio_stays_full_before_shrink_start() {
    let theme = Theme::default_theme();
    assert_eq!(Editor::centered_column_ratio(900.0, &theme.dimensions), 1.0);
    assert_eq!(
        Editor::centered_column_ratio(theme.dimensions.centered_shrink_start, &theme.dimensions),
        1.0
    );
}

#[test]
fn centered_column_ratio_reaches_new_minimum() {
    let theme = Theme::default_theme();
    let ratio =
        Editor::centered_column_ratio(theme.dimensions.centered_shrink_end, &theme.dimensions);
    assert!((ratio - 0.58).abs() < f32::EPSILON);
}

#[test]
fn scrollbar_geometry_and_inverse_mapping_stay_aligned() {
    let geometry = Editor::scrollbar_geometry(400.0, 600.0, 300.0);
    assert_eq!(geometry.track_height, 400.0);
    assert!(geometry.thumb_height >= 28.0);
    assert!((geometry.thumb_top - (400.0 - geometry.thumb_height) * 0.5).abs() < 0.001);

    let scroll_y = Editor::scroll_offset_for_thumb_top(
        geometry.thumb_top,
        geometry.track_height,
        geometry.thumb_height,
        geometry.max_scroll_y,
    );
    assert!((scroll_y - 300.0).abs() < 0.001);
}

#[test]
fn scrollbar_geometry_handles_a_newly_mounted_short_viewport() {
    let geometry = Editor::scrollbar_geometry(20.0, 100.0, 0.0);
    assert_eq!(geometry.track_height, 20.0);
    assert_eq!(geometry.thumb_height, 20.0);
}

#[test]
fn scrollbar_offset_mapping_clamps_to_track_bounds() {
    let geometry = Editor::scrollbar_geometry(300.0, 450.0, 0.0);
    assert_eq!(
        Editor::scroll_offset_for_thumb_top(
            -25.0,
            geometry.track_height,
            geometry.thumb_height,
            geometry.max_scroll_y,
        ),
        0.0
    );
    assert_eq!(
        Editor::scroll_offset_for_thumb_top(
            999.0,
            geometry.track_height,
            geometry.thumb_height,
            geometry.max_scroll_y,
        ),
        geometry.max_scroll_y
    );
}

/// Equal-height rows as per-row footprints, the input `rendered_window` takes.
fn uniform_strides(count: usize, height: f32) -> Vec<f32> {
    vec![height; count]
}

#[test]
fn rendered_window_culls_offscreen_rows() {
    // 100 rows of 50px (total 5000). Scroll 2000, viewport 400 -> band [2000, 2400].
    let strides = uniform_strides(100, 50.0);
    let window = Editor::rendered_window(&strides, 2000.0, 400.0, 0.0, None, 16.0);

    // Row i spans [50i, 50i+50). bottom>=2000 -> i>=39; top<=2400 -> i<=48.
    assert_eq!(window.run_start, 39);
    assert_eq!(window.run_end, 49);
    assert!((window.top_h - 1950.0).abs() < 0.01);
    assert!((window.bottom_h - 2550.0).abs() < 0.01);
}

#[test]
fn rendered_window_keeps_focus_row_mounted() {
    let strides = uniform_strides(100, 50.0);
    // Viewport at the top, caret parked far below at row 80.
    let window = Editor::rendered_window(&strides, 0.0, 400.0, 0.0, Some(80), 16.0);

    // The caret rides its own island; the rows above it stay culled.
    assert_eq!(window.run_start, 0);
    assert_eq!(window.run_end, 9);
    let island = window.focus_island.expect("caret row stays mounted");
    assert_eq!(island.row, 80);
    assert!((island.lead_h - 3550.0).abs() < 0.01);
}

#[test]
fn rendered_window_focus_above_run_does_not_widen_it() {
    // Reading downward leaves the caret at the top of the document, so the rows
    // between it and the viewport must stay culled.
    let strides = uniform_strides(100, 50.0);
    let window = Editor::rendered_window(&strides, 2000.0, 400.0, 0.0, Some(0), 16.0);

    assert_eq!(window.run_start, 39);
    assert_eq!(window.run_end, 49);
    let island = window.focus_island.expect("caret row stays mounted");
    assert_eq!(island.row, 0);
    assert_eq!(island.lead_h, 0.0);
    assert!((window.top_h - 1900.0).abs() < 0.01);
}

#[test]
fn rendered_window_focus_inside_run_needs_no_island() {
    let strides = uniform_strides(100, 50.0);
    let window = Editor::rendered_window(&strides, 2000.0, 400.0, 0.0, Some(42), 16.0);

    assert_eq!(window.run_start, 39);
    assert_eq!(window.run_end, 49);
    assert_eq!(window.focus_island, None);
}

#[test]
fn rendered_window_tracks_current_scroll_offset() {
    // Scrolling by one row's height shifts the mounted run by exactly one row.
    let strides = uniform_strides(100, 50.0);

    let low = Editor::rendered_window(&strides, 2000.0, 400.0, 0.0, None, 16.0);
    let high = Editor::rendered_window(&strides, 2050.0, 400.0, 0.0, None, 16.0);

    assert_eq!(low.run_start, 39);
    assert_eq!(low.run_end, 49);
    assert_eq!(high.run_start, low.run_start + 1);
    assert_eq!(high.run_end, low.run_end + 1);
}

#[test]
fn rendered_window_has_no_spacer_at_document_edges() {
    let strides = uniform_strides(50, 40.0); // total 2000

    let at_top = Editor::rendered_window(&strides, 0.0, 400.0, 0.0, None, 16.0);
    assert_eq!(at_top.run_start, 0);
    assert_eq!(at_top.top_h, 0.0);
    assert!(at_top.bottom_h > 0.0);

    let at_bottom = Editor::rendered_window(&strides, 1600.0, 400.0, 0.0, None, 16.0);
    assert_eq!(at_bottom.run_end, 50);
    assert_eq!(at_bottom.bottom_h, 0.0);
    assert!(at_bottom.top_h > 0.0);
}

#[test]
fn rendered_window_preserves_total_height() {
    let strides = uniform_strides(200, 37.0);
    let total: f32 = strides.iter().sum();

    for &(scroll_y, viewport_height, focus) in &[
        (0.0f32, 500.0f32, None),
        (3000.0, 500.0, None),
        (37.0 * 150.0, 37.0 * 5.0, Some(10usize)),
    ] {
        let window = Editor::rendered_window(&strides, scroll_y, viewport_height, 200.0, focus, 16.0);
        let rendered: f32 = strides[window.run_start..window.run_end].iter().sum();
        let island: f32 = window
            .focus_island
            .map_or(0.0, |island| island.lead_h + strides[island.row]);
        assert!(
            (window.top_h + rendered + island + window.bottom_h - total).abs() < 0.01,
            "height invariant broken at scroll {scroll_y}"
        );
    }
}

#[test]
fn rendered_window_estimated_row_keeps_culling_active() {
    // Row 60 is an estimated (unmeasured) row; it must not disable culling.
    let mut strides = uniform_strides(100, 50.0);
    strides[60] = 20.0;

    let window = Editor::rendered_window(&strides, 0.0, 400.0, 0.0, None, 16.0);
    assert_eq!(window.run_start, 0);
    assert!(
        window.run_end < strides.len(),
        "a single estimated row must not disable culling"
    );
}

#[test]
fn rendered_window_all_estimated_windows_near_top() {
    // Cold start: all rows estimated. At the top the window still covers the
    // first rows, so the viewport is never blank while heights are learned.
    let strides = uniform_strides(500, 20.0);

    let window = Editor::rendered_window(&strides, 0.0, 400.0, 0.0, None, 16.0);
    assert_eq!(window.run_start, 0);
    assert!(window.run_end < strides.len());
    // A viewport-plus-band worth of rows, not the whole document.
    assert!(window.run_end >= 20);
}

#[gpui::test]
async fn selection_word_count_stays_cheap_on_a_long_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 状态栏每帧都要算选中词数。旧实现走 O(整篇) 的 markdown 序列化 +
    // source mapping 重建（600 块文档实测 38ms/次），长文档拖动选择卡死。
    let markdown = (0..300)
        .map(|index| {
            format!(
                "## 第 {index} 节标题\n\n这是第 {index} 段中文正文，足够长以便换行，含标点与英文 mixed text。\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        let first = visible[0].entity.entity_id();
        let last = visible[visible.len() - 1].entity.entity_id();
        editor.cross_block_selection = Some(super::CrossBlockSelection {
            anchor: super::CrossBlockSelectionEndpoint {
                entity_id: first,
                offset: 0,
            },
            focus: super::CrossBlockSelectionEndpoint {
                entity_id: last,
                offset: usize::MAX,
            },
        });

        let text = editor.selected_visible_text(cx).expect("selection text");
        assert!(text.contains("第 0 节标题"), "选中文本应包含首块内容");

        let calls = 20;
        let start = Instant::now();
        for _ in 0..calls {
            let _ = editor.selected_visible_text(cx);
        }
        let per_call = start.elapsed() / calls;
        println!(
            "[measure] selected_visible_text 单次 = {per_call:?}（可见块 {} 个）",
            visible.len()
        );
        assert!(
            per_call < Duration::from_millis(5),
            "状态栏选词路径又变回 O(整篇) 了：{per_call:?}（可见块 {} 个）",
            visible.len()
        );
    });
}

#[gpui::test]
async fn dragging_inside_a_rendered_code_block_does_not_panic(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 用户报修：在一篇长 markdown（含多个 fence）里，把鼠标放到 ``​`text`` 代码块内部
    // 按住拖动选中时 coredump。这里用同构文档真实模拟。
    let markdown = r#"# 基于 opencode 插件的自定义上下文压缩

## 1. 背景与问题

随着任务复杂度上升，超长会话成为语料治理的核心痛点：单行超长、调试长尾。

## 5. 压缩会话的语料处理：按真实压缩点拆行

压缩解决了运行期问题，但采集侧必须处理「一行跨压缩点全量历史」的超长 + 失真问题。

segment 的重建规则（对齐真实重启上下文）：

```text
seg_1 = [system] + 原始消息序列 + 压缩响应（清洗为纯摘要：从 <stage_summary> 截取）

seg_k (k ≥ 2) = [system]
      + [user: 原始任务 + [RESUME NOTICE] + 上一段阶段摘要]
      + 保留尾部消息（tail_start_id … 压缩宿主消息）
      + 该段内的续作消息
```

配套细节：RESUME NOTICE 明确告知「此前已执行 N 轮、因上下文限制做过阶段压缩」。

```json
{
  "compaction": {
    "auto": true,
    "reserved": 10000,
    "preserve_recent_tokens": 15000
  }
}
```

## 8. 总结

方案核心是把压缩从「运行时兜底机制」升级为「可设计、可治理的一等公民」。
"#;
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.to_string(), None));
    redraw(cx);
    redraw(cx);

    // 找到装着 seg_1 的代码块。
    let (code_block, bounds) = editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks().to_vec();
        for entry in visible {
            let block = entry.entity.clone();
            let state = block.read(cx);
            if state.display_text().contains("seg_1 = [system]") {
                let bounds = state
                    .last_bounds
                    .expect("the code block has layout bounds");
                return (block, bounds);
            }
        }
        panic!("the fenced block with seg_1 was not found");
    });

    // 在代码块内部做多组按住拖动：点进块→聚焦→按行往下/往上拖。
    for (start_ratio, step) in [
        (0.12_f32, 7.0_f32),
        (0.50, 7.0),
        (0.85, -7.0),
        (0.30, 3.0),
    ] {
        let start = gpui::point(
            bounds.left() + px(37.5),
            bounds.top() + bounds.size.height * start_ratio,
        );
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
        redraw(cx);
        let mut position = start;
        for _ in 0..12 {
            position = gpui::point(position.x + px(4.25), position.y + px(step));
            cx.simulate_mouse_move(position, gpui::MouseButton::Left, Modifiers::none());
            redraw(cx);
        }
        cx.simulate_mouse_up(position, gpui::MouseButton::Left, Modifiers::none());
        redraw(cx);
    }

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.prev_mounted_run.is_some(),
            "拖动后编辑器仍然在渲染"
        );
    });
}

#[gpui::test]
async fn mouse_over_a_block_whose_text_shrank_since_layout_does_not_panic(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 指针定位用上一帧的 last_layout 行序去索引当前文本的行范围。文本在布局之后
    // 变短（后台续建、外部修改、编辑）时 ranges[line_idx] 会越界 panic，而
    // panic = abort，就是用户看到的 coredump。
    let markdown = "# 标题\n\n第一段中文\n\n第二段中文\n\n第三段中文\n";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        let editor = Editor::from_markdown(cx, markdown.to_string(), None);
        editor
    });
    // 源码模式：整篇就是一个多行块，而且它是聚焦的（未聚焦的代码块不挂文本元素，
    // last_layout 为空，跑不到越界分支）。
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);
    redraw(cx);

    let (block, bounds) = editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        let state = block.read(cx);
        let lines = state
            .last_layout
            .as_ref()
            .map(|lines| lines.len())
            .unwrap_or(0);
        assert!(
            lines >= 4,
            "前置条件：源码模式下的多行布局，实际 last_layout = {lines} 行"
        );
        (
            block,
            state.last_bounds.expect("the source block has layout bounds"),
        )
    });

    // 布局之后把多行缩成一行，且不再重绘（真实运行时事件总是先于下一帧到达）。
    block.update(cx, |block, cx| {
        let len = block.visible_len();
        block.replace_text_in_visible_range(0..len, "只剩一行", None, false, cx);
    });

    // 指针仍落在旧布局的第二行高度上。
    let stale_line = gpui::point(bounds.left() + px(20.0), bounds.top() + px(30.0));
    let (offset, len) = block.read_with(cx, |block, _cx| {
        (block.index_for_mouse_position(stale_line), block.visible_len())
    });
    assert!(
        offset <= len,
        "指针偏移必须落在当前文本内：offset={offset}, len={len}"
    );
}

#[gpui::test]
async fn dragging_the_left_button_across_cjk_blocks_keeps_selecting(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 用户报修：左键按住拖动选择文本时又卡又容易崩。拖动路径上每次 mouse_move
    // 都会重新定位端点并同步全部可见块的选中样式；中文文档还要跨 UTF-8 字符
    // 边界取偏移。这个用例真的模拟按住拖动。
    let markdown = (0..400)
        .map(|index| {
            format!(
                "## 第 {index} 节标题\n\n这是第 {index} 段中文正文，足够长以便换行，含标点与英文 mixed text。\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    redraw(cx);

    let first_bounds = editor.read_with(cx, |editor, cx| {
        editor.document.visible_blocks()[0]
            .entity
            .read(cx)
            .last_bounds
            .expect("the first block has layout bounds")
    });

    let start = gpui::point(first_bounds.left() + px(37.5), first_bounds.center().y);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    // 按住往下拖：每步都走一次 on_editor_mouse_move / 块内 select_to。
    let mut position = start;
    for _ in 0..60 {
        position = gpui::point(position.x + px(6.25), position.y + px(11.0));
        cx.simulate_mouse_move(position, gpui::MouseButton::Left, Modifiers::none());
        redraw(cx);
    }
    cx.simulate_mouse_up(position, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.cross_block_selection.is_some(),
            "按住拖动后应当有跨块选择"
        );
    });
}

#[test]
fn rendered_window_cold_start_covers_the_viewport() {
    // 冷启动时 stride 仍是估计值，run 起点被放在视口上方 overdraw 处。旧实现
    // 把「起点 + COLD_RUN_MAX_ROWS 行」当成挂载上限，12 行的估计高度连视口
    // 顶部都到不了，整屏落在 spacer 上（用户报「长文档滚动到下方全是空白，
    // 等几秒才渲染」）。现在上限只削预挂载：视口顶部必须始终有挂载行，
    // 底部要么被铺满、要么明确要求续帧。
    let estimate = 28.0;
    let viewport = 800.0;
    let overdraw = 800.0;
    let strides = uniform_strides(500, estimate); // 总高 14000，全部为估计值
    let total: f32 = strides.iter().sum();

    for scroll_y in [0.0, 700.0, 2_800.0, 7_000.0, 12_000.0] {
        let window = Editor::rendered_window(&strides, scroll_y, viewport, overdraw, None, estimate);
        let mounted_top: f32 = strides[..window.run_start].iter().sum();
        let mounted_bottom: f32 = strides[..window.run_end].iter().sum();
        let viewport_bottom = (scroll_y + viewport).min(total);
        assert!(
            mounted_bottom > scroll_y,
            "scroll {scroll_y}: 视口顶部落在 spacer 上（挂载 {mounted_top}..{mounted_bottom}px）"
        );
        assert!(
            mounted_bottom >= viewport_bottom || window.needs_fill,
            "scroll {scroll_y}: 视口底部 {viewport_bottom}px 是 spacer，且没有要求续帧"
        );
    }
}

#[test]
fn rendered_window_cold_start_still_bounds_the_mount() {
    // 修正不能把冷启动保护整个取消掉：挂载量要随视口高度增长，
    // 不能随文档长度增长（5 千行文档也只挂视口那一段）。
    let estimate = 28.0;
    let strides = uniform_strides(5_000, estimate);
    let window = Editor::rendered_window(&strides, 70_000.0, 800.0, 800.0, None, estimate);
    let mounted = window.run_end - window.run_start;
    let overdraw_rows = 12; // COLD_RUN_MAX_ROWS
    assert!(
        mounted <= 3 * overdraw_rows,
        "挂载 {mounted} 行，超出视口加两侧预挂载的预算"
    );
}

#[gpui::test]
async fn scrolling_a_long_document_keeps_the_viewport_filled(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 长文档滚到中段与底部：挂载行必须一直铺到视口底部。旧实现下 12 行的
    // 估计高度盖不住视口，读者看到的是整屏 spacer，要等好几秒才渲染出来。
    let markdown = (0..400)
        .map(|index| format!("## Section {index}\n\nParagraph body for section {index}.\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);
    redraw(cx);

    for fraction in [0.5_f32, 1.0] {
        editor.update(cx, |editor, _cx| {
            let max = editor.scroll_handle.max_offset().height;
            editor
                .scroll_handle
                .set_offset(gpui::point(gpui::px(0.0), -max * fraction));
        });
        // 冷启动续帧走 16ms 定时器；推进时钟再画两帧，等同真实滚动后的补帧。
        cx.executor().advance_clock(Duration::from_millis(16));
        redraw(cx);
        redraw(cx);

        editor.read_with(cx, |editor, _cx| {
            let run = editor.prev_mounted_run.expect("a run was mounted");
            let viewport = editor
                .last_scroll_viewport_size
                .expect("the viewport size was measured");
            let scroll_y = f32::from(-editor.scroll_handle.offset().y);
            let viewport_bottom = scroll_y + f32::from(viewport.height);
            let last_child = run.child_base + run.row_end - run.row_start - 1;
            let bottom = f32::from(
                editor
                    .scroll_handle
                    .bounds_for_item(last_child)
                    .expect("the mounted run has layout bounds")
                    .bottom(),
            );
            assert!(
                bottom + 0.5 >= viewport_bottom,
                "滚动到 {:.0}% 时挂载内容止于 {bottom}px，视口底部 {viewport_bottom}px 是 spacer",
                fraction * 100.0
            );
        });
    }
}

#[test]
fn rendered_window_scrolled_past_estimates_mounts_trailing_run() {
    // Rows the window has never mounted are lower bounds, so the scroll offset
    // can sit past their running sum. The tail must still fill the viewport.
    let strides = uniform_strides(100, 20.0); // total 2000
    let window = Editor::rendered_window(&strides, 9000.0, 400.0, 200.0, None, 16.0);

    assert_eq!(window.run_end, 100);
    assert_eq!(window.bottom_h, 0.0);
    let mounted: f32 = strides[window.run_start..window.run_end].iter().sum();
    assert!(
        mounted >= 600.0,
        "a viewport plus overdraw must stay mounted, got {mounted}px"
    );
}

#[gpui::test]
async fn footprints_are_dropped_when_the_scroll_column_changes_shape(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // Footprints are read back by child index, so anything added to the scroll
    // column would pair them with the wrong rows. The count has to be re-checked
    // rather than assumed, or the mismatch is silent.
    let markdown = (0..60)
        .map(|index| format!("## Section {index}\n\nParagraph {index}.\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    for _ in 0..3 {
        redraw(cx);
    }

    editor.read_with(cx, |editor, _cx| {
        let run = editor.prev_mounted_run.expect("a run was mounted");
        assert!(
            editor.mounted_run_is_addressable(run),
            "the run the column just emitted must be readable"
        );
        for drift in [run.child_count + 1, run.child_count - 1] {
            assert!(
                !editor.mounted_run_is_addressable(MountedRun {
                    child_count: drift,
                    ..run
                }),
                "a column emitting {drift} children instead of {} must not be trusted",
                run.child_count
            );
        }
    });
}

#[gpui::test]
async fn reading_to_the_bottom_leaves_the_caret_row_behind(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // Scrolling never moves the caret, so it stays where the document loaded.
    // The rows between it and the viewport must not ride along.
    let markdown = (0..200)
        .map(|index| format!("## Section {index}\n\nParagraph body for section {index}.\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    for _ in 0..3 {
        redraw(cx);
    }

    for _ in 0..10 {
        editor.update(cx, |editor, _cx| {
            let max = editor.scroll_handle.max_offset().height;
            editor
                .scroll_handle
                .set_offset(gpui::point(gpui::px(0.0), -max));
        });
        redraw(cx);
        redraw(cx);
    }

    editor.read_with(cx, |editor, _cx| {
        let run = editor.prev_mounted_run.expect("a run was mounted");
        let (run_start, run_end) = (run.row_start, run.row_end);
        let rows = editor.document.visible_blocks().len();
        assert!(
            run_start > 0,
            "the caret's row dragged the whole prefix on screen"
        );
        assert!(
            run_end - run_start < rows / 4,
            "{} of {rows} rows mounted at the bottom of the document",
            run_end - run_start
        );
    });
}

#[gpui::test]
async fn document_present_on_the_first_frame_is_measured_at_the_real_width(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    // Launching with a file renders one frame before the scroll bounds exist,
    // collapsing the content column to its floor so every block wraps a
    // character per line. Caching those footprints would size the document from
    // a layout the reader never sees.
    let markdown = (0..40)
        .map(|index| {
            format!(
                "## Section {index}\n\nA paragraph with enough words in it to wrap many times over \
                 once the content column narrows to a sliver.\n"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));

    for _ in 0..4 {
        redraw(cx);
    }

    editor.read_with(cx, |editor, _cx| {
        let widest = editor
            .row_stride_cache
            .values()
            .fold(0.0f32, |widest, stride| widest.max(*stride));
        assert!(
            widest > 0.0 && widest < 200.0,
            "headings and one-line paragraphs cannot be {widest}px tall; \
             the first frame's collapsed column was cached"
        );
        let run = editor.prev_mounted_run.expect("a run was mounted");
        assert_eq!(
            run.row_start, 0,
            "the top of the document must stay mounted"
        );
        assert!(
            run.row_end > 8,
            "only {} rows mounted, so the viewport is mostly spacer",
            run.row_end
        );
    });
}

#[test]
fn about_dialog_body_lines_use_velora_brand_and_repository_link() {
    let strings = I18nStrings::zh_cn();
    let lines = Editor::about_dialog_body_lines(&strings);

    assert_eq!(lines[0], format!("Velora {}", env!("CARGO_PKG_VERSION")));
    assert_eq!(
        lines[2],
        format!("项目仓库: {}", super::render::ABOUT_GITHUB_URL)
    );
    assert_eq!(lines[3], "第三方来源与许可信息见项目文档。");
}

#[gpui::test]
async fn about_github_link_uses_gpui_url_opening(cx: &mut TestAppContext) {
    cx.update(|cx| {
        super::render::open_about_github_url(cx);
    });

    assert_eq!(
        cx.opened_url(),
        Some(super::render::ABOUT_GITHUB_URL.to_string())
    );
}

#[gpui::test]
async fn ctrl_s_saves_rendered_mode_edit_to_existing_file(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("ctrl-s-rendered-save");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });

    cx.simulate_input("!");
    redraw(cx);
    let expected = editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(!editor.pending_save);
        editor.document.markdown_text(cx)
    });
    assert_ne!(expected, "alpha");

    cx.simulate_keystrokes("ctrl-s");
    redraw(cx);

    assert_eq!(
        fs::read_to_string(&path).expect("read saved markdown"),
        expected
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.document_dirty);
        assert!(!editor.pending_save);
    });
}

#[gpui::test]
async fn dirty_saved_document_is_autosaved(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("autosave");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("autosaved text".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&path).expect("read autosaved markdown"),
        "autosaved text"
    );
    editor.read_with(cx, |editor, _cx| assert!(!editor.document_dirty));
}

#[gpui::test]
async fn chinese_ime_composition_commit_save_and_undo_keep_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("chinese-ime-commit");
    fs::write(&path, "note").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "note".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    let block = editor.read_with(cx, |editor, _cx| {
        editor.document.first_root().expect("paragraph").clone()
    });
    block.update(cx, |block, _cx| block.selected_range = 4..4);

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_and_mark_text_in_range(
                block,
                None,
                "nihao",
                Some(5..5),
                window,
                block_cx,
            );
        });
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.marked_range.clone()),
        Some(4..9)
    );
    redraw(cx);
    editor.update(cx, |editor, _cx| {
        let entry = editor
            .undo_history
            .last_mut()
            .expect("composition should capture its original source");
        assert_eq!(entry.kind, UndoCaptureKind::ImeComposition);
        entry.timestamp = Instant::now() - Duration::from_secs(2);
    });
    let block_bounds = block.read_with(cx, |block, _cx| {
        block
            .last_bounds
            .expect("composing text should be laid out")
    });
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            assert_eq!(
                <Block as EntityInputHandler>::marked_text_range(block, window, block_cx),
                Some(4..9)
            );
            assert!(
                <Block as EntityInputHandler>::bounds_for_range(
                    block,
                    4..9,
                    block_bounds,
                    window,
                    block_cx,
                )
                .is_some()
            );
        });
    });
    editor.update(cx, |editor, cx| editor.request_save_document(cx));
    redraw(cx);
    assert_eq!(
        fs::read_to_string(&path).expect("composition should not be saved"),
        "note"
    );
    editor.read_with(cx, |editor, _cx| assert!(editor.pending_save));

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "你好", window, block_cx,
            );
        });
    });
    assert_eq!(
        block.read_with(cx, |block, _cx| block.display_text().to_string()),
        "note你好"
    );
    assert_eq!(
        block.read_with(cx, |block, _cx| block.marked_range.clone()),
        None
    );
    redraw(cx);
    editor.read_with(cx, |editor, _cx| assert!(!editor.pending_save));
    assert_eq!(
        fs::read_to_string(&path).expect("read saved Markdown"),
        "note你好"
    );
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.undo_history.len(), 1);
    });

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, cx| editor.document.markdown_text(cx)),
        "note"
    );
}

#[gpui::test]
async fn autosave_waits_until_ime_composition_is_committed(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("ime-autosave");
    fs::write(&path, "note").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "note".to_string(), Some(path))
    });
    let block = editor.read_with(cx, |editor, _cx| {
        editor.document.first_root().expect("paragraph").clone()
    });
    block.update(cx, |block, _cx| block.selected_range = 4..4);
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_and_mark_text_in_range(
                block,
                None,
                "nihao",
                Some(5..5),
                window,
                block_cx,
            );
        });
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).expect("read Markdown during composition"),
        "note"
    );

    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "你好", window, block_cx,
            );
        });
    });
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).expect("read committed Markdown"),
        "note你好"
    );
}

#[gpui::test]
async fn external_autosave_conflict_survives_workspace_rescan(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("autosave-conflict-rescan");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("our edits".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    fs::write(&path, "external edits").expect("write external changes");

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.has_external_autosave_conflict());
    });

    // 工作区重新扫描会清空普通错误提示，但外部修改冲突必须保留，
    // 否则自动保存会重新放开并覆盖磁盘上的新内容。
    editor.update(cx, |editor, cx| {
        let root = path.parent().expect("temp dir").to_path_buf();
        editor.set_workspace_root(root, cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.has_external_autosave_conflict(),
            "conflict flag must survive a workspace rescan"
        );
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        fs::read_to_string(&path).expect("read external file"),
        "external edits",
        "autosave must not overwrite the externally modified file"
    );
}

#[gpui::test]
async fn autosave_does_not_overwrite_external_file_changes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("autosave-external-change");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(recovery_id);
    });
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("our edits".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    fs::write(&path, "external edits").expect("write external changes");

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&path).expect("read external file"),
        "external edits"
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.document_dirty);
        assert!(editor.has_external_autosave_conflict());
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(!editor.save_to_existing_path(&path, window, cx));
        });
    });
    assert_eq!(
        fs::read_to_string(&path).expect("read external file after manual save"),
        "external edits"
    );
}

#[gpui::test]
async fn autosave_flushes_a_dirty_tab_after_switching_away(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let first_path = temp_markdown_path("autosave-tab-first");
    let second_path = temp_markdown_path("autosave-tab-second");
    fs::write(&first_path, "alpha").expect("write first document");
    fs::write(&second_path, "beta").expect("write second document");
    let cleanup_first = first_path.clone();
    let cleanup_second = second_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_first);
        let _ = fs::remove_file(cleanup_second);
    });

    let (editor, cx) = cx.add_window_view({
        let first_path = first_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(first_path))
    });
    let first_recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited alpha".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path.clone(), window, cx);
        });
    });
    editor.update(cx, |editor, cx| {
        let second = editor.document.first_root().expect("second block").clone();
        second.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited beta".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    let second_recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(first_recovery_id);
        let _ = crate::config::remove_recovery_snapshot(second_recovery_id);
    });

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&first_path).expect("read first autosaved document"),
        "edited alpha"
    );
    assert_eq!(
        fs::read_to_string(&second_path).expect("read second autosaved document"),
        "edited beta"
    );
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&second_path));
        assert!(!editor.document_dirty);
        assert!(!editor.has_dirty_workspace_documents());
    });
}

#[gpui::test]
async fn save_and_close_writes_every_dirty_workspace_tab(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let first_path = temp_markdown_path("save-close-tab-first");
    let second_path = temp_markdown_path("save-close-tab-second");
    fs::write(&first_path, "alpha").expect("write first document");
    fs::write(&second_path, "beta").expect("write second document");
    let cleanup_first = first_path.clone();
    let cleanup_second = second_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(cleanup_first);
        let _ = fs::remove_file(cleanup_second);
    });

    let (editor, cx) = cx.add_window_view({
        let first_path = first_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(first_path))
    });
    let first_recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited alpha".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path.clone(), window, cx);
        });
    });
    let second_recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    cx.on_quit(move || {
        let _ = crate::config::remove_recovery_snapshot(first_recovery_id);
        let _ = crate::config::remove_recovery_snapshot(second_recovery_id);
    });
    editor.update(cx, |editor, cx| {
        let second = editor.document.first_root().expect("second block").clone();
        second.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited beta".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.save_dirty_workspace_documents_and_close(window, cx);
        });
    });
    cx.run_until_parked();

    assert_eq!(
        fs::read_to_string(&first_path).expect("read first"),
        "edited alpha"
    );
    assert_eq!(
        fs::read_to_string(&second_path).expect("read second"),
        "edited beta"
    );
    assert_eq!(cx.cx.windows().len(), 0);
}

#[gpui::test]
async fn recovered_document_is_opened_as_a_dirty_copy(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let recovery_id = uuid::Uuid::new_v4();
    let source_path = temp_markdown_path("recovered-source");
    let (editor, cx) = cx.add_window_view({
        let source_path = source_path.clone();
        move |_window, cx| {
            Editor::from_recovery(
                cx,
                crate::config::RecoverySnapshot {
                    id: recovery_id,
                    source_path: Some(source_path),
                    markdown: "recovered text".to_string(),
                },
            )
        }
    });

    editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(editor.is_recovered_document);
        assert_eq!(editor.recovery_id, recovery_id);
        assert_eq!(editor.file_path, None);
        assert_eq!(editor.document.markdown_text(cx), "recovered text");
    });
}

#[gpui::test]
async fn workspace_tabs_restore_unsaved_markdown_state(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let first_path = temp_markdown_path("workspace-tab-first");
    let second_path = temp_markdown_path("workspace-tab-second");
    fs::write(&first_path, "alpha").expect("write first document");
    fs::write(&second_path, "beta").expect("write second document");
    let cleanup_first = first_path.clone();
    let cleanup_second = second_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_first);
        let _ = fs::remove_file(&cleanup_second);
    });

    let (editor, cx) = cx.add_window_view({
        let first_path = first_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(first_path))
    });
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited alpha".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(second_path.clone(), window, cx);
        });
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&second_path));
        assert_eq!(editor.document.markdown_text(cx), "beta");
        assert!(!editor.document_dirty);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(first_path.clone(), window, cx);
        });
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&first_path));
        assert_eq!(editor.document.markdown_text(cx), "edited alpha");
        assert!(editor.document_dirty);
    });
}

#[gpui::test]
async fn window_save_action_saves_current_editor_without_global_menu_route(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let path = temp_markdown_path("window-action-save");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "alpha".to_string(), Some(path))
    });

    cx.simulate_input(" action");
    redraw(cx);
    let expected = editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        editor.document.markdown_text(cx)
    });
    assert_ne!(expected, "alpha");

    cx.dispatch_action(SaveDocument);
    redraw(cx);

    assert_eq!(
        fs::read_to_string(&path).expect("read saved markdown"),
        expected
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.document_dirty);
        assert!(!editor.pending_save);
    });
}

#[gpui::test]
async fn window_title_tracks_file_and_edited_state(cx: &mut TestAppContext) {
    // roadmap A7 的可自动验证面：标题（含「已编辑」前缀标记）由渲染帧同步到窗口，
    // 编辑后立刻带上标记、保存后去掉；锁屏期间的系统显示延迟属平台行为，留人工复核。
    init_editor_test_app(cx);

    let path = temp_markdown_path("window-title-sync");
    fs::write(&path, "alpha").expect("write initial markdown");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let file_name = path
        .file_name()
        .expect("temp path has a file name")
        .to_string_lossy()
        .to_string();
    let clean_title = format!("Velora - {file_name}");
    let dirty_marker = cx.update(|cx| cx.global::<I18nManager>().strings().dirty_title_marker.clone());
    assert!(!dirty_marker.is_empty(), "脏标记文案不应为空");

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        let markdown = "alpha".to_string();
        move |_window, cx| Editor::from_markdown(cx, markdown, Some(path))
    });
    redraw(cx);
    assert_eq!(cx.update(|window, _cx| window.window_title()), clean_title);

    cx.simulate_input(" x");
    redraw(cx);
    editor.read_with(cx, |editor, _cx| assert!(editor.document_dirty));
    assert_eq!(
        cx.update(|window, _cx| window.window_title()),
        format!("{dirty_marker} {clean_title}"),
        "编辑后窗口标题应带已编辑标记"
    );

    cx.dispatch_action(SaveDocument);
    redraw(cx);
    editor.read_with(cx, |editor, _cx| assert!(!editor.document_dirty));
    assert_eq!(
        cx.update(|window, _cx| window.window_title()),
        clean_title,
        "保存后窗口标题应去掉已编辑标记"
    );
}

/// 窗口 frame 用例需要独占配置目录：关闭/退出窗口的用例都会往 config.toml
/// 写 frame，共用进程级目录时并行执行会互相覆盖（实测会让断言读到别的用例的 frame）。
fn isolated_window_frame_config(test_name: &str) -> (PathBuf, crate::config::TestConfigRootGuard) {
    let root = std::env::temp_dir().join(format!(
        "velora-{test_name}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let guard = crate::config::override_test_config_root(&root);
    (root, guard)
}

#[gpui::test]
async fn quitting_the_app_remembers_each_window_frame(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-frame-quit");
    // roadmap A2：⌘Q 也必须记住窗口位置与大小。此前只有关闭单窗口才落盘，
    // 「调完位置直接退出」会把调整丢掉（用户报修）。先放一个哨兵 frame，
    // 用来区分「退出路径没写盘」与「写盘写对了」。
    init_editor_test_app(cx);
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 11,
        y: 13,
        width: 1111,
        height: 777,
    })
    .expect("seed sentinel frame");

    let (_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, _cx| window.resize(gpui::size(px(1200.0), px(820.0))));
    redraw(cx);

    // 走用户真实路径：⌘Q 在窗口内派发，窗口正处于借用状态。
    cx.dispatch_action(QuitApplication);
    cx.run_until_parked();

    let stored = crate::config::saved_window_frame()
        .expect("read window frame")
        .expect("quitting should store the window frame");
    assert_ne!(
        (stored.width, stored.height),
        (1111, 777),
        "退出路径没有落盘：读到的还是哨兵 frame"
    );
    assert_eq!(
        (stored.width, stored.height),
        (1200, 820),
        "退出时应记住退出前的窗口尺寸"
    );
    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn platform_close_remembers_the_window_frame(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-frame-platform-close");
    // 平台自己发起的关闭（macOS 红灯）不经过应用内任何关闭入口，
    // 只有 on_window_should_close 能在窗口还活着时落盘（用户报修场景）。
    init_editor_test_app(cx);
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 3,
        y: 5,
        width: 999,
        height: 666,
    })
    .expect("seed sentinel frame");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, _cx| window.resize(gpui::size(px(1280.0), px(860.0))));
    redraw(cx);

    let allowed = cx.update(|window, cx| {
        editor
            .clone()
            .update(cx, |editor, cx| editor.on_window_should_close(window, cx))
    });
    assert!(allowed, "干净文档应允许平台关闭窗口");

    let stored = crate::config::saved_window_frame()
        .expect("read window frame")
        .expect("平台关闭路径应落盘窗口 frame");
    assert_eq!(
        (stored.width, stored.height),
        (1280, 860),
        "平台关闭前应记住当时的窗口尺寸"
    );
    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn resizing_the_window_records_the_frame_without_quitting(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-frame-resize");
    // 窗口位置/大小不能只在关窗/退出时落盘：强杀进程、平台关闭回调缺位、或调完
    // 窗口程序就崩，最后一次调整就丢了。窗口一动（bounds 变化）就该记住。
    init_editor_test_app(cx);
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 10,
        y: 20,
        width: 900,
        height: 600,
    })
    .expect("seed frame");

    // 走真实开窗路径（open_editor_window 里装监听），不关窗、不退出。
    let editor = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();
    cx.simulate_window_resize(editor.into(), gpui::size(px(1320.0), px(880.0)));
    // 防抖窗口过后才落盘。
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(700));
    cx.run_until_parked();

    let stored = crate::config::saved_window_frame()
        .expect("read window frame")
        .expect("窗口刚被缩放，frame 就应该已经落盘");
    assert_eq!(
        (stored.width, stored.height),
        (1320, 880),
        "缩放窗口后应立刻记住新尺寸，不依赖退出路径"
    );
    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn window_open_position_setting_controls_how_windows_open(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-open-position");
    // 锁定设置语义：「记住上次位置」恢复 frame 的位置+大小；「居中打开」只把
    // 位置居中，大小仍用记住的 frame；「默认窗口尺寸」只在没有记住 frame 时生效。
    init_editor_test_app(cx);
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 40,
        y: 60,
        width: 1000,
        height: 700,
    })
    .expect("seed frame");
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));

    cx.update(|cx| {
        crate::config::EditorSettings::set_window_open_position(
            cx,
            crate::config::WindowOpenPosition::Remember,
        );
    });
    let remembered = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();
    assert_eq!(
        windowed_rect(&remembered, cx),
        (40, 60, 1000, 700),
        "打开位置=记住上次位置 时应恢复记住的 frame"
    );

    cx.update(|cx| {
        crate::config::EditorSettings::set_window_open_position(
            cx,
            crate::config::WindowOpenPosition::Center,
        );
    });
    let centered = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();
    // 测试平台主屏 1920×1080，记住的 frame 1000×700 → 居中原点 (460, 190)。
    assert_eq!(
        windowed_rect(&centered, cx),
        (460, 190, 1000, 700),
        "打开位置=居中打开 时只居中位置，大小用记住的 frame"
    );
    let _ = fs::remove_dir_all(root);

    // 没有记住 frame 时，「居中打开」才用「默认窗口尺寸」（1080×720 → (420, 180)）。
    let (empty_root, _empty_root_guard) = isolated_window_frame_config("window-open-position-empty");
    let fallback = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();
    assert_eq!(
        windowed_rect(&fallback, cx),
        (420, 180, 1080, 720),
        "没有记住的 frame 时按默认窗口尺寸居中"
    );
    let _ = fs::remove_dir_all(empty_root);
}

#[gpui::test]
async fn window_frame_from_a_missing_display_keeps_its_size(cx: &mut TestAppContext) {
    let (root, _root_guard) = isolated_window_frame_config("window-frame-missing-display");
    // 副屏拔掉/分辨率变小后，记住的 frame 中心点落在任何显示器之外。gpui 的
    // Windows 后端遇到这种 frame 会把整块 bounds（连大小）换成显示器默认值——
    // 表现就是「无论上次多大，打开永远默认大小」。应用层必须把它挪回一块屏上，
    // 且尺寸照旧。
    init_editor_test_app(cx);
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    crate::config::store_window_frame(crate::config::WindowFrame {
        x: 2600,
        y: 300,
        width: 1400,
        height: 900,
    })
    .expect("seed frame");

    let handle = cx.update(|cx| crate::app_menu::open_editor_window(cx, String::new(), None));
    cx.run_until_parked();

    // 测试主屏 1920×1080：窗口能整块放下 → 搬进屏内 (520, 180)，尺寸不变。
    assert_eq!(
        windowed_rect(&handle, cx),
        (520, 180, 1400, 900),
        "frame 不在任何显示器上时应挪回主屏，并保留记住的尺寸"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn every_editor_window_removal_remembers_the_frame() {
    // 用户报修：调整窗口位置/大小后关闭、下一次启动又回到旧位置。
    // 窗口移除统一走 Editor::close_editor_window（先落盘再移除），
    // 这条守卫挡住「新写一条关闭路径时忘了记 frame」。
    assert_eq!(
        include_str!("close.rs")
            .matches("window.remove_window()")
            .count(),
        1,
        "窗口移除应只在 Editor::close_editor_window 里发生"
    );
    for (name, source) in [
        ("persistence.rs", include_str!("persistence.rs")),
        ("workspace.rs", include_str!("workspace.rs")),
        ("window_state.rs", include_str!("window_state.rs")),
        ("events.rs", include_str!("events.rs")),
        ("file_drop.rs", include_str!("file_drop.rs")),
        ("render.rs", include_str!("render.rs")),
    ] {
        assert_eq!(
            source.matches("remove_window()").count(),
            0,
            "{name} 里移除窗口应改用 Editor::close_editor_window"
        );
    }
}

#[test]
fn every_svg_icon_sets_its_own_text_color() {
    // 状态栏的源码切换按钮换成 svg 图标后整块看不见：因为
    // gpui 的 svg 元素只读自身 style.text.color，父容器 text_color 不继承
    // （docs/architecture/overview.md §GPUI 限制）：漏设 ⇒ 一个像素都不画。
    for (name, source) in [
        ("status_bar.rs", include_str!("status_bar.rs")),
        ("workspace.rs", include_str!("workspace.rs")),
        ("window_chrome.rs", include_str!("../window_chrome.rs")),
        (
            "components/block/render.rs",
            include_str!("../components/block/render.rs"),
        ),
    ] {
        for (index, chain) in source.split("svg()").skip(1).enumerate() {
            let chain = &chain[..chain.find(';').unwrap_or(chain.len())];
            assert!(
                chain.contains(".text_color("),
                "{name} 第 {} 处 svg() 没有自己的 .text_color：\
                 gpui 不从父容器继承文字色，图标会整块不渲染",
                index + 1
            );
        }
    }
}

#[gpui::test]
async fn export_html_writes_rendered_document_without_changing_editor_state(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("rendered-export-html", "html");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# Title\n\nbody".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        editor.mark_dirty(cx);
        assert!(editor.document_dirty);
        assert!(editor.file_path.is_none());
        editor
            .export_document_to_path(ExportFormat::Html, &export_path, cx)
            .expect("html export should write");
        assert!(editor.document_dirty);
        assert!(editor.file_path.is_none());
    });

    let html = fs::read_to_string(&export_path).expect("read exported html");
    assert!(html.contains("<h1>Title</h1>"));
    assert!(html.contains("<p>body</p>"));
}

#[gpui::test]
async fn export_png_writes_long_image_without_changing_editor_state(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("rendered-export-png", "png");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# Title\n\nbody".to_string(), None)
    });

    let export_result = editor.update(cx, |editor, cx| {
        editor.mark_dirty(cx);
        let result = editor.export_document_to_path(ExportFormat::Png, &export_path, cx);
        assert!(editor.document_dirty);
        assert!(editor.file_path.is_none());
        result
    });

    match export_result {
        Ok(()) => {
            let png = fs::read(&export_path).expect("read exported png");
            assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        }
        // Chrome 缺失时只要求给出可行动的错误，磁盘上不产生半成品。
        Err(err) => {
            let message = err.to_string();
            assert!(
                message.contains("Chromium") || message.contains("Chrome"),
                "unexpected PNG export error: {message}"
            );
            assert!(!export_path.exists());
        }
    }
}

#[gpui::test]
async fn export_html_uses_source_mode_raw_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("source-export-html", "html");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "rendered".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        let source_block = editor
            .document
            .first_root()
            .expect("source mode should keep one root block")
            .clone();
        source_block.update(cx, |block, _cx| {
            block.record.set_title(InlineTextTree::plain(
                "# Source\n\n<!--\n<strong>visible</strong>\n-->".to_string(),
            ));
            block.sync_render_cache();
        });
        editor
            .export_document_to_path(ExportFormat::Html, &export_path, cx)
            .expect("source html export should write");
    });

    let html = fs::read_to_string(&export_path).expect("read exported html");
    assert!(html.contains("<h1>Source</h1>"));
    assert!(html.contains("class=\"vlt-comment\""));
    assert!(html.contains("&lt;strong&gt;visible&lt;/strong&gt;"));
}

#[gpui::test]
async fn dropped_markdown_replaces_clean_editor_in_current_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let dropped_path = temp_markdown_path("drop-clean-replace");
    fs::write(
        &dropped_path,
        "# Dropped\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n",
    )
    .expect("write dropped markdown");
    let cleanup_path = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "old".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(editor.view_mode == ViewMode::Source);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path.clone(), window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&dropped_path));
        assert!(editor.view_mode == ViewMode::Rendered);
        assert!(!editor.document_dirty);
        assert!(!editor.show_drop_replace_dialog);
        assert_eq!(editor.document.root_count(), 2);
        assert_eq!(
            editor
                .document
                .root_blocks()
                .last()
                .expect("table block")
                .read(cx)
                .kind(),
            BlockKind::Table
        );
        assert!(editor.document.markdown_text(cx).contains("# Dropped"));
    });

    let fallback_source = "!!! note\n  keep this text";
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown(fallback_source.into(), None, cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        assert!(editor.source_mode_fallback_required);
        assert_eq!(editor.document.raw_source_text(cx), fallback_source);
    });
    redraw(cx);
    assert_eq!(cx.cx.windows().len(), 1);
}

#[gpui::test]
async fn dropped_paths_pick_first_valid_markdown_file(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let text_path = temp_export_path("drop-ignore-non-markdown", "txt");
    let markdown_path = temp_export_path("drop-pick-markdown", "markdown");
    fs::write(&text_path, "plain").expect("write text");
    fs::write(&markdown_path, "markdown").expect("write markdown");
    let cleanup_text = text_path.clone();
    let cleanup_markdown = markdown_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_text);
        let _ = fs::remove_file(&cleanup_markdown);
    });

    assert_eq!(
        Editor::first_dropped_markdown_path(&[text_path, markdown_path.clone()]),
        Some(markdown_path)
    );
}

#[gpui::test]
async fn dropped_paths_pick_first_valid_image_file(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let text_path = temp_export_path("drop-ignore-non-image", "txt");
    let image_path = temp_export_path("drop-pick-image", "png");
    fs::write(&text_path, "plain").expect("write text");
    fs::write(&image_path, b"image bytes").expect("write image");
    let cleanup_text = text_path.clone();
    let cleanup_image = image_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_text);
        let _ = fs::remove_file(&cleanup_image);
    });

    assert_eq!(
        Editor::first_dropped_image_path(&[text_path, image_path.clone()]),
        Some(image_path)
    );
}

#[gpui::test]
async fn dirty_drop_waits_for_replace_decision_and_cancel_preserves_document(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let dropped_path = temp_markdown_path("drop-dirty-cancel");
    fs::write(&dropped_path, "dropped").expect("write dropped markdown");
    let cleanup_path = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "current".to_string(), None));
    editor.update(cx, |editor, cx| editor.mark_dirty(cx));

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path, window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(editor.show_drop_replace_dialog);
        assert_eq!(editor.document.markdown_text(cx), "current");
        assert!(editor.pending_drop_replace_path.is_some());
    });

    editor.update(cx, |editor, cx| editor.cancel_drop_replace_dialog(cx));

    editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(!editor.show_drop_replace_dialog);
        assert!(editor.pending_drop_replace_path.is_none());
        assert_eq!(editor.document.markdown_text(cx), "current");
    });
}

#[gpui::test]
async fn dirty_drop_can_replace_without_saving(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let dropped_path = temp_markdown_path("drop-dirty-discard");
    fs::write(&dropped_path, "dropped").expect("write dropped markdown");
    let cleanup_path = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "current".to_string(), None));
    editor.update(cx, |editor, cx| editor.mark_dirty(cx));

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path.clone(), window, cx);
            editor.discard_pending_drop_replace(window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&dropped_path));
        assert_eq!(editor.document.markdown_text(cx), "dropped");
        assert!(!editor.document_dirty);
        assert!(!editor.show_drop_replace_dialog);
    });
}

#[gpui::test]
async fn dirty_drop_saves_existing_document_before_replace(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let current_path = temp_markdown_path("drop-save-current");
    let dropped_path = temp_markdown_path("drop-save-replace");
    fs::write(&current_path, "original").expect("write current markdown");
    fs::write(&dropped_path, "dropped").expect("write dropped markdown");
    let cleanup_current = current_path.clone();
    let cleanup_dropped = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_current);
        let _ = fs::remove_file(&cleanup_dropped);
    });

    let (editor, cx) = cx.add_window_view({
        let current_path = current_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "original".to_string(), Some(current_path))
    });

    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("current root").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path.clone(), window, cx);
            editor.save_and_replace_pending_drop(window, cx);
        });
    });
    redraw(cx);

    assert_eq!(
        fs::read_to_string(&current_path).expect("read saved current"),
        "edited"
    );
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&dropped_path));
        assert_eq!(editor.document.markdown_text(cx), "dropped");
        assert!(!editor.document_dirty);
        assert!(!editor.pending_drop_replace_after_save);
    });
}

#[gpui::test]
async fn close_window_menu_action_closes_only_active_editor_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (_first_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "first".to_string(), None));
    let first_window = activate_visual_window(cx);

    let (_second_editor, cx) = cx
        .cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, "second".to_string(), None));
    let second_window = activate_visual_window(cx);

    assert_ne!(first_window.window_id(), second_window.window_id());
    assert_eq!(cx.cx.windows().len(), 2);

    cx.cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&CloseWindow, cx);
    });
    cx.run_until_parked();

    let remaining = cx.cx.windows();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].window_id(), first_window.window_id());
    assert_ne!(remaining[0].window_id(), second_window.window_id());
}

#[gpui::test]
async fn app_menu_opened_windows_activate_and_close_independently(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let first_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "first".to_string(), None));
    cx.run_until_parked();
    let second_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "second".to_string(), None));
    cx.run_until_parked();

    let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
    assert_eq!(active_window.window_id(), second_window.window_id());
    assert_ne!(first_window.window_id(), second_window.window_id());
    assert_eq!(cx.update(|cx| cx.windows().len()), 2);

    assert!(
        second_window
            .update(cx, |editor, _window, _cx| editor.close_guard_installed)
            .expect("second editor window should be open")
    );

    cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&CloseWindow, cx);
    });
    cx.run_until_parked();

    let remaining = cx.update(|cx| cx.windows());
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].window_id(), first_window.window_id());

    cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&CloseWindow, cx);
    });
    cx.run_until_parked();

    assert!(cx.update(|cx| cx.windows().is_empty()));
}

#[gpui::test]
async fn app_menu_opened_file_window_reinstalls_close_guard_after_registration(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let opened_path = temp_markdown_path("app-menu-opened-file-window-close");
    fs::write(&opened_path, "opened from file").expect("write opened markdown");

    let first_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "first".to_string(), None));
    cx.run_until_parked();
    let second_window = cx.update(|cx| {
        crate::app_menu::open_editor_window(
            cx,
            fs::read_to_string(&opened_path).expect("read opened markdown"),
            Some(opened_path.clone()),
        )
    });
    cx.run_until_parked();

    let active_window = cx.update(|cx| cx.active_window().expect("window should be active"));
    assert_eq!(active_window.window_id(), second_window.window_id());
    assert_ne!(first_window.window_id(), second_window.window_id());

    second_window
        .update(cx, |editor, window, cx| {
            assert!(editor.close_guard_installed);
            assert!(editor.on_window_should_close(window, cx));
        })
        .expect("second editor window should be open");

    cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&CloseWindow, cx);
    });
    cx.run_until_parked();

    let remaining = cx.update(|cx| cx.windows());
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].window_id(), first_window.window_id());
    assert_ne!(remaining[0].window_id(), second_window.window_id());

    let _ = fs::remove_file(opened_path);
}

#[gpui::test]
async fn app_menu_opened_dirty_file_window_prompts_only_that_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let opened_path = temp_markdown_path("app-menu-opened-dirty-file-window-close");
    fs::write(&opened_path, "opened from file").expect("write opened markdown");

    let first_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "first".to_string(), None));
    let second_window = cx.update(|cx| {
        crate::app_menu::open_editor_window(
            cx,
            fs::read_to_string(&opened_path).expect("read opened markdown"),
            Some(opened_path.clone()),
        )
    });
    cx.run_until_parked();

    second_window
        .update(cx, |editor, window, cx| {
            editor.mark_dirty(cx);
            assert!(!editor.on_window_should_close(window, cx));
        })
        .expect("second editor window should be open");

    first_window
        .update(cx, |editor, _window, _cx| {
            assert!(!editor.show_unsaved_changes_dialog);
        })
        .expect("first editor window should be open");
    second_window
        .update(cx, |editor, _window, _cx| {
            assert!(editor.show_unsaved_changes_dialog);
        })
        .expect("second editor window should be open");

    let _ = fs::remove_file(opened_path);
}

#[gpui::test]
async fn app_menu_opened_dirty_window_close_guard_prompts_only_that_window(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let first_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "first".to_string(), None));
    let second_window =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, "second".to_string(), None));
    cx.run_until_parked();

    second_window
        .update(cx, |editor, window, cx| {
            editor.mark_dirty(cx);
            assert!(!editor.on_window_should_close(window, cx));
        })
        .expect("second editor window should be open");

    first_window
        .update(cx, |editor, _window, _cx| {
            assert!(!editor.show_unsaved_changes_dialog);
        })
        .expect("first editor window should be open");
    second_window
        .update(cx, |editor, _window, _cx| {
            assert!(editor.show_unsaved_changes_dialog);
        })
        .expect("second editor window should be open");
}

#[gpui::test]
async fn quit_application_allows_clean_editor_windows_to_quit(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (first_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "first".to_string(), None));
    let _first_window = activate_visual_window(cx);

    let (second_editor, cx) = cx
        .cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, "second".to_string(), None));
    let _second_window = activate_visual_window(cx);

    assert_eq!(cx.cx.windows().len(), 2);

    cx.cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&QuitApplication, cx);
    });
    cx.run_until_parked();

    first_editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_unsaved_changes_dialog);
    });
    second_editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_unsaved_changes_dialog);
    });
}

#[gpui::test]
async fn quit_application_prompts_dirty_editor_without_quitting(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let (first_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "first".to_string(), None));
    let first_window = activate_visual_window(cx);

    let (second_editor, cx) = cx
        .cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, "second".to_string(), None));
    let second_window = activate_visual_window(cx);

    second_editor.update(cx, |editor, cx| editor.mark_dirty(cx));
    assert_eq!(cx.cx.windows().len(), 2);

    cx.cx.update(|cx| {
        crate::app_menu::dispatch_menu_action(&QuitApplication, cx);
    });
    cx.run_until_parked();

    let open_windows = cx.cx.windows();
    assert_eq!(open_windows.len(), 2);
    assert!(
        open_windows
            .iter()
            .any(|window| window.window_id() == first_window.window_id())
    );
    assert!(
        open_windows
            .iter()
            .any(|window| window.window_id() == second_window.window_id())
    );
    first_editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_unsaved_changes_dialog);
    });
    second_editor.read_with(cx, |editor, _cx| {
        assert!(editor.show_unsaved_changes_dialog);
    });
}

#[gpui::test]
async fn windows_fallback_close_window_dispatch_closes_target_editor_window(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "target".to_string(), None));
    let target_window = activate_visual_window(cx);

    cx.update(|window, cx| {
        let editor = editor.downgrade();
        crate::app_menu::dispatch_menu_action_for_editor(&CloseWindow, &editor, window, cx);
    });
    cx.run_until_parked();

    assert!(
        cx.cx
            .windows()
            .iter()
            .all(|window| window.window_id() != target_window.window_id())
    );
}

#[gpui::test]
async fn window_close_action_closes_current_editor_before_global_menu_route(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let (_first_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "first".to_string(), None));
    let first_window = activate_visual_window(cx);

    let (_second_editor, cx) = cx
        .cx
        .add_window_view(|_window, cx| Editor::from_markdown(cx, "second".to_string(), None));
    let second_window = activate_visual_window(cx);

    cx.dispatch_action(CloseWindow);
    cx.run_until_parked();

    let remaining = cx.cx.windows();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].window_id(), first_window.window_id());
    assert_ne!(remaining[0].window_id(), second_window.window_id());
}

#[gpui::test]
async fn hamburger_menu_toggles_and_closes_with_the_item_panel(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        // 点一下：列表打开，条目面板先不显示。
        editor.toggle_hamburger_menu(cx);
        assert!(editor.hamburger_menu_open);
        assert_eq!(editor.menu_bar_open, None);

        // 划过（或点）某一项：列表留着，它的条目面板打开。
        editor.open_hamburger_menu_item(1, cx);
        assert!(editor.hamburger_menu_open);
        assert_eq!(editor.menu_bar_open, Some(1));

        // 再点一下按钮：列表与条目面板一起关。
        editor.toggle_hamburger_menu(cx);
        assert!(!editor.hamburger_menu_open);
        assert_eq!(editor.menu_bar_open, None);
    });
}

#[gpui::test]
async fn dismissing_from_body_closes_the_hamburger_list(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.toggle_hamburger_menu(cx);
        assert!(editor.hamburger_menu_open);

        // 点正文（或 Esc）时，列表也要一起关——只开列表、没开条目面板也算打开。
        editor.dismiss_menu_bar_from_body(cx);
        assert!(!editor.hamburger_menu_open);
    });
}

#[gpui::test]
async fn dismissing_menu_bar_from_body_clears_open_state(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.open_menu_bar(0, cx);
        editor.set_menu_bar_hovered(true, cx);
        editor.set_menu_panel_hovered(true, cx);
        assert_eq!(editor.menu_bar_open, Some(0));

        editor.dismiss_menu_bar_from_body(cx);
        assert_eq!(editor.menu_bar_open, None);
        assert!(!editor.menu_bar_hovered);
        assert!(!editor.menu_panel_hovered);
        assert!(!editor.menu_submenu_panel_hovered);
        assert!(editor.menu_close_task.is_none());
    });
}

#[gpui::test]
async fn submenu_panel_hover_keeps_in_window_menu_open(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.open_menu_bar(0, cx);
        editor.open_menu_submenu(2, cx);
        editor.set_menu_submenu_panel_hovered(true, cx);
        editor.set_menu_panel_hovered(false, cx);
        editor.set_menu_bar_hovered(false, cx);

        assert_eq!(editor.menu_bar_open, Some(0));
        assert_eq!(editor.menu_submenu_open, Some(2));
        assert!(editor.menu_submenu_panel_hovered);
        assert!(editor.menu_close_task.is_none());

        editor.set_menu_submenu_panel_hovered(false, cx);
        assert!(editor.menu_close_task.is_some());

        editor.close_menu_bar(cx);
    });
}

// The gap bridge and the submenu panel overlap, so moving the cursor from the
// bridge onto the submenu emits `bridge: false` and `panel: true` in the same
// gesture. With both regions sharing one hover flag the stale `bridge: false`
// could win and tear the menu down, which made reaching the recent-files list
// fail intermittently. Track the two regions independently so the handoff
// always keeps the menu open, regardless of event order.
#[gpui::test]
async fn submenu_survives_bridge_to_panel_hover_handoff(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.open_menu_bar(0, cx);
        editor.open_menu_submenu(3, cx);

        // Crossing the gap: only the bridge is hovered.
        editor.set_menu_panel_hovered(false, cx);
        editor.set_menu_bar_hovered(false, cx);
        editor.set_menu_submenu_bridge_hovered(true, cx);
        assert!(editor.menu_close_task.is_none());

        // Handoff into the submenu panel. The bridge reporting `false` after
        // the panel is already hovered must not schedule a close.
        editor.set_menu_submenu_panel_hovered(true, cx);
        editor.set_menu_submenu_bridge_hovered(false, cx);

        assert_eq!(editor.menu_bar_open, Some(0));
        assert_eq!(editor.menu_submenu_open, Some(3));
        assert!(editor.menu_submenu_panel_hovered);
        assert!(
            editor.menu_close_task.is_none(),
            "menu must stay open across the bridge-to-panel handoff"
        );

        editor.close_menu_bar(cx);
    });
}

#[gpui::test]
async fn starting_and_ending_scrollbar_drag_updates_editor_state(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.pending_scroll_active_block_into_view = true;
        editor.pending_scroll_recheck_after_layout = true;

        editor.start_scrollbar_drag(12.0, 320.0, 64.0, 500.0, cx);
        assert_eq!(
            editor.scrollbar_drag,
            Some(super::ScrollbarDragSession {
                pointer_offset_y: 12.0,
                track_height: 320.0,
                thumb_height: 64.0,
                max_scroll_y: 500.0,
            })
        );
        assert!(!editor.pending_scroll_active_block_into_view);
        assert!(!editor.pending_scroll_recheck_after_layout);

        editor.update_scrollbar_drag(172.0, cx);
        let offset_y = -f32::from(editor.scroll_handle.offset().y);
        assert!(offset_y > 0.0);

        editor.end_scrollbar_drag(cx);
        assert!(editor.scrollbar_drag.is_none());
    });
}

#[gpui::test]
async fn parsed_table_runtime_installs_column_alignment_on_cells(cx: &mut TestAppContext) {
    let markdown = [
        "| Left | Center | Right |",
        "| :--- | :---: | ---: |",
        "| a | b | c |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        assert_eq!(table.read(cx).kind(), BlockKind::Table);
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        assert_eq!(
            runtime.header[0].read(cx).table_cell_alignment(),
            Some(TableColumnAlignment::Left)
        );
        assert_eq!(
            runtime.header[1].read(cx).table_cell_alignment(),
            Some(TableColumnAlignment::Center)
        );
        assert_eq!(
            runtime.rows[0][2].read(cx).table_cell_alignment(),
            Some(TableColumnAlignment::Right)
        );
    });
}

#[gpui::test]
async fn short_delimiter_dashes_still_render_as_tables(cx: &mut TestAppContext) {
    // 回归：`|:--|:--:|` 表头和 `htmd` 产出的 `| ---- | -- |` 分隔行曾被判为
    // “不是表格”，整段降级成纯文本。
    let markdown = [
        "| 分组 | 总数 | 保留 | 存疑 | 剔除 |",
        "|:--|:--:|:--:|:--:|:--:|",
        "| 1a 产物分 >0.8 | 1 | 1 | 0 | 0 |",
        "| **合计** | **3** | **2** | **0** | **1** |",
        "",
        "| 源文件 | 行数 | 动作 |",
        "| ---- | --- | -- |",
        "| `a.md` | 160 | 增强 |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.root_count(), 2);
        let roots = editor.document.root_blocks();
        for root in roots {
            assert_eq!(root.read(cx).kind(), BlockKind::Table);
        }
        let record = roots[0]
            .read(cx)
            .record
            .table
            .as_ref()
            .expect("table record");
        assert_eq!(record.alignments.len(), 5);
        assert_eq!(record.rows.len(), 2);
    });
}

#[gpui::test]
async fn append_column_updates_table_and_focuses_new_header_cell(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | ---: |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.append_table_column(&table, cx);

        let record = table
            .read(cx)
            .record
            .table
            .as_ref()
            .expect("table record after append");
        assert_eq!(record.header.len(), 3);
        assert_eq!(record.rows[0].len(), 3);
        assert_eq!(
            record.alignments,
            vec![
                TableColumnAlignment::Default,
                TableColumnAlignment::Right,
                TableColumnAlignment::Right,
            ]
        );

        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("rebuilt runtime");
        let focused = runtime.header[2].entity_id();
        assert_eq!(editor.pending_focus, Some(focused));
    });
}

#[gpui::test]
async fn append_row_updates_table_and_focuses_first_cell_of_new_row(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | :---: |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.append_table_row(&table, cx);

        let record = table
            .read(cx)
            .record
            .table
            .as_ref()
            .expect("table record after append");
        assert_eq!(record.rows.len(), 2);
        assert_eq!(record.rows[1].len(), 2);
        assert!(
            record.rows[1]
                .iter()
                .all(|cell| cell.serialize_markdown().is_empty())
        );

        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("rebuilt runtime");
        let focused = runtime.rows[1][0].entity_id();
        assert_eq!(editor.pending_focus, Some(focused));
    });
}

#[gpui::test]
async fn setting_column_alignment_updates_record_and_selection(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.set_table_column_alignment(&table, 1, TableColumnAlignment::Right, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(
            record.alignments,
            vec![TableColumnAlignment::Default, TableColumnAlignment::Right]
        );
        assert_eq!(
            editor.table_axis_selection,
            Some(super::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Column,
                index: 1,
            })
        );
    });
}

#[gpui::test]
async fn moving_table_row_updates_focus_and_selection(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Visual row 2 is the second body row; move it up above the first.
        editor.move_table_row(&table, 2, -1, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.rows[0][0].serialize_markdown(), "3");
        assert_eq!(
            editor.table_axis_selection,
            Some(super::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Row,
                index: 1,
            })
        );

        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("rebuilt runtime");
        assert_eq!(editor.pending_focus, Some(runtime.rows[0][0].entity_id()));
    });
}

#[gpui::test]
async fn moving_first_body_row_up_swaps_with_header(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Visual row 1 (first body row) moves up into the header position.
        editor.move_table_row(&table, 1, -1, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.header[0].serialize_markdown(), "1");
        assert_eq!(record.rows[0][0].serialize_markdown(), "A");
        assert_eq!(
            editor.table_axis_selection,
            Some(super::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Row,
                index: 0,
            })
        );
    });
}

#[gpui::test]
async fn moving_header_row_down_swaps_with_first_body(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Visual row 0 (header) moves down, swapping with the first body row.
        editor.move_table_row(&table, 0, 1, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.header[0].serialize_markdown(), "1");
        assert_eq!(record.rows[0][0].serialize_markdown(), "A");
        assert_eq!(
            editor.table_axis_selection,
            Some(super::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Row,
                index: 1,
            })
        );
    });
}

#[gpui::test]
async fn column_selection_does_not_insert_a_blank_row(cx: &mut TestAppContext) {
    use crate::components::TableAxisKind;
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "| A | B |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |".into(), None)
    });
    redraw(cx);
    let (table, header_before) = editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().unwrap().clone();
        let header = table.read(cx).table_runtime.as_ref().unwrap().header[0].read(cx).last_bounds.unwrap();
        (table, header)
    });
    editor.update(cx, |editor, cx| {
        editor.select_table_axis(table.entity_id(), TableAxisKind::Column, 0, cx);
    });
    redraw(cx);
    let header_after = table.read_with(cx, |table, cx| {
        table.table_runtime.as_ref().unwrap().header[0].read(cx).last_bounds.unwrap()
    });
    assert_eq!(header_before, header_after, "选择列不能插入空白行或推动表头");
    let indicator = cx.debug_bounds("table-column-indicator-0").expect("表头中的列指示线");
    assert!(indicator.top() < header_after.top());
    assert_eq!(indicator.size.height, px(2.0));
    let second = cx.debug_bounds("table-column-indicator-1").unwrap();
    cx.simulate_click(second.center(), Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        let selection = editor.table_axis_selection.unwrap();
        assert_eq!(selection.kind, TableAxisKind::Column);
        assert_eq!(selection.index, 1, "表头上沿仍应能直接选择整列");
    });
}

#[gpui::test]
async fn selecting_first_body_row_does_not_highlight_header(cx: &mut TestAppContext) {
    use crate::components::{TableAxisHighlight, TableAxisKind};
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |", "| 3 | 4 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Visual row 1 is the first body row; the header (row 0) must stay clear.
        editor.select_table_axis(table.entity_id(), TableAxisKind::Row, 1, cx);

        let runtime = table.read(cx).table_runtime.clone().expect("runtime");
        for cell in &runtime.header {
            assert_eq!(
                cell.read(cx).table_axis_highlight,
                TableAxisHighlight::None,
                "header should not be highlighted"
            );
        }
        for cell in &runtime.rows[0] {
            assert_eq!(
                cell.read(cx).table_axis_highlight,
                TableAxisHighlight::Selected
            );
        }
        for cell in &runtime.rows[1] {
            assert_eq!(cell.read(cx).table_axis_highlight, TableAxisHighlight::None);
        }
    });
}

#[gpui::test]
async fn selecting_header_row_highlights_only_header(cx: &mut TestAppContext) {
    use crate::components::{TableAxisHighlight, TableAxisKind};
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.select_table_axis(table.entity_id(), TableAxisKind::Row, 0, cx);

        let runtime = table.read(cx).table_runtime.clone().expect("runtime");
        for cell in &runtime.header {
            assert_eq!(
                cell.read(cx).table_axis_highlight,
                TableAxisHighlight::Selected
            );
        }
        for cell in &runtime.rows[0] {
            assert_eq!(cell.read(cx).table_axis_highlight, TableAxisHighlight::None);
        }
    });
}

#[gpui::test]
async fn body_row_preview_survives_stale_header_leave(cx: &mut TestAppContext) {
    use crate::components::TableAxisKind;
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let id = table.entity_id();

        // Pointer crosses from the header handle down onto the first body row.
        // The body handle's enter arrives first, then the header handle's leave;
        // the stale leave must not clear the preview the pointer moved onto.
        editor.preview_table_axis(id, TableAxisKind::Row, 1, true, cx);
        editor.preview_table_axis(id, TableAxisKind::Row, 0, false, cx);
        assert_eq!(
            editor.table_axis_preview,
            Some(super::TableAxisSelection {
                table_block_id: id,
                kind: TableAxisKind::Row,
                index: 1,
            }),
            "body row preview must survive the header's stale leave"
        );

        // Leaving the body handle that owns the preview still clears it.
        editor.preview_table_axis(id, TableAxisKind::Row, 1, false, cx);
        assert_eq!(editor.table_axis_preview, None);
    });
}

#[gpui::test]
async fn deleting_table_column_moves_selection_to_nearest_survivor(cx: &mut TestAppContext) {
    let markdown = ["| A | B | C |", "| --- | --- | --- |", "| 1 | 2 | 3 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.delete_table_column(&table, 2, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.header.len(), 2);
        assert_eq!(
            editor.table_axis_selection,
            Some(super::TableAxisSelection {
                table_block_id: table.entity_id(),
                kind: crate::components::TableAxisKind::Column,
                index: 1,
            })
        );
    });
}

#[gpui::test]
async fn deleting_table_header_promotes_next_row(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.delete_table_header_row(&table, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert_eq!(record.header[0].serialize_markdown(), "1");
        assert_eq!(record.header[1].serialize_markdown(), "2");
        assert!(record.rows.is_empty());

        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("rebuilt runtime");
        assert_eq!(editor.pending_focus, Some(runtime.header[0].entity_id()));
    });
}

#[gpui::test]
async fn deleting_last_body_row_leaves_header_only_table(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        // Deleting the only body row used to be blocked; now it leaves a
        // header-only table behind.
        editor.delete_table_row(&table, 0, cx);

        let record = table.read(cx).record.table.as_ref().expect("table record");
        assert!(record.rows.is_empty());
        assert_eq!(record.header[0].serialize_markdown(), "A");
        assert_eq!(editor.document.root_count(), 1);
        assert_eq!(table.read(cx).kind(), BlockKind::Table);
    });
}

#[gpui::test]
async fn removing_table_block_replaces_it_with_empty_paragraph(cx: &mut TestAppContext) {
    let markdown = [
        "intro",
        "",
        "| A | B |",
        "| --- | --- |",
        "| 1 | 2 |",
        "",
        "outro",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.root_blocks()[1].clone();
        assert_eq!(table.read(cx).kind(), BlockKind::Table);
        editor.remove_table_block(&table, cx);

        let roots = editor.document.root_blocks();
        assert_eq!(roots.len(), 3);
        assert_eq!(roots[0].read(cx).display_text(), "intro");
        assert_eq!(roots[1].read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(roots[1].read(cx).display_text(), "");
        assert_eq!(roots[2].read(cx).display_text(), "outro");
        assert_eq!(editor.pending_focus, Some(roots[1].entity_id()));
    });
}

#[gpui::test]
async fn removing_the_only_table_leaves_one_empty_paragraph(cx: &mut TestAppContext) {
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        editor.remove_table_block(&table, cx);

        let roots = editor.document.root_blocks();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(roots[0].read(cx).display_text(), "");
    });
}

#[gpui::test]
async fn standalone_root_image_installs_runtime_and_resolves_relative_path(
    cx: &mut TestAppContext,
) {
    let markdown = "![diagram](./assets/diagram.png \"System diagram\")".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.title.as_deref(), Some("System diagram"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn standalone_root_image_with_underscores_installs_runtime(cx: &mut TestAppContext) {
    let markdown =
        "![1.1_进制转换例子](./NetworkEngineerSummer.assets/1.1_进制转换例子.jpg)".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "1.1_进制转换例子");
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("NetworkEngineerSummer.assets/1.1_进制转换例子.jpg")
            )
        );
        assert_eq!(editor.document.markdown_text(cx), markdown);
    });
}

#[gpui::test]
async fn indented_root_images_install_runtime_before_indented_code(cx: &mut TestAppContext) {
    let url1 = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508201435626.png";
    let url2 = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508201438742.png";
    let url3 = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508201439288.png";
    let url4 = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508201419865.png";
    let markdown = [
        format!("![image-1]({})", url1.replace("_", "\\_")),
        String::new(),
        format!("   ![image-2]({})", url2.replace("_", "\\_")),
        String::new(),
        format!("        ![image-3]({})", url3.replace("_", "\\_")),
        String::new(),
        "   所有组或用户名均对**Anaconda安装目录**的权限设置为**完全控制**后，如下图所示："
            .to_string(),
        String::new(),
        format!("![image-4]({})", url4.replace("_", "\\_")),
        String::new(),
        "    plain indented code".to_string(),
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        let image_sources = roots
            .iter()
            .filter_map(|block| {
                block
                    .read(cx)
                    .image_runtime()
                    .map(|runtime| runtime.src.clone())
            })
            .collect::<Vec<_>>();
        assert_eq!(image_sources, vec![url1, url2, url3, url4]);
        assert!(
            roots
                .iter()
                .any(|block| matches!(block.read(cx).kind(), BlockKind::CodeBlock { .. }))
        );
    });
}

#[gpui::test]
async fn mixed_text_does_not_activate_image_runtime(cx: &mut TestAppContext) {
    let markdown = "before ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        assert!(block.read(cx).image_runtime().is_none());
    });
}

#[gpui::test]
async fn ordinary_edit_skips_global_context_but_image_edits_refresh_it(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| {
        Editor::from_markdown(
            cx,
            "# Heading\n\n![old](https://example.com/old.png)".into(),
            None,
        )
    });
    let (heading, image, definitions) = editor.read_with(cx, |editor, _cx| {
        (
            editor.document.root_blocks()[0].clone(),
            editor.document.root_blocks()[1].clone(),
            editor.image_reference_definitions.clone(),
        )
    });

    heading.update(cx, |heading, cx| {
        heading.record.set_title(InlineTextTree::plain("Updated"));
        heading.sync_render_cache();
        cx.emit(BlockEvent::Changed);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(Arc::ptr_eq(
            &definitions,
            &editor.image_reference_definitions
        ));
    });

    image.update(cx, |image, cx| {
        image.record.set_title(InlineTextTree::plain("plain"));
        image.sync_render_cache();
        cx.emit(BlockEvent::Changed);
    });
    cx.run_until_parked();
    assert!(image.read_with(cx, |image, _cx| image.image_runtime().is_none()));

    image.update(cx, |image, cx| {
        image
            .record
            .set_title(InlineTextTree::plain("![new](https://example.com/new.png)"));
        image.sync_render_cache();
        cx.emit(BlockEvent::Changed);
    });
    cx.run_until_parked();
    image.read_with(cx, |image, _cx| {
        assert_eq!(
            image
                .image_runtime()
                .as_ref()
                .map(|runtime| runtime.src.as_str()),
            Some("https://example.com/new.png")
        );
    });
}

#[gpui::test]
async fn editing_image_reference_definition_refreshes_existing_image(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| {
        Editor::from_markdown(
            cx,
            "![photo][asset]\n\n[asset]: https://example.com/old.png".into(),
            None,
        )
    });
    let (image, definition) = editor.read_with(cx, |editor, _cx| {
        (
            editor.document.root_blocks()[0].clone(),
            editor.document.root_blocks()[1].clone(),
        )
    });
    assert_eq!(
        image.read_with(cx, |image, _cx| image
            .image_runtime()
            .map(|runtime| runtime.src.clone())),
        Some("https://example.com/old.png".into())
    );

    definition.update(cx, |definition, cx| {
        definition.record.set_title(InlineTextTree::plain(
            "[asset]: https://example.com/new.png",
        ));
        definition.sync_render_cache();
        cx.emit(BlockEvent::Changed);
    });
    cx.run_until_parked();
    assert_eq!(
        image.read_with(cx, |image, _cx| image
            .image_runtime()
            .map(|runtime| runtime.src.clone())),
        Some("https://example.com/new.png".into())
    );
}

#[gpui::test]
async fn reference_style_root_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown =
        "![reference image][ref-image]\n\n[ref-image]: ./assets/ref-image.png \"Caption\""
            .to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "reference image");
        assert_eq!(runtime.src, "./assets/ref-image.png");
        assert_eq!(runtime.title.as_deref(), Some("Caption"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/ref-image.png")
            )
        );
    });
}

#[gpui::test]
async fn quote_child_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = ">     ![diagram](./assets/diagram.png \"Caption\")".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let quote = editor.document.first_root().expect("quote root").clone();
        let image_block = quote
            .read(cx)
            .children
            .first()
            .expect("quote image child")
            .clone();
        let runtime = image_block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Caption"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn bulleted_list_item_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = "-     ![diagram](./assets/diagram.png \"Caption\")".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Caption"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn html_fallback_before_image_does_not_swallow_standalone_image(cx: &mut TestAppContext) {
    let image_url = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508200941158.png";
    let markdown = format!(
        "<span style='color:blue;'>Anaconda下载地址</span>：https://mirrors.tuna.tsinghua.edu.cn/anaconda/archive/\n\n![image-20250820094109009]({image_url})"
    );
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.root_count(), 2);
        {
            let html = editor.document.root_blocks()[0].read(cx);
            assert_eq!(html.kind(), BlockKind::HtmlBlock);
            assert!(
                html.display_text()
                    .starts_with("<span style='color:blue;'>")
            );
            assert!(
                html.record
                    .html
                    .as_ref()
                    .is_some_and(|html| html.is_semantic())
            );
        }

        let image = editor.document.root_blocks()[1].read(cx);
        let runtime = image.image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "image-20250820094109009");
        assert_eq!(runtime.src, image_url);
        match &runtime.resolved_source {
            ImageResolvedSource::Remote(uri) => assert_eq!(uri.to_string(), image_url),
            other => panic!("expected remote image, got {other:?}"),
        }
    });
}

#[gpui::test]
async fn unclosed_html_block_stops_before_standalone_image_without_blank(cx: &mut TestAppContext) {
    let image_url = "https://example.com/image.png";
    let markdown = format!("<span>unclosed html\n![image]({image_url})");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.root_count(), 2);
        // html5ever recovers the unclosed tag like a browser does; the block is
        // still an HTML block, not raw Markdown.
        let html = editor.document.root_blocks()[0].read(cx);
        assert_eq!(html.kind(), BlockKind::HtmlBlock);
        assert!(
            html.record
                .html
                .as_ref()
                .is_some_and(|html| html.is_semantic())
        );
        let image = editor.document.root_blocks()[1].read(cx);
        let runtime = image.image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "image");
        assert_eq!(runtime.src, image_url);
    });
}

#[gpui::test]
async fn numbered_list_item_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = "1. ![diagram](https://example.com/diagram.gif \"Caption\")".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.title.as_deref(), Some("Caption"));
        match &runtime.resolved_source {
            ImageResolvedSource::Remote(uri) => {
                assert_eq!(uri.to_string(), "https://example.com/diagram.gif");
            }
            other => panic!("expected remote source, got {other:?}"),
        }
    });
}

#[gpui::test]
async fn task_list_item_reference_style_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = "- [ ] ![diagram][cover]\n\n[cover]: ./assets/diagram.png \"Cover\"".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("task list item root")
            .clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Cover"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn mixed_list_item_title_does_not_activate_image_runtime(cx: &mut TestAppContext) {
    let markdown = "- text ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        assert!(block.read(cx).image_runtime().is_none());
    });
}

#[gpui::test]
async fn list_child_reference_style_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = [
        "- item",
        "  ![diagram][cover]",
        "",
        "[cover]: ./assets/diagram.png \"Cover\"",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let list_item = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let image_block = list_item
            .read(cx)
            .children
            .first()
            .expect("list child image")
            .clone();
        let runtime = image_block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Cover"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn list_scoped_reference_definition_supports_list_item_image_runtime(
    cx: &mut TestAppContext,
) {
    let markdown = [
        "- ![diagram][cover]",
        "  [cover]: ./assets/diagram.png \"Cover\"",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let list_item = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let runtime = list_item.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Cover"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
        assert_eq!(
            list_item
                .read(cx)
                .children
                .first()
                .expect("reference definition child")
                .read(cx)
                .kind(),
            BlockKind::RawMarkdown
        );
    });
}

#[gpui::test]
async fn quote_list_item_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = "> - ![diagram](./assets/diagram.png)".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let quote = editor.document.first_root().expect("quote root").clone();
        let list_item = quote
            .read(cx)
            .children
            .first()
            .expect("quote list child")
            .clone();
        let runtime = list_item.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn callout_task_list_reference_style_image_uses_container_scoped_definition(
    cx: &mut TestAppContext,
) {
    let markdown = [
        "> [!NOTE]",
        "> - [ ] ![diagram][cover]",
        ">",
        "> [cover]: ./assets/diagram.png \"Cover\"",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let callout = editor.document.first_root().expect("callout root").clone();
        let list_item = callout
            .read(cx)
            .children
            .first()
            .expect("callout list child")
            .clone();
        let runtime = list_item.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Cover"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn callout_list_child_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = [
        "> [!NOTE]",
        "> - item",
        ">   ![diagram](./assets/diagram.png)",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let callout = editor.document.first_root().expect("callout root").clone();
        let list_item = callout
            .read(cx)
            .children
            .first()
            .expect("callout list child")
            .clone();
        let image_block = list_item
            .read(cx)
            .children
            .first()
            .expect("list child image")
            .clone();
        let runtime = image_block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn callout_child_reference_style_image_uses_container_scoped_definition(
    cx: &mut TestAppContext,
) {
    let markdown = [
        "> [!NOTE]",
        ">     ![diagram][anim]",
        ">",
        "> [anim]: ./assets/diagram.png \"Animated\"",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let callout = editor.document.first_root().expect("callout root").clone();
        let image_block = callout
            .read(cx)
            .children
            .iter()
            .find(|child| {
                child.read(cx).kind() == BlockKind::Paragraph
                    && child.read(cx).image_runtime().is_some()
            })
            .expect("callout image child")
            .clone();
        let runtime = image_block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Animated"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn table_cell_with_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = [
        "| Preview |",
        "| --- |",
        "|    ![diagram](https://example.com/diagram.gif \"Animated\") |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        let cell_runtime = runtime.rows[0][0]
            .read(cx)
            .image_runtime()
            .expect("cell image runtime");
        assert_eq!(cell_runtime.alt, "diagram");
        assert_eq!(cell_runtime.title.as_deref(), Some("Animated"));
        match &cell_runtime.resolved_source {
            ImageResolvedSource::Remote(uri) => {
                assert_eq!(uri.to_string(), "https://example.com/diagram.gif");
            }
            other => panic!("expected remote source, got {other:?}"),
        }
    });
}

#[gpui::test]
async fn table_cell_with_mixed_inline_image_uses_inline_image_segments(cx: &mut TestAppContext) {
    let markdown = [
        "| Preview |",
        "| --- |",
        "| image ![alt](https://example.com/x.png) |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        let cell = runtime.rows[0][0].read(cx);
        assert!(cell.image_runtime().is_none());

        let segments = parse_table_cell_inline_images(&cell.record.title_markdown());
        assert_eq!(segments.len(), 2);
        assert_eq!(
            segments[0],
            TableCellInlineImageSegment::Text("image ".to_string())
        );
        assert!(matches!(
            &segments[1],
            TableCellInlineImageSegment::Image { syntax, .. }
                if syntax.alt == "alt"
                    && syntax
                        .resolve_target(&ImageReferenceDefinitions::default())
                        .is_some_and(|target| target.src == "https://example.com/x.png")
        ));
    });
}

#[gpui::test]
async fn table_cell_with_reference_style_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = [
        "| Preview |",
        "| --- |",
        "| ![diagram][anim] |",
        "",
        "[anim]: https://example.com/diagram.gif \"Animated\"",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        let cell_runtime = runtime.rows[0][0]
            .read(cx)
            .image_runtime()
            .expect("cell image runtime");
        assert_eq!(cell_runtime.alt, "diagram");
        assert_eq!(cell_runtime.title.as_deref(), Some("Animated"));
        match &cell_runtime.resolved_source {
            ImageResolvedSource::Remote(uri) => {
                assert_eq!(uri.to_string(), "https://example.com/diagram.gif");
            }
            other => panic!("expected remote source, got {other:?}"),
        }
    });
}

#[gpui::test]
async fn reference_style_link_in_root_paragraph_resolves_document_wide(cx: &mut TestAppContext) {
    let markdown = [
        "[reference link][ref-link]",
        "",
        "[ref-link]: https://example.com",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        assert_eq!(block.read(cx).display_text(), "reference link");
        assert_eq!(
            block.read(cx).inline_link_at(0),
            Some("https://example.com")
        );
    });
}

#[gpui::test]
async fn reference_style_link_in_table_cell_resolves_document_wide(cx: &mut TestAppContext) {
    let markdown = [
        "| Link |",
        "| --- |",
        "| [reference link][ref-link] |",
        "",
        "[ref-link]: https://example.com",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        let cell = runtime.rows[0][0].clone();
        assert_eq!(cell.read(cx).display_text(), "reference link");
        assert_eq!(cell.read(cx).inline_link_at(0), Some("https://example.com"));
    });
}

#[gpui::test]
async fn root_level_footnotes_number_by_first_reference_and_render_in_place(
    cx: &mut TestAppContext,
) {
    let markdown = [
        "Here is a footnote reference.[^1]",
        "",
        "Here is another footnote reference.[^longnote]",
        "",
        "A footnote can appear after multiple paragraphs, lists, and code blocks.",
        "",
        "[^1]: Footnote text.",
        "",
        "[^longnote]: Footnote text with **bold**, `code`, and a nested list:",
        "    - item 1",
        "    - item 2",
        "    ",
        "    Second paragraph in the footnote.",
    ]
    .join("\n");
    let canonical_markdown = [
        "Here is a footnote reference.[^1]",
        "",
        "Here is another footnote reference.[^longnote]",
        "",
        "A footnote can appear after multiple paragraphs, lists, and code blocks.",
        "",
        "[^1]: Footnote text.",
        "",
        "[^longnote]: Footnote text with **bold**, `code`, and a nested list:",
        "",
        "    - item 1",
        "    - item 2",
        "",
        "    Second paragraph in the footnote.",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();

        let first_ref = visible
            .iter()
            .find(|visible| {
                visible
                    .entity
                    .read(cx)
                    .display_text()
                    .contains("Here is a footnote reference.")
            })
            .expect("first footnote reference")
            .entity
            .clone();
        assert_eq!(
            first_ref.read(cx).display_text(),
            format!("Here is a footnote reference.{}", superscript_ordinal(1))
        );

        let second_ref = visible
            .iter()
            .find(|visible| {
                visible
                    .entity
                    .read(cx)
                    .display_text()
                    .contains("Here is another footnote reference.")
            })
            .expect("second footnote reference")
            .entity
            .clone();
        assert_eq!(
            second_ref.read(cx).display_text(),
            format!(
                "Here is another footnote reference.{}",
                superscript_ordinal(2)
            )
        );

        let footnote_defs = visible
            .iter()
            .filter_map(|visible| {
                let block = visible.entity.read(cx);
                (block.kind() == BlockKind::FootnoteDefinition).then_some(visible.entity.clone())
            })
            .collect::<Vec<_>>();
        assert_eq!(footnote_defs.len(), 2);
        assert_eq!(footnote_defs[0].read(cx).display_text(), "1");
        assert_eq!(
            footnote_defs[0].read(cx).footnote_definition_ordinal(),
            Some(1)
        );
        assert_eq!(footnote_defs[1].read(cx).display_text(), "longnote");
        assert_eq!(
            footnote_defs[1].read(cx).footnote_definition_ordinal(),
            Some(2)
        );

        assert_eq!(editor.document.markdown_text(cx), canonical_markdown);
    });
}

#[gpui::test]
async fn callout_footnotes_number_and_render_in_place(cx: &mut TestAppContext) {
    let markdown = [
        "> [!WARNING]",
        "> Callout footnote reference.[^final]",
        "> ",
        "> [^final]: Nested footnote text.",
        "> Tail paragraph.",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();

        let reference_block = visible
            .iter()
            .find(|visible| {
                visible
                    .entity
                    .read(cx)
                    .display_text()
                    .contains("Callout footnote reference.")
            })
            .expect("callout footnote reference")
            .entity
            .clone();
        assert_eq!(
            reference_block.read(cx).display_text(),
            format!("Callout footnote reference.{}", superscript_ordinal(1))
        );

        let definition = visible
            .iter()
            .find(|visible| visible.entity.read(cx).kind() == BlockKind::FootnoteDefinition)
            .expect("callout footnote definition")
            .entity
            .clone();
        assert_eq!(definition.read(cx).display_text(), "final");
        assert_eq!(definition.read(cx).quote_depth, 1);
        assert_eq!(definition.read(cx).footnote_definition_ordinal(), Some(1));
        assert_eq!(editor.document.markdown_text(cx), markdown);
    });
}

#[gpui::test]
async fn root_reference_binds_to_nested_quote_footnote_definition(cx: &mut TestAppContext) {
    let markdown = "Root reference.[^note]\n\n> [^note]: Nested quote footnote".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();

        let root_reference = visible
            .iter()
            .find(|visible| visible.entity.read(cx).quote_depth == 0)
            .expect("root reference block")
            .entity
            .clone();
        assert_eq!(
            root_reference.read(cx).display_text(),
            format!("Root reference.{}", superscript_ordinal(1))
        );

        let definition = visible
            .iter()
            .find(|visible| visible.entity.read(cx).kind() == BlockKind::FootnoteDefinition)
            .expect("nested quote footnote definition")
            .entity
            .clone();
        assert_eq!(definition.read(cx).display_text(), "note");
        assert_eq!(definition.read(cx).quote_depth, 1);
        assert_eq!(definition.read(cx).footnote_definition_ordinal(), Some(1));
        assert_eq!(editor.document.markdown_text(cx), markdown);
    });
}

#[gpui::test]
async fn unresolved_footnote_reference_stays_literal_and_unlinked(cx: &mut TestAppContext) {
    let markdown = "Missing footnote[^missing].".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("root paragraph")
            .clone();
        assert_eq!(block.read(cx).display_text(), markdown);
        assert!(
            block
                .read(cx)
                .inline_footnote_hit_at("Missing footnote".len())
                .is_none()
        );
        assert!(editor.footnote_registry.binding("missing").is_none());
        assert_eq!(editor.document.markdown_text(cx), markdown);
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_root_image_runtime(cx: &mut TestAppContext) {
    let markdown = "![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        assert!(block.read(cx).image_runtime().is_some());
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_reference_style_root_image_runtime(
    cx: &mut TestAppContext,
) {
    let markdown = "![diagram][ref]\n\n[ref]: ./assets/diagram.png".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.src, "./assets/diagram.png");
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_quote_child_image_runtime(cx: &mut TestAppContext) {
    let markdown = "> ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let quote = editor.document.first_root().expect("quote root").clone();
        let image_block = quote
            .read(cx)
            .children
            .first()
            .expect("quote image child")
            .clone();
        assert!(image_block.read(cx).image_runtime().is_some());
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_list_item_image_runtime(cx: &mut TestAppContext) {
    let markdown = "- ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        assert!(block.read(cx).image_runtime().is_some());
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_list_child_image_runtime(cx: &mut TestAppContext) {
    let markdown = "- item\n  ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let list_item = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let image_block = list_item
            .read(cx)
            .children
            .first()
            .expect("list child image")
            .clone();
        assert!(image_block.read(cx).image_runtime().is_some());
    });
}

#[gpui::test]
async fn undo_reverts_recent_rendered_typing(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " beta", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "alpha beta");
        assert_eq!(editor.undo_history.len(), 1);
        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha");
    });
}

#[gpui::test]
async fn toc_block_renders_entries_and_jumps_to_heading(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "# Title\n\n[TOC]\n\n## Section\n\nbody";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    redraw(cx);

    let (toc_block, section_heading) = editor.update(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().to_vec();
        assert_eq!(visible.len(), 4); // Title, TOC, Section, body
        (visible[1].entity.clone(), visible[2].entity.clone())
    });
    editor.read_with(cx, |_editor, cx| {
        let entries = &toc_block.read(cx).toc_entries;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Title");
        assert_eq!(entries[1].title, "Section");
        assert_eq!(entries[1].line, 4);
    });

    // 目录条目按层级渲染并可点击。
    let bounds = cx
        .debug_bounds("toc-entry-1")
        .expect("second TOC entry is rendered");
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.active_entity_id,
            Some(section_heading.entity_id()),
            "clicking a TOC entry focuses the target heading"
        );
        // 光标落在 `## Section` 这一行上（"# Title\n\n[TOC]\n\n" 共 16 字节，
        // 渲染态把整行选区收敛到标题文字末尾，即第 26 字节）。
        let caret = editor.capture_source_selection_snapshot(cx).range.start;
        assert!(
            (16..=26).contains(&caret),
            "caret should land on the heading line, got {caret}"
        );
    });
}

#[gpui::test]
async fn manual_external_change_policy_keeps_buffer_until_user_reload(cx: &mut TestAppContext) {
    // roadmap H2：外部变更策略 manual 时不自动重载，auto 时重载。
    init_editor_test_app(cx);
    let path = temp_markdown_path("external-policy");
    fs::write(&path, "disk v1").expect("seed");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "disk v1".into(), Some(path))
    });

    cx.update(|_window, cx| {
        // 安装 EditorSettings 全局（否则 setter 只落盘、不改内存）。
        crate::config::EditorSettings::init(cx, true);
        crate::config::EditorSettings::set_external_change_policy(
            cx,
            crate::config::ExternalChangePolicy::Manual,
        );
    });
    fs::write(&path, "disk v2").expect("external write");
    editor.update(cx, |editor, cx| {
        editor.reload_externally_changed_document(&path, cx);
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx),
            "disk v1",
            "manual policy must not reload externally changed documents"
        );
    });

    cx.update(|_window, cx| {
        crate::config::EditorSettings::set_external_change_policy(
            cx,
            crate::config::ExternalChangePolicy::Auto,
        );
    });
    editor.update(cx, |editor, cx| {
        editor.reload_externally_changed_document(&path, cx);
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx),
            "disk v2",
            "auto policy reloads unedited documents"
        );
    });
    // 复原默认策略，避免影响同进程其他用例读取配置。
    cx.update(|_window, cx| {
        crate::config::EditorSettings::set_external_change_policy(
            cx,
            crate::config::ExternalChangePolicy::Auto,
        );
    });
}

#[test]
fn permanent_delete_removes_file_and_directory() {
    // roadmap H2：永久删除策略的落盘行为。
    let root = std::env::temp_dir().join(format!("velora-permadelete-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("nested")).expect("create nested");
    let file = root.join("gone.md");
    fs::write(&file, "bye").expect("write file");

    crate::editor::workspace::permanent_delete(&file, false).expect("delete file");
    assert!(!file.exists());
    crate::editor::workspace::permanent_delete(&root.join("nested"), true).expect("delete dir");
    assert!(!root.join("nested").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn recovery_snapshot_merges_into_open_session_tab(cx: &mut TestAppContext) {
    // roadmap E10：会话已打开同一文件时，恢复快照并入标签而不是另开窗口。
    init_editor_test_app(cx);
    let path = temp_markdown_path("merge-recovery");
    fs::write(&path, "disk version\n").expect("seed file");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| Editor::from_markdown(cx, "disk version\n".into(), Some(path))
    });
    let snapshot_id = uuid::Uuid::new_v4();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            // 注册为会话标签（模拟 A4 会话恢复已打开该文件）。
            editor.snapshot_current_document(cx);
            assert!(
                editor
                    .workspace_open_document_paths()
                    .iter()
                    .any(|open| open == &path)
            );

            assert!(editor.merge_recovery_snapshot(
                &path,
                "unsaved edits\n",
                snapshot_id,
                window,
                cx,
            ));
            assert_eq!(editor.document.markdown_text(cx).trim_end(), "unsaved edits");
            assert!(editor.document_dirty);
            assert_eq!(editor.recovery_id, snapshot_id);
            let (dirty, tab_recovery_id, markdown) = editor
                .workspace_tab_state_for_test(&path)
                .expect("tab");
            assert!(dirty);
            assert_eq!(tab_recovery_id, snapshot_id);
            assert_eq!(markdown.trim_end(), "unsaved edits");
        });
    });
    // 磁盘仍是旧内容，未保存内容留在内存标签里。
    assert_eq!(fs::read_to_string(&path).unwrap(), "disk version\n");
}

#[gpui::test]
async fn tree_copy_then_paste_duplicates_file_into_selected_folder(cx: &mut TestAppContext) {
    // roadmap D6：树右键 复制 → 目标目录 粘贴 生成副本。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-tree-paste-{}", uuid::Uuid::new_v4()));
    let sub = root.join("notes");
    std::fs::create_dir_all(&sub).expect("create sub");
    let doc = root.join("alpha.md");
    std::fs::write(&doc, "# Alpha\n").expect("write doc");

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, String::new(), None)
    });
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.select_workspace_path_for_test(doc.clone(), cx);
        editor.copy_selected_workspace_file(cx);
        assert_eq!(editor.tree_clipboard.as_deref(), Some(doc.as_path()));
        editor.select_workspace_path_for_test(sub.clone(), cx);
        editor.paste_into_workspace_tree(cx);
    });

    let pasted = sub.join("alpha copy.md");
    assert!(pasted.exists(), "paste should create the copy in the folder");
    assert_eq!(std::fs::read_to_string(&pasted).unwrap(), "# Alpha\n");
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn tree_paste_writes_clipboard_image_with_date_hash_name(cx: &mut TestAppContext) {
    // roadmap D6：剪贴板图片粘贴到树，沿用 B10 命名模板。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-tree-img-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, String::new(), None)
    });
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.tree_clipboard = None;
        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            vec![0x89, b'P', b'N', b'G'],
        )));
        editor.select_workspace_path_for_test(root.clone(), cx);
        editor.paste_into_workspace_tree(cx);
    });

    let entries: Vec<_> = std::fs::read_dir(&root)
        .expect("read root")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let image_name = entries
        .iter()
        .find(|name| name.ends_with(".png"))
        .unwrap_or_else(|| panic!("expected a pasted png, got {entries:?}"));
    let stem = image_name.trim_end_matches(".png");
    let parts: Vec<&str> = stem.rsplitn(2, '-').collect();
    assert_eq!(parts[0].len(), 8, "hash suffix: {stem}");
    assert!(parts[0].chars().all(|ch| ch.is_ascii_hexdigit()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn copy_as_html_item_carries_html_flavor() {
    // roadmap F2 增强：纯文本 flavor 保留 HTML 源码，同时附带 text/html flavor。
    let html = "<h1>Title</h1>".to_string();
    let item = crate::editor::workspace::copy_as_html_clipboard_item(html.clone());
    assert_eq!(item.html(), Some(html.as_str()));
}

#[gpui::test]
async fn copy_as_html_writes_html_source_to_clipboard(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# Title\n\nbody".into(), None)
    });
    editor.update(cx, |editor, cx| editor.copy_as_html(cx));
    let text = cx.update(|_window, cx| {
        cx.read_from_clipboard().and_then(|item| item.text())
    });
    let text = text.expect("clipboard should hold HTML source");
    assert!(text.contains("<h1"), "expected rendered HTML, got: {text}");
}

#[gpui::test]
async fn long_paragraph_renders_as_plain_source(cx: &mut TestAppContext) {
    // roadmap B12：单块超阈值时渲染态降级为源码文本。
    init_editor_test_app(cx);
    let long_line = "x".repeat(crate::components::LONG_BLOCK_SOURCE_LIMIT + 1);
    let markdown = format!("# Title\n\n{long_line}\n\nshort");
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    redraw(cx);

    assert!(
        cx.debug_bounds("block-long-source").is_some(),
        "over-limit block falls back to the plain source element"
    );
    let blocks = editor.read_with(cx, |editor, _cx| editor.document.visible_blocks().to_vec());
    let long_block = &blocks[1].entity;
    assert!(
        long_block.read_with(cx, |block, _cx| block.exceeds_long_block_source_limit()),
        "the over-limit block reports the degraded state"
    );
    assert!(
        !blocks[2]
            .entity
            .read_with(cx, |block, _cx| block.exceeds_long_block_source_limit()),
        "short blocks keep the rich render path"
    );
}

#[gpui::test]
async fn code_block_copy_button_copies_code_to_clipboard(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "正文先获得焦点。\n\n```rust\nlet x = 1;\n```";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    redraw(cx);

    let bounds = cx
        .debug_bounds("code-copy-button")
        .expect("未聚焦代码块也应显示复制按钮");
    let header = cx.debug_bounds("code-block-header").expect("代码块顶栏");
    assert!(header.contains(&bounds.center()), "复制按钮应在顶部同一行");
    assert!(bounds.center().x > header.center().x, "复制按钮应在右侧");
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);

    let clipboard = cx.update(|_window, cx| {
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .map(|text| text.to_string())
    });
    assert_eq!(clipboard.as_deref(), Some("let x = 1;"));
    editor.read_with(cx, |editor, _cx| {
        // 点击复制不顺带移动光标/选中文字。
        let block = &editor.document.visible_blocks()[1].entity;
        assert!(block.read(_cx).selected_range.is_empty());
    });
}

#[gpui::test]
async fn code_block_copy_button_preserves_literal_backticks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "正文\n\n````text\n    keep indentation\nliteral ``` stays\n中文也保留\n````";
    let (_editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, markdown.into(), None)
    });
    redraw(cx);
    let button = cx.debug_bounds("code-copy-button").expect("复制按钮常驻");
    cx.simulate_click(button.center(), Modifiers::none());
    let clipboard = cx.update(|_window, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(clipboard.as_deref(), Some("    keep indentation\nliteral ``` stays\n中文也保留"));
}

#[gpui::test]
async fn undo_after_view_mode_switch_keeps_text(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " beta", None, false, cx);
        });
    });

    // 源码/渲染模式来回切换后，撤销历史仍应完整可用（roadmap B11）。
    editor.update(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "alpha beta");
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha");
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
        assert_eq!(editor.document.markdown_text(cx), "alpha");
        editor.redo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha beta");
    });
}

#[gpui::test]
async fn undo_first_edit_after_marker_normalization_restores_content(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "1) first".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("list root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, "!", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "1. first!");
        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "1. first");
    });
}

#[gpui::test]
async fn consecutive_text_edits_within_window_coalesce_into_one_undo(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "a".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());

        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(1..1, "b", None, false, cx);
        });
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(2..2, "c", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "abc");
        assert_eq!(editor.undo_history.len(), 1);

        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "a");
    });
}

#[gpui::test]
async fn redo_restores_text_reverted_by_undo(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " beta", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        editor.undo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha");
        assert_eq!(editor.redo_history.len(), 1);

        editor.redo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha beta");
        assert!(editor.redo_history.is_empty());
    });
}

#[gpui::test]
async fn fresh_edit_clears_pending_redo_history(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root").clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " beta", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        editor.undo_document(cx);
        assert_eq!(editor.redo_history.len(), 1);

        // A new edit invalidates the redo stack so it cannot revive stale text.
        let block = editor.document.first_root().expect("root").clone();
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(crate::components::UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(5..5, " gamma", None, false, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        editor.finalize_pending_undo_capture(cx);
        assert!(editor.redo_history.is_empty());

        editor.redo_document(cx);
        assert_eq!(editor.document.markdown_text(cx), "alpha gamma");
    });
}

#[gpui::test]
async fn toggle_view_mode_preserves_paragraph_caret_position(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None));

    editor.update(cx, |editor, cx| {
        let target = editor.document.visible_blocks()[1].entity.clone();
        target.update(cx, |block, _cx| {
            block.selected_range = 2..2;
        });
        editor.active_entity_id = Some(target.entity_id());

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        let source = editor.document.first_root().expect("source root").clone();
        assert_eq!(source.read(cx).selected_range, 9..9);
        assert!(source.read(cx).show_source_line_numbers());

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 2);
        assert!(
            visible
                .iter()
                .all(|visible| !visible.entity.read(cx).show_source_line_numbers())
        );
        assert_eq!(visible[1].entity.read(cx).display_text(), "beta");
        assert_eq!(visible[1].entity.read(cx).selected_range, 2..2);
        assert_eq!(editor.pending_focus, Some(visible[1].entity.entity_id()));
    });
}

#[gpui::test]
async fn toggle_view_mode_ends_stale_code_block_pointer_selection(cx: &mut TestAppContext) {
    let editor =
        cx.new(|cx| Editor::from_markdown(cx, "```rust\nfn main() {}\n```".to_string(), None));

    editor.update(cx, |editor, cx| {
        let target = editor.document.visible_blocks()[0].entity.clone();
        target.update(cx, |block, _cx| {
            block.selected_range = 3..7;
            block.is_selecting = true;
            block.code_language_is_selecting = true;
        });
        editor.active_entity_id = Some(target.entity_id());

        editor.toggle_view_mode(cx);

        assert!(matches!(editor.view_mode, ViewMode::Source));
        target.read_with(cx, |block, _cx| {
            assert!(!block.is_selecting);
            assert!(!block.code_language_is_selecting);
            assert_eq!(block.selected_range, 3..7);
        });
    });
}

#[gpui::test]
async fn ctrl_tab_toggles_view_mode(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    redraw(cx);
    cx.simulate_keystrokes("ctrl-tab");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert!(matches!(editor.view_mode, ViewMode::Source));
    });

    cx.simulate_keystrokes("ctrl-tab");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });
}

#[gpui::test]
async fn ctrl_a_selects_entire_source_document_in_source_mode(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        let source = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(source.entity_id());
        source.update(cx, |block, _cx| {
            block.selected_range = 1..3;
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let source = editor.document.visible_blocks()[0].entity.read(cx);
        assert_eq!(source.selected_range, 0..source.visible_len());
        assert!(editor.cross_block_selection.is_none());
    });
}

#[gpui::test]
async fn ctrl_a_selects_only_focused_block_text_in_rendered_mode(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[1].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, _cx| {
            block.selected_range = 1..1;
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let first = editor.document.visible_blocks()[0].entity.read(cx);
        let second = editor.document.visible_blocks()[1].entity.read(cx);
        assert_eq!(first.selected_range, 0..0);
        assert_eq!(second.selected_range, 0..second.visible_len());
        assert!(editor.cross_block_selection.is_none());
    });
}

#[gpui::test]
async fn repeated_ctrl_a_selects_all_rendered_blocks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown =
        "alpha\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\ngamma";
    let (editor, cx) = cx.add_window_view({
        let markdown = markdown.to_string();
        move |_window, cx| Editor::from_markdown(cx, markdown.clone(), None)
    });

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(0, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let first = editor.document.visible_blocks()[0].entity.read(cx);
        assert_eq!(first.selected_range, 0..first.visible_len());
        assert!(editor.cross_block_selection.is_none());
    });

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        let first_id = visible[0].entity.entity_id();
        let last = visible.last().expect("visible blocks");
        let last_id = last.entity.entity_id();
        let last_len = last.entity.read(cx).visible_len();
        let selection = editor
            .cross_block_selection
            .expect("second Ctrl+A should select the rendered document");
        assert_eq!(selection.anchor.entity_id, first_id);
        assert_eq!(selection.anchor.offset, 0);
        assert_eq!(selection.focus.entity_id, last_id);
        assert_eq!(selection.focus.offset, last_len);
        for visible in visible {
            let block = visible.entity.read(cx);
            let len = block.visible_len();
            if len > 0 {
                assert_eq!(block.editor_selection_range, Some(0..len));
            }
        }
    });

    let selected_after_second = editor.read_with(cx, |editor, _cx| editor.cross_block_selection);
    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.cross_block_selection, selected_after_second,
            "third Ctrl+A should keep the full rendered document selected"
        );
        for visible in editor.document.visible_blocks() {
            let block = visible.entity.read(cx);
            let len = block.visible_len();
            if len > 0 {
                assert_eq!(block.editor_selection_range, Some(0..len));
            }
        }
    });
}

#[gpui::test]
async fn rendered_ctrl_a_cycle_expires_before_second_press(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[1].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[1].entity.clone();
        block.update(cx, |block, _cx| {
            block.selected_range = 1..1;
        });
        let cycle = editor
            .rendered_select_all_cycle
            .as_mut()
            .expect("first Ctrl+A should arm the rendered select-all cycle");
        cycle.last_pressed_at =
            Instant::now() - (Editor::RENDERED_SELECT_ALL_CYCLE_WINDOW + Duration::from_millis(1));
    });

    cx.simulate_keystrokes("ctrl-a");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let second = editor.document.visible_blocks()[1].entity.read(cx);
        assert_eq!(second.selected_range, 0..second.visible_len());
        assert!(editor.cross_block_selection.is_none());
        assert_eq!(
            editor
                .rendered_select_all_cycle
                .expect("cycle should be reset by expired second press")
                .count,
            1
        );
    });
}

#[gpui::test]
async fn tab_key_inserts_tab_in_focused_paragraph(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "ab".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("tab");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(block.read(cx).display_text(), "a    b");
        assert_eq!(editor.document.markdown_text(cx), "a    b");
    });
}

#[gpui::test]
async fn tab_key_inserts_tab_in_focused_code_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("tab");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(block.read(cx).display_text(), "a    b");
        assert_eq!(editor.document.markdown_text(cx), "```rust\na    b\n```");
    });
}

#[gpui::test]
async fn captured_tab_key_inserts_visible_indent_in_paragraph(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "ab".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
    });
    redraw(cx);

    let event = KeyDownEvent {
        keystroke: Keystroke::parse("tab").expect("valid tab keystroke"),
        is_held: false,
    };
    editor.update_in(cx, |editor, window, cx| {
        editor.on_editor_key_down_capture(&event, window, cx);
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(block.read(cx).display_text(), "a    b");
    });
}

#[gpui::test]
async fn down_from_code_content_focuses_language_input(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    // Settle focus on the code content first (and clear any pending focus that a
    // later redraw would otherwise re-apply and steal back).
    editor.update_in(cx, |editor, _window, _cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
    });
    redraw(cx);

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
            block.on_focus_next(&FocusNext, window, block_cx);
        });
        assert!(
            block.read(cx).code_language_focus_handle.is_focused(window),
            "Down from the last code line should focus the language field"
        );
    });
}

#[gpui::test]
async fn down_from_code_language_at_document_end_creates_trailing_paragraph(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.code_language_focus_handle.focus(window);
            block.on_code_language_focus_next(&FocusNext, window, block_cx);
        });
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        assert_eq!(roots.len(), 2, "a trailing paragraph should be created");
        assert_eq!(roots[1].read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(roots[1].read(cx).display_text(), "");
    });
}

#[gpui::test]
async fn enter_in_code_language_does_not_exit_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.code_language_focus_handle.focus(window);
            block.on_code_language_newline(&Newline, window, block_cx);
        });
    });
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        // Enter must not leave the block, so no trailing paragraph appears.
        assert_eq!(editor.document.root_count(), 1);
    });
}

#[gpui::test]
async fn captured_tab_key_does_not_modify_code_language_input(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "```rust\nab\n```".to_string(), None)
    });

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(1, block_cx);
        });
        block.update(cx, |block, _cx| {
            block.code_language_focus_handle.focus(window);
        });
    });
    redraw(cx);

    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _cx| {
            block.code_language_focus_handle.focus(window);
        });
    });

    let event = KeyDownEvent {
        keystroke: Keystroke::parse("tab").expect("valid tab keystroke"),
        is_held: false,
    };
    editor.update_in(cx, |editor, window, cx| {
        editor.on_editor_key_down_capture(&event, window, cx);
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        let block = block.read(cx);
        assert_eq!(block.code_language_text(), "rust");
        assert_eq!(block.display_text(), "ab");
    });
}

#[gpui::test]
async fn tab_key_keeps_list_indent_semantics(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "- a\n- b".to_string(), None));

    editor.update(cx, |editor, cx| {
        let second = editor.document.visible_blocks()[1].entity.clone();
        editor.focus_block(second.entity_id());
        second.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("tab");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[1].entity.read(cx).render_depth, 1);
        assert_eq!(editor.document.markdown_text(cx), "- a\n  - b");
    });
}

#[gpui::test]
async fn tab_key_keeps_table_cell_navigation(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    let second_cell_id = editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .clone();
        let first = runtime.rows[0][0].clone();
        let second = runtime.rows[0][1].clone();
        editor.focus_block(first.entity_id());
        first.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
        second.entity_id()
    });
    redraw(cx);

    cx.simulate_keystrokes("tab");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert_eq!(editor.active_entity_id, Some(second_cell_id));
    });
}

#[gpui::test]
async fn right_arrow_at_cell_end_moves_to_next_cell(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    let second_cell_id = editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .clone();
        let first = runtime.rows[0][0].clone();
        let second = runtime.rows[0][1].clone();
        editor.focus_block(first.entity_id());
        first.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
        second.entity_id()
    });
    redraw(cx);

    cx.simulate_keystrokes("right");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert_eq!(editor.active_entity_id, Some(second_cell_id));
    });
}

#[gpui::test]
async fn left_arrow_at_cell_start_moves_to_previous_cell(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    let first_cell_id = editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .clone();
        let first = runtime.rows[0][0].clone();
        let second = runtime.rows[0][1].clone();
        editor.focus_block(second.entity_id());
        second.update(cx, |block, block_cx| {
            block.move_to(0, block_cx);
        });
        first.entity_id()
    });
    redraw(cx);

    cx.simulate_keystrokes("left");
    redraw(cx);

    editor.update(cx, |editor, _cx| {
        assert_eq!(editor.active_entity_id, Some(first_cell_id));
    });
}

#[gpui::test]
async fn inserting_table_at_document_end_adds_trailing_paragraph(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.table_insert_dialog = Some(super::context_menu::TableInsertDialogState {
                target: super::context_menu::TableInsertTarget::Append,
                body_rows: 2,
                columns: 2,
            });
            editor.on_confirm_table_insert_dialog(&ClickEvent::default(), window, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        let roots = editor.document.visible_blocks();
        let kinds = roots
            .iter()
            .map(|visible| visible.entity.read(cx).kind())
            .collect::<Vec<_>>();
        let table_index = kinds
            .iter()
            .position(|kind| *kind == BlockKind::Table)
            .expect("table inserted");
        // The table is the last meaningful block, so an empty paragraph is
        // appended after it to give the caret somewhere to land.
        assert_eq!(kinds.get(table_index + 1), Some(&BlockKind::Paragraph));
        assert_eq!(table_index + 1, kinds.len() - 1);
        assert_eq!(roots[table_index + 1].entity.read(cx).display_text(), "");
    });
}

#[gpui::test]
async fn ctrl_enter_exits_focused_math_block(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "$$n^2$$".to_string(), None));

    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
        block.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-enter");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::MathBlock);
        assert_eq!(visible[0].entity.read(cx).display_text(), "$$n^2$$");
        assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(visible[1].entity.read(cx).display_text(), "");
        assert_eq!(editor.document.markdown_text(cx), "$$n^2$$\n\n");
    });
}

#[gpui::test]
async fn ctrl_enter_exits_focused_table_cell(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = ["| A | B |", "| --- | --- |", "| 1 | 2 |"].join("\n");
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let cell = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .rows[0][0]
            .clone();
        editor.focus_block(cell.entity_id());
        cell.update(cx, |block, block_cx| {
            block.move_to(block.visible_len(), block_cx);
        });
    });
    redraw(cx);

    cx.simulate_keystrokes("ctrl-enter");
    redraw(cx);

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].entity.read(cx).kind(), BlockKind::Table);
        assert_eq!(visible[1].entity.read(cx).kind(), BlockKind::Paragraph);
        assert_eq!(visible[1].entity.read(cx).display_text(), "");
        assert_eq!(editor.active_entity_id, Some(visible[1].entity.entity_id()));
    });
}

#[gpui::test]
async fn ending_editor_pointer_selection_sessions_keeps_normal_selection(cx: &mut TestAppContext) {
    let editor =
        cx.new(|cx| Editor::from_markdown(cx, "```rust\nfn main() {}\n```".to_string(), None));

    editor.update(cx, |editor, cx| {
        let target = editor.document.visible_blocks()[0].entity.clone();
        target.update(cx, |block, _cx| {
            block.selected_range = 3..7;
            block.marked_range = Some(4..6);
            block.is_selecting = true;
        });
        editor.active_entity_id = Some(target.entity_id());

        assert!(editor.end_block_pointer_selection_sessions(cx));
        target.read_with(cx, |block, _cx| {
            assert!(!block.is_selecting);
            assert_eq!(block.selected_range, 3..7);
            assert_eq!(block.marked_range, Some(4..6));
        });

        assert!(!editor.end_block_pointer_selection_sessions(cx));
    });
}

#[gpui::test]
async fn toggle_view_mode_preserves_table_cell_position(cx: &mut TestAppContext) {
    let markdown = ["| Name | Value |", "| --- | --- |", "| alpha | beta |"].join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let cell = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .rows[0][1]
            .clone();
        cell.update(cx, |block, _cx| {
            block.selected_range = 2..2;
        });
        editor.active_entity_id = Some(cell.entity_id());

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
        let restored_table = editor
            .document
            .first_root()
            .expect("restored table")
            .clone();
        let restored_cell = restored_table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("restored runtime")
            .rows[0][1]
            .clone();
        assert_eq!(restored_cell.read(cx).display_text(), "beta");
        assert_eq!(restored_cell.read(cx).selected_range, 2..2);
        assert_eq!(editor.pending_focus, Some(restored_cell.entity_id()));
    });
}

#[gpui::test]
async fn toggle_view_mode_preserves_callout_table_cell_position(cx: &mut TestAppContext) {
    let markdown = [
        "> [!NOTE]",
        "> | Name | Value |",
        "> | --- | --- |",
        "> | alpha | beta |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        let callout = editor.document.first_root().expect("callout root").clone();
        let table = callout
            .read(cx)
            .children
            .iter()
            .find(|child| child.read(cx).kind() == BlockKind::Table)
            .expect("nested table child")
            .clone();
        let cell = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime")
            .rows[0][1]
            .clone();
        cell.update(cx, |block, _cx| {
            block.selected_range = 2..2;
        });
        editor.active_entity_id = Some(cell.entity_id());

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));

        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
        let restored_callout = editor
            .document
            .first_root()
            .expect("restored callout")
            .clone();
        let restored_table = restored_callout
            .read(cx)
            .children
            .iter()
            .find(|child| child.read(cx).kind() == BlockKind::Table)
            .expect("restored nested table")
            .clone();
        let restored_cell = restored_table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("restored runtime")
            .rows[0][1]
            .clone();
        assert_eq!(restored_cell.read(cx).display_text(), "beta");
        assert_eq!(restored_cell.read(cx).selected_range, 2..2);
        assert_eq!(editor.pending_focus, Some(restored_cell.entity_id()));
    });
}

#[gpui::test]
async fn welcome_page_renders_and_dismisses_into_a_new_document(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.show_welcome = true;
        cx.notify();
    });
    // The welcome overlay must render without panicking.
    cx.update(|window, cx| {
        window.draw(cx).clear();
    });

    // 新建文档 dismisses the page and leaves an editable empty document.
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.on_welcome_new_document(&gpui::ClickEvent::default(), window, cx);
        });
        window.draw(cx).clear();
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_welcome);
    });
}

#[gpui::test]
async fn welcome_page_hides_once_a_document_opens(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-welcome-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let doc = root.join("welcome-sample.md");
    std::fs::write(&doc, "# Welcome\n\nBody text.\n").expect("write doc");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.show_welcome = true;
        editor.set_workspace_root(root.clone(), cx);
        cx.notify();
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(doc.clone(), window, cx);
        });
        window.draw(cx).clear();
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.show_welcome);
        assert_eq!(editor.file_path.as_deref(), Some(doc.as_path()));
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn status_bar_breadcrumb_renders_without_panicking(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-crumb-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let doc = root.join("nested").join("note.md");
    std::fs::create_dir_all(doc.parent().unwrap()).expect("create nested");
    std::fs::write(&doc, "# note\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.open_workspace_file(doc.clone(), window, cx);
        });
    });
    // 渲染含面包屑的状态栏不应 panic。
    cx.update(|window, cx| {
        window.draw(cx).clear();
    });
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path.as_deref(), Some(doc.as_path()));
        // set_workspace_root canonicalizes, so compare against the
        // canonicalized root (/var ↔ /private/var on macOS).
        let canonical = std::fs::canonicalize(&root).expect("canonicalize");
        assert_eq!(editor.workspace_root_path(), Some(canonical.as_path()));
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn large_document_opens_within_budget(cx: &mut TestAppContext) {
    // roadmap G8: a 10 MiB document opens without waiting for every block. The
    // first chunk is imported up front (the 3 s budget covers exactly that) and
    // the rest streams in while the window stays interactive. Fixtures are
    // generated by scripts/generate-fixtures.mjs and gitignored, so skip when
    // absent.
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/perf/ten-mib.md");
    if !fixture.is_file() {
        eprintln!("skipping: generate fixtures with `node scripts/generate-fixtures.mjs tests/fixtures/perf`");
        return;
    }
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
    });
    let markdown = std::fs::read_to_string(&fixture).expect("read fixture");
    let bytes = markdown.len();
    assert!(bytes >= 10 * 1024 * 1024, "fixture should be ~10 MiB");

    let start = std::time::Instant::now();
    let editor = cx.update(|cx| {
        cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None))
    });
    let open_elapsed = start.elapsed();
    eprintln!("G8: 10 MiB open(first chunk): {open_elapsed:?}");

    let (first_blocks, pending, source_len) = cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            (
                editor.document.visible_blocks().len(),
                editor.document.pending_tail().is_some(),
                editor.serialized_document_text(cx).len(),
            )
        })
    });
    // 打开即只建首块：首屏可交互，其余在后台续建（roadmap G8）。
    assert!(first_blocks > 0);
    assert!(pending, "超大文档应先只建首块，其余挂起");
    // 未建完也要能序列化全文：保存/导出/自动恢复不得丢尾段。
    assert!(source_len >= 9 * 1024 * 1024, "未建完时序列化丢内容: {source_len}");
    // G8 预算的形状判据（并发跑测时绝对墙钟不可靠，单机实测见 eprintln）：
    // 打开只建首块，且打开耗时远小于整篇建块成本。
    assert!(
        first_blocks <= 4_000,
        "打开时应只建首块（上限 4000），实测 {first_blocks} 块"
    );

    let deadline = Instant::now() + Duration::from_secs(180);
    while cx.read(|cx| editor.read_with(cx, |editor, _cx| editor.document.pending_tail().is_some()))
    {
        assert!(Instant::now() < deadline, "续建未在预算时间内完成");
        cx.run_until_parked();
    }

    let (blocks, text_len, text) = cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            (
                editor.document.visible_blocks().len(),
                editor.serialized_document_text(cx).len(),
                editor.serialized_document_text(cx),
            )
        })
    });
    let total_elapsed = start.elapsed();
    eprintln!("G8: 续建完成 {total_elapsed:?}，{blocks} 块，文本 {text_len} 字节");
    assert!(
        blocks > first_blocks * 10,
        "续建后块数未增长: {first_blocks} -> {blocks}"
    );
    // 打开只付首块的钱：打开耗时必须显著小于整篇建块成本（相对判据在并发跑测下也稳）。
    assert!(
        open_elapsed * 5 < total_elapsed,
        "打开 {open_elapsed:?} 与整篇建块 {total_elapsed:?} 不成比例：打开可能又付了整篇的钱"
    );
    assert!(text_len >= 9 * 1024 * 1024, "续建后文本仍不完整: {text_len}");

    // 分块导入必须与一次整篇导入等价：逐字节比较。
    let single_pass = cx.update(|cx| {
        cx.new(|cx| {
            Editor::from_markdown_with_chunk_budget(cx, markdown.clone(), None, usize::MAX)
        })
    });
    let expected = cx.read(|cx| {
        single_pass.read_with(cx, |editor, cx| {
            assert!(editor.document.pending_tail().is_none());
            editor.serialized_document_text(cx)
        })
    });
    assert_eq!(text, expected, "分块导入与整篇导入结果不一致");

    // 防回归：首块 + 续建的总成本仍应在每块预算内。
    let per_block_us = total_elapsed.as_micros() as f64 / blocks.max(1) as f64;
    eprintln!("G8: {per_block_us:.1} µs/block（首块 + 续建，debug）");
    // 400 µs tolerates parallel-test CPU contention while still catching
    // catastrophic regressions (a 2x+ per-block slowdown).
    assert!(
        per_block_us <= 400.0,
        "per-block open cost regressed: {per_block_us:.1} µs > 400 µs budget"
    );
}

/// roadmap G8：分块导入的任务边界必须与一次整篇导入逐字节一致。
///
/// 小预算把一份小文档切成很多块，覆盖空白行串、列表/段落紧邻、定界符、
/// 表格、围栏、公式、前导 frontmatter 等边界。
#[gpui::test]
async fn progressive_import_matches_single_pass_import(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = progressive_import_fixture();

    let baseline = progressive_test_editor(cx, markdown.clone(), usize::MAX);
    let (expected_text, expected_blocks, expected_raw) = cx.read(|cx| {
        baseline.read_with(cx, |editor, cx| {
            assert!(editor.document.pending_tail().is_none());
            (
                editor.document.markdown_text(cx),
                editor.document.visible_blocks().len(),
                editor.document.raw_source_text(cx),
            )
        })
    });

    for budget in [1, 2, 3, 5, 8] {
        let editor = progressive_test_editor(cx, markdown.clone(), budget);
        // 未建完时文本就已完整到可保存：已建块是序列化结果，尾段是逐行原文，
        // 重新导入这份文本必须得到与整篇导入相同的文档（保存不丢内容）。
        let mid_stream = cx.read(|cx| {
            editor.read_with(cx, |editor, cx| {
                assert!(editor.document.pending_tail().is_some());
                editor.document.markdown_text(cx)
            })
        });
        assert!(mid_stream.contains("末尾段落"), "预算 {budget}：尾段内容缺失");
        let reparsed = progressive_test_editor(cx, mid_stream, usize::MAX);
        let reparsed_text = cx.read(|cx| {
            reparsed.read_with(cx, |editor, cx| editor.document.markdown_text(cx))
        });
        assert_eq!(
            reparsed_text, expected_text,
            "预算 {budget}：未建完的文本重新导入后与整篇导入不一致"
        );

        let deadline = Instant::now() + Duration::from_secs(30);
        while cx
            .read(|cx| editor.read_with(cx, |editor, _cx| editor.document.pending_tail().is_some()))
        {
            assert!(Instant::now() < deadline, "预算 {budget}：续建未完成");
            cx.run_until_parked();
        }

        cx.read(|cx| {
            editor.read_with(cx, |editor, cx| {
                assert_eq!(
                    editor.document.visible_blocks().len(),
                    expected_blocks,
                    "预算 {budget}：块数与整篇导入不一致"
                );
                assert_eq!(
                    editor.document.markdown_text(cx),
                    expected_text,
                    "预算 {budget}：建完后文本与整篇导入不一致"
                );
                assert_eq!(
                    editor.document.raw_source_text(cx),
                    expected_raw,
                    "预算 {budget}：原文视图与整篇导入不一致"
                );
            })
        });
    }
}

/// roadmap G8：结构编辑（真实的块插入入口）必须先补建完剩余块再改树。
#[gpui::test]
async fn structural_edit_flushes_the_pending_import(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = progressive_import_fixture();
    let baseline = progressive_test_editor(cx, markdown.clone(), usize::MAX);
    let baseline_count = cx.read(|cx| {
        baseline.read_with(cx, |editor, _cx| editor.document.visible_blocks().len())
    });

    let editor = progressive_test_editor(cx, markdown.clone(), 2);
    cx.read(|cx| {
        editor.read_with(cx, |editor, _cx| {
            assert!(editor.document.pending_tail().is_some());
        })
    });

    cx.update(|cx| {
        editor.update(cx, |editor, cx| {
            let block = Editor::new_block(cx, BlockRecord::paragraph("新插入的块"));
            editor.document.insert_blocks_at(None, 0, vec![block], cx);
        });
    });

    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            let text = editor.document.markdown_text(cx);
            assert!(
                editor.document.pending_tail().is_none(),
                "结构编辑后不应再有挂起的尾段"
            );
            assert!(text.contains("新插入的块"));
            assert!(text.contains("末尾段落"), "尾段内容在结构编辑后丢失");
            assert_eq!(
                editor.document.visible_blocks().len(),
                baseline_count + 1,
                "补建 + 插入后的块数不符"
            );
        })
    });
}

/// roadmap G8：超大文档续建完成后，保存到磁盘的仍是完整文本。
#[gpui::test]
async fn streamed_document_saves_complete_text_to_disk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = progressive_import_fixture();
    let path = temp_markdown_path("progressive-streamed-save");
    fs::write(&path, &markdown).expect("write fixture");
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view({
        let path = path.clone();
        move |_window, cx| {
            Editor::from_markdown_with_chunk_budget(cx, markdown.clone(), Some(path), 2)
        }
    });
    redraw(cx);
    cx.read(|cx| {
        editor.read_with(cx, |editor, _cx| {
            assert!(
                editor.document.pending_tail().is_none(),
                "窗口打开后续建任务应已跑完"
            );
        })
    });

    cx.simulate_input(" x");
    redraw(cx);
    let expected = editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        editor.document.markdown_text(cx)
    });
    assert!(expected.contains("末尾段落"), "续建后的文档缺少尾段");

    cx.dispatch_action(SaveDocument);
    redraw(cx);
    assert_eq!(
        fs::read_to_string(&path).expect("read saved markdown"),
        expected
    );
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.document_dirty);
    });
}

fn progressive_test_editor(
    cx: &mut TestAppContext,
    markdown: String,
    chunk_budget: usize,
) -> gpui::Entity<Editor> {
    cx.update(|cx| {
        cx.new(|cx| {
            Editor::from_markdown_with_chunk_budget(cx, markdown.clone(), None, chunk_budget)
        })
    })
}

/// 一份刻意包含各种任务边界的短文档：frontmatter、懒惰续行、未闭合反引号、
/// 空行串、列表与段落紧邻、有序列表、引用、围栏、表格、定界符、缩进代码、
/// 公式、任务项、结尾无换行。
fn progressive_import_fixture() -> String {
    [
        "---",
        "title: 边界",
        "---",
        "",
        "开头段落",
        "紧跟的第二行",
        "",
        "# 标题",
        "正文 `未闭合的反引号",
        "",
        "后面的行补上闭合 `",
        "",
        "- 一",
        "- 二",
        "  - 嵌套项",
        "",
        "   ",
        "",
        "1. 甲",
        "2. 乙",
        "",
        "> 引用",
        "> 续行",
        "",
        "```rust",
        "fn main() {}",
        "```",
        "",
        "| a | b |",
        "| --- | --- |",
        "| 1 | 2 |",
        "表格后的段落",
        "",
        "标题二",
        "===",
        "",
        "段落紧邻列表",
        "- 紧邻项",
        "",
        "---",
        "中段分隔线后的内容",
        "---",
        "",
        "",
        "",
        "   缩进代码",
        "",
        "$$",
        "x^2",
        "$$",
        "",
        "- [ ] 任务",
        "",
        "1. 续号甲",
        "2. 续号乙",
        "",
        "末尾段落",
    ]
    .join("\n")
}

#[gpui::test]
async fn wikilink_creates_missing_file_at_workspace_root(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-wikilink-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    let created = root.join("fresh note.md");
    let created_for_assert = created.clone();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_wikilink("fresh note".into(), window, cx);
        });
        window.draw(cx).clear();
    });
    // The open path is canonicalized by set_workspace_root, so compare
    // against the canonical form of the created file.
    let canonical_created = std::fs::canonicalize(&created_for_assert)
        .expect("canonicalize created");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path, Some(canonical_created.clone()));
    });
    assert!(created.is_file(), "wikilink target should be created");
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn wikilink_opens_existing_workspace_file(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-wikilink-open-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let existing = root.join("target.md");
    std::fs::write(&existing, "# target\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_wikilink("target".into(), window, cx);
        });
        window.draw(cx).clear();
    });
    let canonical_existing = std::fs::canonicalize(&existing).expect("canonicalize existing");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.file_path.as_deref(), Some(canonical_existing.as_path()));
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn crash_recovery_drill_snapshot_restore_save(cx: &mut TestAppContext) {
    // roadmap G6：「写快照 → 崩溃 → 恢复 → 保存」全链路演练。
    init_editor_test_app(cx);

    let source_path = temp_markdown_path("crash-drill");
    fs::write(&source_path, "saved before crash").expect("seed file");
    let cleanup_source = source_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_source);
    });

    // Phase 1 — 编辑后写入恢复快照（与自动保存使用同一持久化函数）。
    let (editor, cx) = cx.add_window_view({
        let path = source_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "saved before crash".into(), Some(path))
    });
    let recovery_id = editor.read_with(cx, |editor, _cx| editor.recovery_id);
    let unsaved = "edited after save, not yet on disk";
    editor.update(cx, |editor, cx| {
        let block = editor.document.first_root().expect("block").clone();
        block.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain(unsaved.to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    crate::config::save_recovery_snapshot(&crate::config::RecoverySnapshot {
        id: recovery_id,
        source_path: Some(source_path.clone()),
        markdown: unsaved.into(),
    })
    .expect("write snapshot");
    assert!(crate::config::read_recovery_snapshots()
        .expect("read snapshots")
        .iter()
        .any(|snapshot| snapshot.id == recovery_id));

    // Phase 2 — 崩溃：直接丢弃编辑器实体（不走任何关闭流程）。磁盘文件
    // 仍是旧内容，只有恢复快照里有未保存编辑。
    drop(editor);
    assert_eq!(
        fs::read_to_string(&source_path).expect("read disk"),
        "saved before crash"
    );

    // Phase 3 — 恢复：from_recovery 打开未保存内容（dirty 副本）。
    let (editor, cx) = cx.add_window_view({
        let snapshot_path = source_path.clone();
        move |_window, cx| {
            Editor::from_recovery(
                cx,
                crate::config::RecoverySnapshot {
                    id: recovery_id,
                    source_path: Some(snapshot_path),
                    markdown: unsaved.into(),
                },
            )
        }
    });
    editor.read_with(cx, |editor, cx| {
        assert!(editor.is_recovered_document);
        assert_eq!(editor.document.markdown_text(cx), unsaved);
    });

    // Phase 4 — 保存：磁盘内容更新，恢复快照被清理。
    // 真实应用中恢复文档经对话框另存到源路径；演练直接调用同一保存内核。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.save_to_existing_path(&source_path, window, cx));
        });
    });
    assert_eq!(
        fs::read_to_string(&source_path).expect("read after save"),
        unsaved
    );
    assert!(!crate::config::read_recovery_snapshots()
        .expect("read snapshots")
        .iter()
        .any(|snapshot| snapshot.id == recovery_id));
}

#[gpui::test]
async fn render_structure_snapshot_for_key_blocks(cx: &mut TestAppContext) {
    // roadmap G7：关键块渲染结构的黄金快照。任何解析/渲染回归改动若
    // 改变块序列或文本，需同步更新此快照并在 PR 中说明。
    let source = "# Title\n\nBody with **bold**, `code` and [link](https://x).\n\n- one\n- two\n\n- [ ] task\n\n> quoted\n\n```rust\nlet x = 1;\n```\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        let snapshot: Vec<(String, String)> = visible
            .iter()
            .map(|visible| {
                let block = visible.entity.read(cx);
                (
                    format!("{:?}", block.kind()),
                    block.display_text().to_string(),
                )
            })
            .collect();

        let expected = vec![
            ("Heading { level: 1 }".to_string(), "Title".to_string()),
            (
                "Paragraph".to_string(),
                "Body with bold, code and link.".to_string(),
            ),
            (
                "BulletedListItem".to_string(),
                "one".to_string(),
            ),
            (
                "BulletedListItem".to_string(),
                "two".to_string(),
            ),
            // 列表组与下一块之间的空段落分隔（设计使然）。
            ("Paragraph".to_string(), String::new()),
            (
                "TaskListItem { checked: false }".to_string(),
                "task".to_string(),
            ),
            ("Quote".to_string(), "quoted".to_string()),
            (
                "CodeBlock { language: Some(\"rust\") }".to_string(),
                "let x = 1;".to_string(),
            ),
            // 表格内容由 table runtime 渲染，display_text 为空（设计使然）。
            ("Table".to_string(), String::new()),        ];
        assert_eq!(snapshot, expected, "render structure snapshot mismatch");
    });
}

#[gpui::test]
async fn heading_fold_hides_section_content(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "## Section\n\nalpha\n\nbeta\n\n## Next\n\ngamma";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 5); // H, alpha, beta, H2, gamma
        let heading = visible[0].entity.clone();
        heading.update(cx, |block, _cx| block.folded = true);

        let filtered = editor
            .apply_heading_fold_filter(
                editor.document.visible_blocks().to_vec(),
                cx,
            )
            .iter()
            .map(|visible| visible.entity.read(cx).display_text().to_string())
            .collect::<Vec<_>>();

        // 折叠章节内容 alpha/beta 被隐藏，下一同级标题保持可见。
        assert_eq!(
            filtered,
            vec!["Section".to_string(), "Next".to_string(), "gamma".to_string()]
        );
    });
}

#[gpui::test]
async fn heading_fold_chevron_marks_only_foldable_headings(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "## Section\n\nalpha\n\n### Child\n\nbeta\n\n## Empty\n\n## Next\n\ngamma";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    editor.update(cx, |editor, cx| {
        editor.apply_heading_fold_filter(editor.document.visible_blocks().to_vec(), cx);
        let visible = editor.document.visible_blocks().to_vec();
        let foldable = visible
            .iter()
            .filter(|visible| matches!(visible.entity.read(cx).kind(), BlockKind::Heading { .. }))
            .map(|visible| visible.entity.read(cx).foldable)
            .collect::<Vec<_>>();
        // Section / Child / Next 后方有章节内容；Empty 紧跟同级标题，没有可折叠内容。
        assert_eq!(foldable, vec![true, true, false, true]);
    });
}

#[gpui::test]
async fn broken_image_placeholder_stays_compact_inside_the_column(cx: &mut TestAppContext) {
    // 用户报修：callout 列表项里的图片读不出来时，占位框按「视口估算宽度」画成
    // 一条横穿整屏的空心条，冲出 callout 右边界。占位框现在贴着文字收紧，
    // 上限只到所在列的可用宽度。
    init_editor_test_app(cx);
    let markdown = "> [!IMPORTANT] 混合块\n>\n> - bold\n> - ![image](missing-image.png)\n";
    let (_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    // 宽窗口：旧实现按视口估算出 definite 宽度，占位框会拉成一条空心长条。
    cx.update(|window, _cx| window.resize(gpui::size(px(1400.0), px(900.0))));
    redraw(cx);

    let viewport_width = cx.update(|window, _cx| window.viewport_size().width);
    let bounds = cx
        .debug_bounds("image-placeholder")
        .expect("broken image should render a placeholder box");
    assert!(
        bounds.size.width <= px(400.0),
        "占位框应贴着文字收紧，实测宽度 {:?}",
        bounds.size.width
    );
    assert!(
        bounds.right() < viewport_width,
        "占位框右边 {:?} 不应超出视口宽度 {viewport_width:?}",
        bounds.right()
    );
}

#[gpui::test]
async fn many_tabs_never_slide_under_the_window_controls(cx: &mut TestAppContext) {
    // 用户报修：标签开多了，第一个标签一直在左移，最后压到红绿灯下面。
    // 根因是标题行的红绿灯预留位（和应用绘制的窗口按钮）没有 flex_shrink_0，
    // 整行溢出时被 taffy 挤扁。
    init_editor_test_app(cx);
    let root = temp_markdown_path("tab-strip-overflow");
    fs::create_dir_all(&root).unwrap();
    let mut paths = Vec::new();
    for index in 0..12 {
        let path = root.join(format!("doc-with-a-much-longer-name-{index:02}.md"));
        fs::write(&path, format!("# doc {index}\n")).unwrap();
        paths.push(path);
    }
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, _cx| window.resize(gpui::size(px(1615.0), px(900.0))));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
        });
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(paths[0].clone(), window, cx);
        });
    });
    redraw(cx);
    let first_with_one_tab = cx
        .debug_bounds("document-tab-0")
        .expect("单个标签应渲染在标题行里")
        .origin
        .x;

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            for path in &paths[1..] {
                editor.open_workspace_file(path.clone(), window, cx);
            }
        });
    });
    redraw(cx);
    let first_with_many = cx.debug_bounds("document-tab-0").expect("首标签仍应存在");
    assert_eq!(
        first_with_many.origin.x, first_with_one_tab,
        "加满标签后第一个标签不该左移（{first_with_one_tab} → {:?}）",
        first_with_many.origin.x
    );
    assert!(
        first_with_many.origin.x >= px(84.0),
        "第一个标签不该进入 macOS 红绿灯预留区（84px），实测 {:?}",
        first_with_many.origin.x
    );
    let max_offset = editor.read_with(cx, |editor, _| {
        editor.workspace.tabs_scroll_handle.max_offset()
    });
    assert!(
        max_offset.width > px(0.0),
        "溢出的标签条应该可以横向滚动，实测 max_offset {max_offset:?}"
    );
}

#[gpui::test]
async fn in_app_modal_buttons_close_it_and_run_the_callback(cx: &mut TestAppContext) {
    // 用户要求：全软件不用系统原生弹窗。模态必须可点、可关、回调拿到正确序号。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# 标题\n".into(), None));
    editor.update(cx, |editor, cx| {
        editor.show_modal(
            crate::editor::modal::ModalSpec {
                title: "确认删除".into(),
                detail: Some("note.md".into()),
                buttons: vec!["删除".into(), "取消".into()],
                default_index: 0,
                cancel_index: 1,
            },
            |choice, editor, _window, cx| {
                if choice == 0 {
                    // 用一个只有单个按钮的模态标记「回调确实按 choice 跑了」。
                    editor.show_message_modal("已删除", "", cx);
                }
            },
            cx,
        );
    });
    redraw(cx);
    assert!(cx.debug_bounds("editor-modal-button-0").is_some(), "模态应渲染第一个按钮");
    assert!(cx.debug_bounds("editor-modal-button-1").is_some(), "模态应渲染取消按钮");

    let first = cx.debug_bounds("editor-modal-button-0").expect("button 0");
    cx.simulate_click(first.center(), Modifiers::none());
    redraw(cx);
    assert!(
        cx.debug_bounds("editor-modal-button-1").is_none(),
        "点「删除」后旧模态应关闭，回调里新开的模态只有一个按钮"
    );
    assert!(cx.debug_bounds("editor-modal-button-0").is_some());

    let second = cx.debug_bounds("editor-modal-button-0").expect("button of second modal");
    cx.simulate_click(second.center(), Modifiers::none());
    redraw(cx);
    assert!(cx.debug_bounds("editor-modal-button-0").is_none());
    editor.read_with(cx, |editor, _| assert!(!editor.modal_is_open()));
}

#[gpui::test]
async fn in_app_modal_backdrop_click_cancels(cx: &mut TestAppContext) {
    // 取消语义：点遮罩 = 按取消位（键盘 Esc/Enter 另计，见 roadmap 后续项）。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# 标题\n".into(), None));
    editor.update(cx, |editor, cx| {
        editor.show_modal(
            crate::editor::modal::ModalSpec {
                title: "确认".into(),
                detail: None,
                buttons: vec!["确定".into(), "取消".into()],
                default_index: 0,
                cancel_index: 1,
            },
            |choice, editor, _window, cx| {
                if choice != 1 {
                    editor.show_message_modal("不该发生", "", cx);
                }
            },
            cx,
        );
    });
    redraw(cx);
    assert!(cx.debug_bounds("editor-modal-button-1").is_some());

    // 遮罩左上角（面板之外）按下 = 取消。
    cx.simulate_click(gpui::point(px(6.0), px(6.0)), Modifiers::none());
    redraw(cx);
    assert!(
        cx.debug_bounds("editor-modal-button-0").is_none(),
        "点遮罩应关闭模态，且回调按取消位走（不再开新模态）"
    );
    editor.read_with(cx, |editor, _| assert!(!editor.modal_is_open()));
}

#[gpui::test]
async fn app_source_never_uses_native_prompts(_cx: &mut TestAppContext) {
    // 用户要求：整个软件禁止系统原生弹窗。这条守卫挡住以后新增 `window.prompt`。
    let mut offenders = Vec::new();
    collect_prompt_offenders(std::path::Path::new("src"), &mut offenders);
    assert!(
        offenders.is_empty(),
        "src/ 里不应再出现系统原生弹窗调用 window.prompt：{offenders:?}"
    );
}

fn collect_prompt_offenders(dir: &std::path::Path, offenders: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("source dir should be readable") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_prompt_offenders(&path, offenders);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("source file should be readable");
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            // `prompt_for_*` 是原生文件/目录选择器，不属于消息框。
            if line.contains(".prompt(") && !line.contains("prompt_for_") {
                offenders.push(format!("{}:{}", path.display(), index + 1));
            }
        }
    }
}

#[gpui::test]
async fn heading_fold_chevron_renders_and_click_toggles_fold(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "## Section\n\nalpha\n\n## Empty";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, markdown.into(), None)
    });
    redraw(cx);

    // 含章节内容的标题在左侧留白渲染 chevron，空章节标题不渲染。
    let bounds = cx
        .debug_bounds("heading-fold-chevron")
        .expect("foldable heading should render a fold chevron");
    assert!(bounds.size.width > px(0.0) && bounds.size.height > px(0.0));

    let (heading, empty_heading) = editor.update(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().to_vec();
        assert_eq!(visible.len(), 3); // Section, alpha, Empty
        (visible[0].entity.clone(), visible[2].entity.clone())
    });
    assert!(heading.read_with(cx, |block, _cx| block.foldable));
    assert!(!empty_heading.read_with(cx, |block, _cx| block.foldable));

    // 点击 chevron 中心：章节折叠，块级 mouse-down 不改变光标。
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);

    editor.update(cx, |editor, cx| {
        assert!(heading.read(cx).folded);
        let filtered = editor
            .apply_heading_fold_filter(editor.document.visible_blocks().to_vec(), cx)
            .iter()
            .map(|visible| visible.entity.read(cx).display_text().to_string())
            .collect::<Vec<_>>();
        assert_eq!(filtered, vec!["Section".to_string(), "Empty".to_string()]);
    });
}

#[gpui::test]
async fn workspace_tree_scan_is_async_and_applies_result(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-async-tree-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("nested")).expect("create nested");
    std::fs::write(root.join("nested").join("note.md"), "# note\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        // 扫描不再同步发生在调用栈内（roadmap D9）。
        assert!(editor.workspace_text_files().is_empty());
    });

    cx.run_until_parked();
    let expected =
        std::fs::canonicalize(root.join("nested").join("note.md")).expect("canonicalize");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected.clone()]);
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn workspace_tree_scan_discards_stale_root_results(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root_a = std::env::temp_dir().join(format!("velora-tree-a-{}", uuid::Uuid::new_v4()));
    let root_b = std::env::temp_dir().join(format!("velora-tree-b-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root_a).expect("create a");
    std::fs::create_dir_all(&root_b).expect("create b");
    std::fs::write(root_a.join("a.md"), "# a\n").expect("write a");
    std::fs::write(root_b.join("b.md"), "# b\n").expect("write b");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root_a.clone(), cx);
        // 第一次扫描尚未落地就切到新根：旧结果必须被丢弃。
        editor.set_workspace_root(root_b.clone(), cx);
    });

    cx.run_until_parked();
    let expected_b = std::fs::canonicalize(root_b.join("b.md")).expect("canonicalize");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected_b.clone()]);
    });
    let _ = std::fs::remove_dir_all(root_a);
    let _ = std::fs::remove_dir_all(root_b);
}

#[gpui::test]
async fn reopening_workspace_root_rescans_after_tree_is_cleared(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-tree-reopen-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("a.md"), "# a\n").expect("write");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();
    let expected = std::fs::canonicalize(root.join("a.md")).expect("canonicalize");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected.clone()]);
    });

    // 再次打开同一文件夹：旧树先清空，随后必须重新扫描出结果，
    // 不能因为"该根已扫描过"而卡在空树（roadmap D9 缓存标记）。
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        assert!(editor.workspace_text_files().is_empty());
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace_text_files(), vec![expected.clone()]);
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn heading_fold_chevron_toggle_hides_section_and_refocuses_heading(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = "## Section\n\nalpha\n\n## Next\n\ngamma";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    let (heading, paragraph) = editor.update(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().to_vec();
        (visible[0].entity.clone(), visible[1].entity.clone())
    });
    // 光标停留在章节内的 alpha 段落上。
    editor.update(cx, |editor, _cx| {
        editor.active_entity_id = Some(paragraph.entity_id());
    });

    // chevron 点击：块发出折叠请求，编辑器统一翻转折叠状态。
    heading.update(cx, |_block, cx| cx.emit(BlockEvent::RequestToggleFold));

    editor.update(cx, |editor, cx| {
        assert!(heading.read(cx).folded);
        // 折叠后光标所在段落被隐藏，焦点回到标题，避免输入静默丢失。
        assert_eq!(editor.pending_focus, Some(heading.entity_id()));
        assert_eq!(editor.active_entity_id, Some(heading.entity_id()));
        let filtered = editor
            .apply_heading_fold_filter(editor.document.visible_blocks().to_vec(), cx)
            .iter()
            .map(|visible| visible.entity.read(cx).display_text().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            filtered,
            vec!["Section".to_string(), "Next".to_string(), "gamma".to_string()]
        );
    });
}

/// 断言快捷切换器只剩一个结果，且文件名匹配（工作区根会被规范化成 /private 前缀）。
fn assert_quick_open_result(results: &[PathBuf], expected_name: &str) {
    assert_eq!(results.len(), 1, "expected one match, got {results:?}");
    assert_eq!(
        results[0].file_name().and_then(|name| name.to_str()),
        Some(expected_name)
    );
}

#[gpui::test]
async fn quick_open_accepts_ime_text_for_non_ascii_file_names(cx: &mut TestAppContext) {
    // roadmap E9：⌘P 输入接 EntityInputHandler，中文文件名可直接用输入法拼写。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-quick-open-ime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let notes = root.join("笔记.md");
    std::fs::write(&notes, "# 笔记\n").expect("write notes");
    std::fs::write(root.join("alpha.md"), "# Alpha\n").expect("write alpha");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.toggle_quick_open(window, cx)
    });
    redraw(cx);

    // 输入法提交路径：key_char → replace_text_in_range（同 macOS insertText）。
    cx.simulate_input("笔记");

    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "笔记");
        assert_eq!(state.selected_range, 6..6);
        assert_eq!(state.marked_range, None);
        assert_quick_open_result(&state.results, "笔记.md");
    });

    // 纯 ASCII 也必须只插入一次（输入处理器接管后不再走手动按键插入）。
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("alpha");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "alpha");
        assert_eq!(state.selected_range, 5..5);
        assert_quick_open_result(&state.results, "alpha.md");
    });

    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn quick_open_composition_commit_backspace_and_escape_edit_the_query(
    cx: &mut TestAppContext,
) {
    // roadmap E9：组合期标记由输入法接管，提交覆盖组合串；退格按字素删除。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-quick-open-ime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    let notes = root.join("笔记.md");
    std::fs::write(&notes, "# 笔记\n").expect("write notes");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.toggle_quick_open(window, cx)
    });
    redraw(cx);

    // 拼音组合中：marked_range 覆盖组合串，结果先按拼音过滤。
    editor.update_in(cx, |editor, window, cx| {
        editor.replace_and_mark_text_in_range(None, "biji", Some(0..4), window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "biji");
        assert_eq!(state.marked_range, Some(0..4));
    });

    // 提交：输入法用候选词覆盖组合串，并刷新结果。
    editor.update_in(cx, |editor, window, cx| {
        editor.replace_text_in_range(None, "笔记", window, cx);
    });
    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "笔记");
        assert_eq!(state.marked_range, None);
        assert_quick_open_result(&state.results, "笔记.md");
    });

    // 退格删掉整个汉字（按字素，而不是按字节）。
    cx.simulate_keystrokes("backspace");
    editor.read_with(cx, |editor, _cx| {
        let state = editor.quick_open.as_ref().expect("quick open stays open");
        assert_eq!(state.query, "笔");
        assert_eq!(state.selected_range, 3..3);
    });

    // escape 关闭面板并清空查询。
    cx.simulate_keystrokes("escape");
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.quick_open.is_none());
    });

    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn every_registered_command_has_a_handler(cx: &mut TestAppContext) {
    // roadmap H5：注册表里的每条命令都必须有处理者——菜单项与命令面板条目
    // 都经 Action 派发，没有处理者的命令会变成「点了没反应」（菜单里还会变灰）。
    // 这里用菜单启用判定 is_action_available 逐条把守。
    init_editor_test_app(cx);
    cx.update(|cx| crate::app_menu::init(cx));
    let (_editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, String::new(), None)
    });
    redraw(cx);

    let mut missing = Vec::new();
    for spec in crate::commands::commands() {
        let action = spec.boxed_action();
        if !cx.update(|_window, cx| cx.is_action_available(action.as_ref())) {
            missing.push(spec.id);
        }
    }

    assert!(missing.is_empty(), "以下命令没有处理者：{missing:?}");
}

#[gpui::test]
async fn tmp_debug_merge_state(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = chunk_boundary_source();
    let path = std::env::temp_dir().join(format!("velora-chunk-dbg-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");

    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    cx.run_until_parked();
    editor
        .read_with(cx, |editor, _cx| {
            for (index, visible) in editor.document.visible_blocks().iter().enumerate() {
                let text = visible.entity.read(_cx).display_text();
                println!("before block[{index}] id={:?} len={}", visible.entity.entity_id(), text.len());
            }
        })
        .expect("open");
    editor
        .update(cx, |editor, window, cx| {
            let blocks = editor.document.flatten_visible_blocks();
            let second = blocks[1].entity.clone();
            second.update(cx, |block, cx| {
                block.selected_range = 0..0;
                block.on_delete_back(&DeleteBack, window, cx);
            });
        })
        .expect("edit");
    editor
        .read_with(cx, |editor, _cx| {
            let blocks = editor.document.visible_blocks();
            println!("pre-park: {} blocks", blocks.len());
            for (index, visible) in blocks.iter().enumerate() {
                let text = visible.entity.read(_cx).display_text();
                println!("pre-park block[{index}] id={:?} len={}", visible.entity.entity_id(), text.len());
            }
        })
        .expect("read");
    cx.run_until_parked();
    editor
        .read_with(cx, |editor, _cx| {
            let blocks = editor.document.visible_blocks();
            println!("after merge: {} blocks", blocks.len());
            for (index, visible) in blocks.iter().enumerate() {
                let text = visible.entity.read(_cx).display_text();
                println!("block[{index}] id={:?} len={}", visible.entity.entity_id(), text.len());
            }
        })
        .expect("read");
}

#[gpui::test]
async fn typing_consecutive_backslashes_keeps_every_one(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    let backslashes = |count: usize| "\\".repeat(count);

    // 用户报修：渲染模式里连按反斜杠只能得到一个（`\\` 被当成“转义的反斜杠”塔缩）。
    for count in 1..=3 {
        cx.simulate_input("\\");
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            assert_eq!(
                block.read(cx).display_text(),
                format!("{}alpha", backslashes(count))
            );
        });
    }
    editor.read_with(cx, |editor, cx| {
        // 文件里每个可见反斜杠转义一次：3 个可见 -> 6 个字符
        assert_eq!(editor.document.markdown_text(cx), format!("{}alpha", backslashes(6)));
    });

    // 反斜杠不再吃掉后面的标记字符
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "seed".to_string(), None));
    cx.simulate_input("\\");
    cx.simulate_input("*");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(
            block.read(cx).display_text(),
            format!("{}*seed", backslashes(1))
        );
        assert_eq!(
            editor.document.markdown_text(cx),
            format!("{}*seed", backslashes(3))
        );
    });

    // 一次插入一段文本（粘贴）：UNC 路径的反斜杠必须保真
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "seed".to_string(), None));
    cx.simulate_input(&format!("{}server{}share", backslashes(2), backslashes(1)));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(
            block.read(cx).display_text(),
            format!("{}server{}shareseed", backslashes(2), backslashes(1))
        );
    });
}

#[gpui::test]
async fn typing_backslashes_in_link_blocks_does_not_multiply(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let count_backslashes = |value: &str| value.matches('\\').count();

    // 用户报修：行首是自动链接的块里按反斜杠，可见数量按「两倍加一」翻倍
    // （1 -> 3 -> 7）。这三类块都走 markdown 源直编路径。
    for source in [
        "<https://example.com> tail",
        "[a][b] tail",
        "[a](https://example.com) tail",
    ] {
        let (editor, cx) = cx.add_window_view({
            let source = source.to_string();
            move |_window, cx| Editor::from_markdown(cx, source, None)
        });
        cx.simulate_keystrokes("home");
        redraw(cx);
        for typed in 1..=3 {
            cx.simulate_input("\\");
            redraw(cx);
            editor.read_with(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                let screen = block.read(cx).display_text();
                let file = editor.document.markdown_text(cx);
                assert_eq!(
                    count_backslashes(&screen),
                    typed,
                    "{source:?} 输入 {typed} 次后可见反斜杠数量（屏幕 {screen:?}）"
                );
                assert_eq!(
                    count_backslashes(&file),
                    typed * 2,
                    "{source:?} 输入 {typed} 次后源文件反斜杠数量（文件 {file:?}）"
                );
            });
        }
    }

    // 光标贴在自动链接末尾（行尾）时同样不能把插入点落进 `<...>` 里。
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "tail <https://example.com>".to_string(), None)
    });
    cx.simulate_keystrokes("end");
    redraw(cx);
    for typed in 1..=2 {
        cx.simulate_input("\\");
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            let screen = block.read(cx).display_text();
            assert_eq!(
                count_backslashes(&screen),
                typed,
                "行尾输入 {typed} 次后可见反斜杠数量（屏幕 {screen:?}）"
            );
        });
    }
}

#[gpui::test]
async fn caret_lands_outside_leading_and_trailing_markup(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 用户报修：块首是 `**`、`<...>`、`[...]()` 这类标记时，打开文件后光标不在行首
    // （跑到标记里面），行尾光标也会落进标记内部甚至越过可见文本。
    for source in [
        "alpha",
        "**bold** tail",
        "<https://example.com> tail",
        "tail <https://example.com>",
        "[a](https://example.com) tail",
    ] {
        let (editor, cx) = cx.add_window_view({
            let source = source.to_string();
            move |_window, cx| Editor::from_markdown(cx, source, None)
        });
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            let block = block.read(cx);
            assert_eq!(block.selected_range, 0..0, "{source:?} 初始光标应在块首");
        });

        cx.simulate_keystrokes("end");
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            let block = block.read(cx);
            let display_len = block.display_text().len();
            let clean_len = block.record.title.visible_text().len();
            assert_eq!(
                block.selected_range,
                display_len..display_len,
                "{source:?} 行尾光标应停在显示文本末尾"
            );
            assert_eq!(
                block.current_to_clean_offset(block.selected_range.start),
                clean_len,
                "{source:?} 行尾光标在可见文本里的位置应是末尾"
            );
        });
    }
}

#[gpui::test]
async fn cjk_wrapping_does_not_start_lines_with_punctuation(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 中文没有词间空格，断行器逐字断行；没有行首禁则时标点会被推到下一行行首
    // （用户报修：标点经常出现在一行开头）。断点落在哪个字由容器宽度决定，所以
    // 这里把所有宽度都扫一遍——旧实现在其中不少宽度上会让标点开头。
    let text = "这是一段用来验证中文行首禁则的文字，里面有逗号、顿号；还有冒号：和分号。句号也不该落到行首，问号呢？感叹号也是！如果标点出现在行首，就说明禁则没有生效。".to_string();
    let font = gpui::Font {
        family: ".SystemUIFont".into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: gpui::FontWeight::NORMAL,
        style: gpui::FontStyle::Normal,
    };
    let mut wrapped_lines = 0usize;
    for width in (60..260).step_by(2) {
        let boundaries = cx.update(|cx| {
            let text_system = cx.text_system().clone();
            let mut wrapper = text_system.line_wrapper(font.clone(), px(12.0));
            wrapper
                .wrap_line(&[gpui::LineFragment::text(&text)], px(width as f32))
                .map(|boundary| boundary.ix)
                .collect::<Vec<_>>()
        });
        wrapped_lines += boundaries.len();
        for ix in boundaries {
            let rest = text.get(ix..).unwrap_or_default();
            let first = rest.chars().next().unwrap_or(' ');
            assert!(
                !"，、；：。？！）】”’".contains(first),
                "{width}px 宽时空行断点让标点出现在行首 {first:?}：{rest}"
            );
        }
    }
    assert!(wrapped_lines > 100, "折行样本太少（{wrapped_lines}），测试没跑够");
}

#[gpui::test]
async fn shaped_text_wrapping_respects_punctuation_rules(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let cx = cx.add_empty_window();
    // 编辑器走 shape_text，而不是 LineWrapper；覆盖截图中的中英文混排和窄行回退。
    let texts = [
        "Velora 把你的写作文件夹作为工作区打开，边打字边渲染 Markdown，在以 MiB 计的长篇手稿上依然流畅。代码文件在同一窗口内以语法高亮编辑，所有确认与提示都是应用内模态——不会有系统弹窗打断写作。",
        "这是一段中文，含有逗号、顿号；冒号：句号。问号？感叹号！以及连续标点？！和省略号……末尾。",
        "中文（Markdown）和“Velora”以及《Rust》文字【混排】结束。",
        "甲，乙。丙？！丁……戊",
        "（Markdown）“Velora”《Rust》【GPUI】",
    ];
    let mut wrapped_lines = 0usize;
    for text in texts {
        let run = gpui::TextRun {
            len: text.len(),
            font: gpui::Font {
                family: ".SystemUIFont".into(),
                features: gpui::FontFeatures::default(),
                fallbacks: None,
                weight: gpui::FontWeight::NORMAL,
                style: gpui::FontStyle::Normal,
            },
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size: None,
        };
        for width in (1..920).step_by(2) {
            cx.update(|window, _cx| {
                let lines = window
                    .text_system()
                    .shape_text(
                        text.into(),
                        px(18.0),
                        &[run.clone()],
                        Some(px(width as f32)),
                        None,
                    )
                    .expect("text should shape");
                for line in lines {
                    let mut previous_ix = 0;
                    for boundary in line.wrap_boundaries() {
                        let ix = line.unwrapped_layout.runs[boundary.run_ix].glyphs
                            [boundary.glyph_ix]
                            .index;
                        assert!(ix > previous_ix, "{width}px 换行断点必须递增");
                        previous_ix = ix;
                        let first = line.text[ix..].chars().next().unwrap();
                        let last = line.text[..ix].chars().next_back().unwrap();
                        assert!(
                            !"，、；：。？！…）】》”’".contains(first),
                            "{width}px 实际排版让标点出现在行首 {first:?}：{}",
                            &line.text[ix..]
                        );
                        assert!(
                            !"（【《“‘".contains(last),
                            "{width}px 实际排版让起始符号出现在行末 {last:?}：{}",
                            &line.text[..ix]
                        );
                        wrapped_lines += 1;
                    }
                }
            });
        }
    }
    assert!(wrapped_lines > 100, "必须实际产生足够的软换行");
}

#[gpui::test]
async fn rendered_prose_wraps_to_width_without_leading_punctuation(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "持有 `image_reference_definitions/link_reference_definitions/footnote_registry`（共享），以及 `table_cells: HashMap<EntityId, TableCellBinding>`。这是 mixed text! 含有 commas, periods. questions? semicolons; colons: 以及右括号 (closing) [bracket] {brace} 和中文（右括号）【方括号】《书名》、“右引号”，都不该落在行首。";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, markdown.to_string(), None)
    });
    for width in [480.0, 640.0, 820.0] {
        cx.simulate_resize(gpui::size(px(width), px(800.0)));
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            let lines = block.last_layout.as_ref().expect("正文应完成排版");
            let mut wraps = 0;
            for line in lines {
                let available = line.wrap_width.expect("正文应有换行宽度");
                let mut start_x = px(0.0);
                for boundary in line.wrap_boundaries() {
                    let glyph = &line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
                    let first = line.text[glyph.index..].trim_start().chars().next().unwrap();
                    assert!(
                        !"!！,，.。?？;；:：)]}>）］｝】》〉」』”’…、".contains(first),
                        "{width}px 行首出现 {first:?}：{}", &line.text[glyph.index..]
                    );
                    let row_width = glyph.position.x - start_x;
                    assert!(row_width <= available + px(0.5), "正文不应溢出");
                    for word in ["mixed", "text", "commas", "periods", "questions", "semicolons", "colons", "closing", "bracket", "brace"] {
                        for (start, _) in line.text.match_indices(word) {
                            assert!(
                                !(start < glyph.index && glyph.index < start + word.len()),
                                "{width}px 普通英文单词 {word} 被拆开"
                            );
                        }
                    }
                    start_x = glyph.position.x;
                    wraps += 1;
                }
            }
            assert!(wraps > 2, "应实际覆盖多行中英文混排");
        });
    }
}

#[gpui::test]
async fn blank_line_block_renders_as_a_small_gap(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 松列表（- a / 空行 / - b）：空行块原本占一整行高 + 上下 padding，加起来
    // 比正文一行还高（用户报修：空行太大）。空行块应该只剩下一个块间距的高度。
    let (_editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "- a\n\n- b\n".to_string(), None)
    });
    redraw(cx);
    let blank = cx.debug_bounds("block-blank-line").expect("没有找到空行块");
    let normal = cx.debug_bounds("block-shell").expect("没有找到正文块");
    assert!(
        blank.size.height <= px(20.0),
        "空行块高度 {:.1}px，还是太大",
        f32::from(blank.size.height)
    );
    assert!(
        blank.size.height < normal.size.height,
        "空行块 {:.1}px 不比正文块 {:.1}px 矮",
        f32::from(blank.size.height),
        f32::from(normal.size.height)
    );
}

#[gpui::test]
async fn tree_filter_appends_once_per_keystroke(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, String::new(), None)
    });
    editor.update(cx, |editor, cx| {
        let keystroke = |key: &str| KeyDownEvent {
            keystroke: Keystroke::parse(key).expect("valid keystroke"),
            is_held: false,
        };
        editor.on_tree_filter_key_down(&keystroke("m"), cx);
        editor.on_tree_filter_key_down(&keystroke("d"), cx);
        assert_eq!(
            editor.workspace.tree_filter, "md",
            "每次按键只追加一个字符（用户报修：重复挂载 on_key_down 曾把 md 双写成 mmdd）"
        );
        editor.on_tree_filter_key_down(&keystroke("backspace"), cx);
        assert_eq!(editor.workspace.tree_filter, "m", "退格只删除一个字符");
        editor.on_tree_filter_key_down(&keystroke("escape"), cx);
        assert!(editor.workspace.tree_filter.is_empty(), "Esc 清空过滤词");
    });
}

#[gpui::test]
async fn status_bar_view_mode_toggle_switches_mode(cx: &mut TestAppContext) {
    // 用户需求：右下角的「分钟阅读」换成源码切换按钮，点击切换视图模式。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# hello\n\nworld\n".to_string(), None)
    });
    redraw(cx);

    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.view_mode, crate::editor::ViewMode::Rendered);
    });

    let bounds = cx
        .debug_bounds("status-bar-view-mode-toggle")
        .expect("状态栏应渲染视图切换按钮");
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.view_mode,
            crate::editor::ViewMode::Source,
            "点击切换按钮应进入源码模式"
        );
    });

    let bounds = cx
        .debug_bounds("status-bar-view-mode-toggle")
        .expect("源码模式下按钮仍在");
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.view_mode,
            crate::editor::ViewMode::Rendered,
            "再次点击应切回渲染模式"
        );
    });
}


// ===== 超长行折叠（JSONL / 日志类文档性能）=====

/// 打开一个带超长行的代码文档，返回编辑器实体。3 行：短 / 2000 字符长行 / 短。
fn long_line_code_source() -> String {
    format!(
        "short line\n{}\nanother short line\n",
        "x".repeat(2000)
    )
}

/// 把源码写成临时 .log 并以代码文档模式开窗（源码文档按行分块、带行号槽）。
fn open_code_document_window<'a>(
    cx: &'a mut gpui::TestAppContext,
    source: &str,
    name: &str,
) -> (gpui::Entity<crate::editor::Editor>, &'a mut gpui::VisualTestContext) {
    let path =
        std::env::temp_dir().join(format!("velora-long-line-{name}-{}.log", std::process::id()));
    std::fs::write(&path, source).expect("write fixture");
    let source = source.to_string();
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        let mut editor = crate::editor::Editor::from_markdown(cx, String::new(), None);
        editor.replace_document_from_code_source(source, path, cx);
        editor
    });
    cx.run_until_parked();
    (editor, cx)
}

#[gpui::test]
async fn long_source_lines_collapse_to_single_unwrapped_row(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = long_line_code_source();
    let (editor, cx) = open_code_document_window(cx, &source, "collapse");

    redraw(cx);
    editor
        .read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            let lines = block.last_layout.as_ref().expect("应完成排版");
            assert_eq!(
                lines.len(),
                4,
                "源文本带行尾换行 = 3 可见行 + 1 空尾行；layout 条目必须与源行范围表一一对应，\
                 超长行折叠成单行、不允许换行炸高"
            );
            for (idx, line) in lines.iter().enumerate() {
                assert!(
                    line.wrap_boundaries().is_empty(),
                    "折叠态第 {idx} 行不应有软换行"
                );
            }
            // 长行单行宽度必须超过视口（否则说明没走不换行路径）。
            let long_width = lines[1].width();
            assert!(
                long_width > gpui::px(800.0),
                "长行应保持单行完整宽度，实际 {long_width:?}"
            );
            assert!(
                !block.expanded_long_lines.contains(&1),
                "默认折叠"
            );
        });
}

#[gpui::test]
async fn gutter_click_expands_long_line_into_wrapped_rows(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = long_line_code_source();
    let (editor, cx) = open_code_document_window(cx, &source, "expand");

    redraw(cx);
    // 点行号槽（text_bounds 左侧的 gutter 区）第 2 行（超长行）。
    let click_position = editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        let bounds = block.last_bounds.as_ref().expect("应已布局");
        gpui::point(
            bounds.left() - block.last_gutter_width / 2.0,
            bounds.top() + block.last_line_height * 1.5,
        )
    });
    let toggled = editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _block_cx| block.toggle_long_line_at_gutter(click_position))
    });
    assert!(toggled, "点行号槽应切换超长行");
    redraw(cx);

    editor
        .read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert!(block.expanded_long_lines.contains(&1), "展开状态应记录");
            let lines = block.last_layout.as_ref().expect("应完成排版");
            assert_eq!(lines.len(), 4, "展开不改变源行条目数（含空尾行）");
            assert!(
                !lines[1].wrap_boundaries().is_empty(),
                "展开后长行应按容器宽换行"
            );
            let short_height = lines[0].size(block.last_line_height).height;
            let long_height = lines[1].size(block.last_line_height).height;
            assert!(
                long_height > short_height * 2.0,
                "展开后的长行应显著高于单行（{long_height:?} vs {short_height:?}）"
            );

            // 再点一次收起。
        });

    let click_position = editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        let bounds = block.last_bounds.as_ref().expect("应已布局");
        gpui::point(
            bounds.left() - block.last_gutter_width / 2.0,
            bounds.top() + block.last_line_height * 1.5,
        )
    });
    let toggled = editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _block_cx| block.toggle_long_line_at_gutter(click_position))
    });
    assert!(toggled, "再次点击应能收起");
    redraw(cx);
    editor
        .read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert!(
                !block.expanded_long_lines.contains(&1),
                "收起后不应留在展开集合里"
            );
            let lines = block.last_layout.as_ref().expect("应完成排版");
            assert!(
                lines[1].wrap_boundaries().is_empty(),
                "收起后长行恢复单行"
            );
        });
}

#[gpui::test]
async fn collapsed_long_line_has_no_horizontal_scrolling(cx: &mut TestAppContext) {
    // 用户定版行为：折叠单行不许横向滚动，超出部分直接裁切，只能点行号展开。
    init_editor_test_app(cx);
    let source = long_line_code_source();
    let (editor, cx) = open_code_document_window(cx, &source, "noscroll");
    redraw(cx);

    // 整个文档树里不应存在任何代码块的横向滚动容器。
    let block_ids = editor.read_with(cx, |editor, _cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .map(|visible| visible.entity.entity_id())
            .collect::<Vec<_>>()
    });
    let mut scroll_containers = 0;
    for id in block_ids {
        // debug_bounds 只收 'static 选择器，测试里泄漏这几个短字符串无妨。
        let code_sel: &'static str = Box::leak(format!("code-x-scroll-{id}").into_boxed_str());
        let source_sel: &'static str = Box::leak(format!("source-x-scroll-{id}").into_boxed_str());
        if cx.debug_bounds(code_sel).is_some() || cx.debug_bounds(source_sel).is_some() {
            scroll_containers += 1;
        }
    }
    assert_eq!(scroll_containers, 0, "折叠单行不允许横向滚动：不应有横滚容器");

    // 文本元素宽度被钳在容器宽内（不溢出），行依然单行不换行。
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        let lines = block.last_layout.as_ref().expect("应完成排版");
        assert!(
            lines[1].wrap_boundaries().is_empty(),
            "折叠行必须保持单行"
        );
        let bounds = block.last_bounds.as_ref().expect("应已布局");
        assert!(
            bounds.size.width < gpui::px(2500.0),
            "文本区应被钳在容器宽内（裁切显示），实际 {:?}",
            bounds.size
        );
    });
}

#[gpui::test]
async fn collapsed_long_line_is_truncated_to_display_cap(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = String::new();
    source.push_str(&"y".repeat(30_000));
    source.push_str("\nshort\n");
    let (editor, cx) = open_code_document_window(cx, &source, "truncate");

    redraw(cx);
    editor
        .read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            let lines = block.last_layout.as_ref().expect("应完成排版");
            assert_eq!(lines.len(), 3, "长行 + short + 空尾行 = 3 条目");
            let shaped_chars = lines[0].text.chars().count();
            assert!(
                shaped_chars < 21_000,
                "折叠态 3 万字符的行应截断到显示上限附近，实际 {shaped_chars}"
            );
            assert!(
                lines[0].text.contains("已截断"),
                "截断行尾应有提示：{}",
                &lines[0].text[lines[0].text.len() - 80..]
            );
            // 截断行的文本不再是原始全文，但索引映射仍以原始文本行范围表为准：
            // 点击行首得到原始偏移 0。
            let origin_index = block.index_for_mouse_position(gpui::point(
                block.last_bounds.expect("应已布局").left() + gpui::px(1.0),
                block.last_bounds.expect("应已布局").top() + gpui::px(1.0),
            ));
            assert_eq!(origin_index, 0, "点击长行行首应映射到原始文本偏移 0");
        });
}

#[gpui::test]
async fn drop_open_mode_matches_workspace_open_mode(cx: &mut TestAppContext) {
    // 拖拽与工作区树打开必须共用同一判定：只有 .md/.markdown 按 Markdown
    // 解析；.jsonl 等一律代码文档（此前拖拽走 is_code_file 白名单，.jsonl
    // 被误当 Markdown）。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, String::new(), None)
    });

    let jsonl = std::env::temp_dir().join(format!("velora-drop-{}.jsonl", std::process::id()));
    std::fs::write(&jsonl, "{\"a\":1}\n{\"a\":2}\n").expect("write jsonl");
    let markdown_ext =
        std::env::temp_dir().join(format!("velora-drop-{}.markdown", std::process::id()));
    std::fs::write(&markdown_ext, "# 标题\n\n正文\n").expect("write markdown");

    editor.update(cx, |editor, cx| {
        editor
            .replace_document_from_path(&jsonl, cx)
            .expect("jsonl should open");
    });
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.code_tab_active(),
            ".jsonl 拖拽打开必须是代码文档模式"
        );
    });

    editor.update(cx, |editor, cx| {
        editor
            .replace_document_from_path(&markdown_ext, cx)
            .expect("markdown should open");
    });
    editor.read_with(cx, |editor, _| {
        assert!(
            !editor.code_tab_active(),
            ".markdown 拖拽打开必须是 Markdown 渲染模式"
        );
    });
}

#[gpui::test]
async fn modal_enter_triggers_default_and_escape_cancels(cx: &mut TestAppContext) {
    // C13：模态支持键盘。Enter = 默认按钮，Esc = 取消位。
    // 实现挂在编辑器按键捕获钩子上（模态不抢焦点），所以先聚焦一个块
    // 让按键有派发路径。
    use std::cell::RefCell;
    use std::rc::Rc;

    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha".to_string(), None)
    });
    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
    });
    redraw(cx);

    let choice: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

    // Enter = 默认按钮（删除）。
    let sink = choice.clone();
    editor.update(cx, |editor, cx| {
        editor.show_modal(
            crate::editor::modal::ModalSpec {
                title: "确认删除".into(),
                detail: None,
                buttons: vec!["删除".into(), "取消".into()],
                default_index: 0,
                cancel_index: 1,
            },
            move |index, _editor, _window, _cx| {
                *sink.borrow_mut() = Some(index);
            },
            cx,
        );
    });
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(editor.modal_is_open(), "前置：模态应已打开");
    });
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(*choice.borrow(), Some(0), "Enter 应触发默认按钮");
    editor.read_with(cx, |editor, _| {
        assert!(!editor.modal_is_open(), "Enter 后模态应关闭");
    });

    // Esc = 取消位（取消）。
    let sink = choice.clone();
    editor.update(cx, |editor, cx| {
        editor.show_modal(
            crate::editor::modal::ModalSpec {
                title: "确认删除".into(),
                detail: None,
                buttons: vec!["删除".into(), "取消".into()],
                default_index: 0,
                cancel_index: 1,
            },
            move |index, _editor, _window, _cx| {
                *sink.borrow_mut() = Some(index);
            },
            cx,
        );
    });
    redraw(cx);
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert_eq!(*choice.borrow(), Some(1), "Esc 应触发取消位");
    editor.read_with(cx, |editor, _| {
        assert!(!editor.modal_is_open(), "Esc 后模态应关闭");
    });
}

#[gpui::test]
async fn knowledge_panels_list_backlinks_and_tags_end_to_end(cx: &mut TestAppContext) {
    // 反链 + 标签面板端到端：树落地后索引重建，面板列条目，点击生效。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-km-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("a.md"), "见 [[target]] #rust\n").expect("write a");
    std::fs::write(root.join("b.md"), "#rust 和 #gpui，与目标无关\n").expect("write b");
    std::fs::write(root.join("target.md"), "# target\n").expect("write target");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace_link_index.tracked_file_count(),
            3,
            "树落地后索引应包含全部 Markdown 文件"
        );
    });

    let target_path = root.join("target.md");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(target_path, window, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.set_workspace_tab(super::workspace::WorkspaceTab::Backlinks, cx);
    });
    redraw(cx);
    assert!(
        cx.debug_bounds("backlink-entry-0").is_some(),
        "反链面板应列出 a.md"
    );
    assert!(
        cx.debug_bounds("backlink-entry-1").is_none(),
        "无关文件不应出现"
    );
    let row = cx.debug_bounds("backlink-entry-0").expect("row");
    cx.simulate_click(row.center(), gpui::Modifiers::none());
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor
                .file_path
                .as_ref()
                .map(|path| path.file_name().unwrap().to_string_lossy().to_string()),
            Some("a.md".to_string()),
            "点击反链条目应打开对应笔记"
        );
    });

    editor.update(cx, |editor, cx| {
        editor.set_workspace_tab(super::workspace::WorkspaceTab::Tags, cx);
    });
    redraw(cx);
    let first = cx.debug_bounds("tag-entry-0").expect("tag row 0");
    let second = cx.debug_bounds("tag-entry-1").expect("tag row 1");
    assert!(first.origin.y <= second.origin.y, "#rust 应排在 #gpui 前");
    cx.simulate_click(first.center(), gpui::Modifiers::none());
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.active_tab,
            super::workspace::WorkspaceTab::Search
        );
        assert_eq!(editor.workspace.search_query, "#rust");
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn tags_panel_lists_workspace_tags_and_click_starts_search(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-tags-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("a.md"), "#rust 笔记\n").expect("write a");
    std::fs::write(root.join("b.md"), "也是 #rust 和 #gpui\n").expect("write b");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();
    cx.run_until_parked();

    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.set_workspace_tab(super::workspace::WorkspaceTab::Tags, cx);
    });
    redraw(cx);

    // #rust 两个文件引用排第一，#gpui 一个排第二。
    let first = cx.debug_bounds("tag-entry-0").expect("tag row 0");
    let second = cx.debug_bounds("tag-entry-1").expect("tag row 1");
    assert!(first.origin.y <= second.origin.y, "#rust 应排在 #gpui 前");

    // 点击标签 → 进入搜索 tab，query 已填 #rust。
    cx.simulate_click(first.center(), gpui::Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.workspace.active_tab, super::workspace::WorkspaceTab::Search);
        assert_eq!(editor.workspace.search_query, "#rust");
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn typing_wikilink_opens_completion_and_enter_inserts_target(cx: &mut TestAppContext) {
    // [[ 补全端到端：输入 [[ 弹浮层 → 查询过滤 → Enter 插入 stem+]]。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-wlc-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("alpha.md"), "").expect("write alpha");
    std::fs::write(root.join("beta.md"), "# beta\n").expect("write beta");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();
    let alpha = root.join("alpha.md");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha, window, cx);
        });
    });

    // 聚焦首块后输入 "a[[be"：浮层出现且过滤到 beta。
    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
    });
    redraw(cx);
    cx.simulate_input("a[[be");
    redraw(cx);
    assert!(
        cx.debug_bounds("wikilink-completion").is_some(),
        "输入 [[查询 后应出现补全浮层"
    );
    assert!(
        cx.debug_bounds("wikilink-entry-0").is_some(),
        "应过滤出 beta"
    );
    assert!(
        cx.debug_bounds("wikilink-entry-1").is_none(),
        "alpha 不匹配 be 查询"
    );

    // Enter 插入 "beta]]" 且不换行。
    cx.simulate_keystrokes("enter");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        assert_eq!(block.display_text(), "a[[beta]]", "Enter 应插入 stem 与收尾");
        assert!(block.cursor_offset() >= 9, "光标应停在 ]] 之后");
    });
    assert!(
        cx.debug_bounds("wikilink-completion").is_none(),
        "插入后浮层应关闭"
    );

    // Esc 关闭：重新输入 [[ 后按 Esc。
    cx.simulate_input(" [[");
    redraw(cx);
    assert!(cx.debug_bounds("wikilink-completion").is_some());
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(
        cx.debug_bounds("wikilink-completion").is_none(),
        "Esc 应关闭浮层"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_history_records_dedupes_and_prunes() {
    // 存储约定：时间戳命名、同内容去重、每文件保留最近 20 条。
    let root = std::env::temp_dir().join(format!("velora-fhist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let file = root.join("doc.md");
    std::fs::create_dir_all(&root).expect("create root");

    // 记录走的是 config 全局目录；测试构建用进程级临时配置根
    // （VeloraConfigDirs::from_system 的 test 分支），不能直接指定 root，
    // 所以这里只验证去重与上限行为，目录隔离交给测试根。
    for index in 0..25 {
        let content = format!("版本 {index}");
        crate::config::record_file_history(&file, &content).expect("record");
    }
    let versions = crate::config::list_file_history(&file);
    assert_eq!(versions.len(), 20, "超出上限应裁剪到 20 条");
    let newest = std::fs::read_to_string(&versions[0]).expect("read newest");
    assert_eq!(newest, "版本 24", "最新一条应是最后一次保存的内容");
    let oldest = std::fs::read_to_string(&versions.last().unwrap()).expect("read oldest");
    assert_eq!(oldest, "版本 5", "最老的 0..=4 应被裁掉");

    // 同内容再保存不重复落盘。
    crate::config::record_file_history(&file, "版本 24").expect("record dup");
    assert_eq!(crate::config::list_file_history(&file).len(), 20);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
async fn inserting_a_table_through_the_dialog_can_be_undone(cx: &mut TestAppContext) {
    // 审查发现：表格插入对话框不进撤销栈（其它表格操作都进），Ctrl+Z 要么什么
    // 都不做，要么把之前的一次编辑一并撤掉。
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# a\n\nbody\n".into(), None));
    redraw(cx);
    let before = editor.read_with(cx, |editor, cx| editor.current_document_source(cx));
    editor.update(cx, |editor, cx| {
        editor.table_insert_dialog = Some(super::context_menu::TableInsertDialogState {
            target: super::context_menu::TableInsertTarget::Append,
            body_rows: 1,
            columns: 2,
        });
        assert!(editor.insert_table_from_dialog(cx), "对话框应插入表格");
    });
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.current_document_source(cx).contains('|'),
            "前置：表格已插入"
        );
    });
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.current_document_source(cx),
            before,
            "⌘Z 应撤销表格插入"
        );
    });
}

#[gpui::test]
async fn escape_dismisses_the_info_dialog(cx: &mut TestAppContext) {
    // 审查发现：信息弹窗（关于/检查更新）没有键盘路径也没有遮罩点击，Esc 关不掉。
    init_editor_test_app(cx);
    cx.update(|cx| crate::app_menu::init(cx));
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# a\n".into(), None));
    cx.update(|window, cx| window.draw(cx).clear());
    editor.update(cx, |editor, cx| {
        editor.show_info_dialog(super::InfoDialogKind::About, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.info_dialog.is_some(), "前置：弹窗已打开");
    });
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.info_dialog.is_none(), "Esc 应关掉信息弹窗");
    });
}

#[gpui::test]
async fn escape_closes_the_in_window_menu_bar(cx: &mut TestAppContext) {
    // 审查发现：标题栏菜单面板没有键盘路径，Esc 关不掉（只能等 hover 超时或点正文）。
    init_editor_test_app(cx);
    cx.update(|cx| crate::app_menu::init(cx));
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# a\n".into(), None));
    cx.update(|window, cx| window.draw(cx).clear());
    editor.update(cx, |editor, _cx| {
        editor.menu_bar_open = Some(0);
    });
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.menu_bar_open.is_some(), "前置：菜单已打开");
    });
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.menu_bar_open.is_none(), "Esc 应关掉标题栏菜单");
    });
}

#[gpui::test]
async fn closing_quick_open_restores_focus_to_the_document(cx: &mut TestAppContext) {
    // 审查发现：关掉 ⌘P 只丢状态不还焦点，之后敲字全丢（要再手点正文）。
    init_editor_test_app(cx);
    cx.update(|cx| crate::app_menu::init(cx));
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# a\n\nbody\n".into(), None));
    cx.update(|window, cx| window.draw(cx).clear());
    let block_id = editor.read_with(cx, |editor, _| {
        editor.document.root_blocks()[1].entity_id()
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let block = editor
                .document
                .block_entity_by_id(block_id)
                .expect("目标块");
            window.focus(&block.read(cx).focus_handle);
            editor.toggle_quick_open(window, cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.quick_open.is_some(), "前置：⌘P 已打开");
    });
    // Esc 关闭（走全局 DismissTransientUi 路径）。
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| {
        editor.read_with(cx, |editor, cx| {
            assert!(editor.quick_open.is_none(), "前置：⌘P 已关闭");
            let block = editor
                .document
                .block_entity_by_id(block_id)
                .expect("目标块");
            assert!(
                block.read(cx).focus_handle.is_focused(window),
                "关闭 ⌘P 后焦点应回到正文块"
            );
        });
    });
}

#[gpui::test]
async fn file_history_restore_can_be_undone(cx: &mut TestAppContext) {
    // 用户报修（审查发现）：恢复历史版本会清空 undo 栈，模块注释承诺的
    // 「可撤销」是假的——误按 Enter 就丢掉当前未保存内容且无法撤回。
    init_editor_test_app(cx);
    let path = temp_markdown_path("file-history-undo");
    std::fs::write(&path, "当前内容").expect("seed current");
    crate::config::record_file_history(&path, "旧版本内容").expect("record older");
    let history_files = crate::config::list_file_history(&path);
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = std::fs::remove_file(cleanup_path);
        for file in history_files {
            let _ = std::fs::remove_file(file);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown("当前内容".to_string(), Some(path.clone()), cx);
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        editor.open_file_history(cx);
        editor.restore_file_history_version(0, cx);
    });
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.current_document_source(cx),
            "旧版本内容",
            "恢复后正文应是历史内容"
        );
        assert!(editor.document_dirty, "恢复是未保存修改");
    });

    // ⌘Z 应回到恢复前的内容。
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.current_document_source(cx),
            "当前内容",
            "恢复历史版本必须可撤销，⌘Z 回到恢复前内容"
        );
    });
}

#[gpui::test]
async fn file_history_overlay_restores_version_as_unsaved_edit(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let path = temp_markdown_path("file-history");
    std::fs::write(&path, "第一版内容").expect("seed v1");

    // 直接落两条历史（记录路径已有单测），浮层走真实数据。
    crate::config::record_file_history(&path, "第一版内容").expect("record v1");
    crate::config::record_file_history(&path, "第二版内容").expect("record v2");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown("当前编辑内容".to_string(), Some(path.clone()), cx);
    });
    redraw(cx);

    // 打开历史浮层：两条版本，最新在前。
    editor.update(cx, |editor, cx| {
        editor.open_file_history(cx);
    });
    redraw(cx);
    assert!(cx.debug_bounds("file-history-entry-0").is_some(), "应有版本行");
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.file_history_overlay.as_ref().expect("浮层应打开").selected,
            0,
            "默认选中最新"
        );
    });

    // ↓ 选上一版，Enter 恢复为未保存修改。
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.current_document_source(cx).contains("第一版内容"),
            "恢复选中版本内容"
        );
        assert!(editor.document_dirty, "恢复后应为未保存状态");
        assert!(!editor.file_history_is_open(), "恢复后浮层关闭");
    });
    let _ = std::fs::remove_file(&path);
}
