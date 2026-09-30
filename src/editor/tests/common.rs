pub(super) use std::fs;
pub(super) use std::path::PathBuf;
pub(super) use std::sync::Arc;
pub(super) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) use gpui::{
    AnyWindowHandle, AppContext, ClickEvent, EntityInputHandler, KeyDownEvent, Keystroke,
    Modifiers, TestAppContext, VisualTestContext, WindowBounds, WindowHandle, px,
};

pub(super) use crate::editor::{Editor, MountedRun, ViewMode};
pub(super) use crate::components::{
    Block, BlockEvent, BlockKind, BlockRecord, CloseWindow, Delete, DeleteBack, FocusNext,
    ImageReferenceDefinitions, ImageResolvedSource, InlineTextTree, Newline, QuitApplication,
    SaveDocument, TableCellInlineImageSegment, TableColumnAlignment, UndoCaptureKind,
    parse_table_cell_inline_images, superscript_ordinal,
};
pub(super) use crate::export::ExportFormat;
pub(super) use crate::i18n::{I18nManager, I18nStrings};
pub(super) use crate::theme::{Theme, ThemeManager};
pub(super) fn init_editor_test_app(cx: &mut TestAppContext) {
    cx.update(|cx| {
        I18nManager::init(cx);
        ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

pub(super) fn temp_markdown_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "velora-{test_name}-{}-{nanos}.md",
        std::process::id()
    ))
}

pub(super) fn temp_export_path(test_name: &str, extension: &str) -> PathBuf {
    let mut path = temp_markdown_path(test_name);
    path.set_extension(extension);
    path
}

/// P2 性能守门的四个全文遍数：序列化 / mapping 重建 / 字数扫描 / 行计划重建。
pub(super) fn perf_passes(
    editor: &gpui::Entity<Editor>,
    cx: &mut gpui::VisualTestContext,
) -> (u64, u64, u64, u64) {
    editor.read_with(cx, |editor, _| {
        (
            editor.source_serializations.get(),
            editor.source_mapping_builds.get(),
            editor.word_count_scans.get(),
            editor.row_plan_rebuilds.get(),
        )
    })
}

pub(super) fn perf_delta(
    from: (u64, u64, u64, u64),
    to: (u64, u64, u64, u64),
) -> (u64, u64, u64, u64) {
    (to.0 - from.0, to.1 - from.1, to.2 - from.2, to.3 - from.3)
}

pub(super) fn redraw(cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

pub(super) fn activate_visual_window(cx: &mut VisualTestContext) -> AnyWindowHandle {
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    cx.cx
        .update(|cx| cx.active_window().expect("window should be active"))
}

/// 读取窗口 frame 为 (x, y, width, height)，用于窗口位置/大小断言。
pub(super) fn windowed_rect(handle: &WindowHandle<Editor>, cx: &mut TestAppContext) -> (i32, i32, i32, i32) {
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
