use super::common::*;

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

#[gpui::test]
async fn starting_and_ending_scrollbar_drag_updates_editor_state(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.pending_scroll_active_block_into_view = true;
        editor.pending_scroll_recheck_after_layout = true;

        editor.start_scrollbar_drag(12.0, 320.0, 64.0, 500.0, cx);
        assert_eq!(
            editor.scrollbar_drag,
            Some(crate::editor::ScrollbarDragSession {
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

