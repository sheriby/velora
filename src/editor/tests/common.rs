pub(super) use std::fs;
pub(super) use std::path::PathBuf;
pub(super) use std::sync::Arc;
pub(super) use std::time::{Duration, Instant};

pub(super) use gpui::{
    AnyWindowHandle, AppContext, ClickEvent, EntityInputHandler, Font, FontStyle, FontWeight,
    KeyDownEvent, Keystroke, Modifiers, TestAppContext, TextRun, VisualTestContext, WindowBounds,
    WindowHandle, px,
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

/// 测试夹具目录。编辑器把「当前文件所在目录」当作隐含工作区根，面板展开时会递归
/// 扫这个根；夹具直接落在系统临时根目录，就会被扫整台机器的 T 目录（本机实测 24k
/// 条目 / 24 万个目录，单条用例十几秒）。统一放这个扁平目录：一次扫描只剩一份目录
/// 项列表，兄弟资源（同名 .assets、导出的 html 等）也仍然落在同一个根下。
pub(super) fn temp_fixture_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("velora-tests-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create test fixture dir");
    dir
}

/// 并发用例共用的唯一后缀：pid 区分进程，uuid 区分同一进程里的每次调用。
///
/// 不许换成墙上时钟。Windows 的钟粒度粗（~15ms），两次 `SystemTime::now()` 会读到同一个
/// 值：同名夹具于是算出同一条路径，两个用例在同一份字节上来回保存——CI 上「无末行换行」
/// 那条就是这样被隔壁用例插进去一个 `X`。pid 单独也不够，同进程的用例共用一个值。
pub(super) fn temp_fixture_token() -> String {
    format!("{}-{}", std::process::id(), uuid::Uuid::new_v4())
}

/// 同名夹具每次都要落到新文件：并发用例各写各的文件，撞上就是互相覆盖。
pub(super) fn temp_markdown_path(test_name: &str) -> PathBuf {
    temp_fixture_dir().join(format!("velora-{test_name}-{}.md", temp_fixture_token()))
}

/// 夹具路径的唯一性只看这条：整块表里的用例都靠它把并发隔开。
#[test]
fn same_name_fixtures_never_share_a_path() {
    let paths: std::collections::HashSet<PathBuf> =
        (0..1000).map(|_| temp_markdown_path("同名夹具")).collect();
    assert_eq!(paths.len(), 1000, "同名夹具算出了重复路径：并发用例会互相覆盖");
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

/// 用系统真实字体逐条量出这些标签的像素宽，返回与输入同序的一份宽度。
///
/// 菜单的面板宽是按字符类别估出来的（`estimated_menu_label_width`），估少了标签会被
/// `.truncate()` 切掉一角且不报错（用户两次报修的都是这一处）。守卫拿真实度量对一遍，
/// 换字体或改系数时才会红。`cx.text_system()` 在 App 层不暴露度量接口，要走窗口那一份。
pub(super) fn real_label_widths(
    labels: &[String],
    text_size: f32,
    cx: &mut VisualTestContext,
) -> Vec<f32> {
    cx.update(|window, _cx| {
        let font = Font {
            family: ".SystemUIFont".into(),
            features: gpui::FontFeatures::default(),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        };
        labels
            .iter()
            .map(|label| {
                let run = TextRun {
                    len: label.len(),
                    font: font.clone(),
                    color: gpui::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                    font_size: None,
                };
                window
                    .text_system()
                    .shape_text(label.clone().into(), px(text_size), &[run], None, None)
                    .expect("这一行该能量出来")
                    .first()
                    .map(|line| f32::from(line.width()))
                    .expect("量出来至少有一行")
            })
            .collect()
    })
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
