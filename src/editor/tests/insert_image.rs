//! 「插入 → 图片」那一条入口。选文件那一步走原生选择器，gpui 的测试壳把
//! `prompt_for_paths` 写成 `unimplemented!()`（vendor/gpui/src/platform/test/platform.rs:334），
//! 一点就 panic，所以用例测的是选择器交回的那条公共入口 `Editor::insert_image_at_caret`
//! ——拖放那条路现在也收在这里。
//!
//! 用例看四件事：图片行自己成一块、夹在光标切开的那两段中间；别的块一个字节不动
//! （尤其 `__下划线__` 那种会被整篇重投影洗掉写法）；写下去的字节重新读一遍还是同一套结构；
//! 一步撤销回到原样。

use super::common::*;
use crate::components::Block;
use crate::editor::context_menu::{
    DocumentMenuCommand, DocumentMenuRow, DocumentSubmenu, document_menu_shortcut,
};
use crate::editor::encoding;
use gpui::{Entity, Modifiers, MouseButton, point};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 图片插在中间那一段，前后各留一段文字，最后一行留着核对波及之外的字节。
const DOC: &str = "段落文字\n\n强调 __下划线__ 结尾\n\n尾巴\n";
/// 磁盘上的图片按本仓的粘贴口径先收进文档旁边的 `assets`，写下的就是那份相对路径。
const IMAGE_LINE: &str = "![pic](./assets/pic.png)";
/// 「强调 __下划线__ 结尾」在屏幕上的样子：下划线记号是写法，不进可见文本。
const UNDERLINED: &str = "强调 下划线 结尾";

fn scratch_dir(cx: &mut TestAppContext, tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时钟早于 unix 纪元")
        .as_nanos();
    let candidate =
        std::env::temp_dir().join(format!("velora-{tag}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&candidate).expect("建临时目录");
    // macOS 的临时目录是软链接（/var → /private/var）：不收敛一遍，算出来的相对路径
    // 与文件真正的目录不同，图片行会写成 `../../var/...`。
    let dir = candidate.canonicalize().expect("收敛临时目录");
    let cleanup = dir.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(&cleanup);
    });
    dir
}

fn open_document<'a>(
    text: &'static str,
    dir: &Path,
    cx: &'a mut TestAppContext,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    let path = dir.join("doc.md");
    fs::write(&path, text).expect("写夹具文件");
    let document = encoding::load_document(&path).expect("读夹具文件");
    let open_path = path.clone();
    cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(open_path))
    })
}

fn picture_in(dir: &Path) -> PathBuf {
    let picture = dir.join("pic.png");
    fs::write(&picture, b"\x89PNG\r\n\x1a\n not a real png").expect("写图片文件");
    picture
}

fn visible_block(
    editor: &Entity<Editor>,
    index: usize,
    cx: &mut VisualTestContext,
) -> Entity<Block> {
    editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[index].entity.clone()
    })
}

fn buffer_text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _cx| editor.buffer.text())
}

fn visible_texts(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .map(|entry| entry.entity.read(cx).display_text().to_string())
            .collect()
    })
}

fn has_image_runtime(editor: &Entity<Editor>, index: usize, cx: &mut VisualTestContext) -> bool {
    visible_block(editor, index, cx).read_with(cx, |block, _cx| block.image_runtime().is_some())
}

fn put_caret(editor: &Entity<Editor>, index: usize, offset: usize, cx: &mut VisualTestContext) {
    let block = visible_block(editor, index, cx);
    editor.update(cx, |editor, _cx| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _cx| {
        block.selected_range = offset..offset;
    });
    redraw(cx);
}

fn insert_picture(editor: &Entity<Editor>, picture: PathBuf, cx: &mut VisualTestContext) -> bool {
    editor.update(cx, |editor, cx| editor.insert_image_at_caret(picture, cx))
}

fn available(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, cx| editor.image_insert_is_available(cx))
}

fn undo(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
}

fn block_center(
    editor: &Entity<Editor>,
    index: usize,
    cx: &mut VisualTestContext,
) -> gpui::Point<gpui::Pixels> {
    let bounds = visible_block(editor, index, cx).read_with(cx, |block, _cx| {
        block
            .last_bounds
            .unwrap_or_else(|| panic!("第 {index} 块该有布局边界"))
    });
    point(
        bounds.left() + bounds.size.width * 0.5,
        bounds.top() + bounds.size.height * 0.5,
    )
}

fn right_click(editor: &Entity<Editor>, index: usize, cx: &mut VisualTestContext) {
    let position = block_center(editor, index, cx);
    cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
    redraw(cx);
}

fn menu_row(name: &'static str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    let selector: &'static str = Box::leak(format!("menu-item-{name}").into_boxed_str());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("菜单里没渲染出 {name} 这一行"))
}

#[gpui::test]
async fn inserting_a_picture_splits_the_paragraph_and_undoes_as_a_step(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let dir = scratch_dir(cx, "insert-image");
    let picture = picture_in(&dir);
    let (editor, cx) = open_document(DOC, &dir, cx);
    redraw(cx);

    // 「段落文字」第 6 个字节之后：正好切在「段落」与「文字」中间。
    put_caret(&editor, 0, 6, cx);
    assert!(available(&editor, cx), "光标在正文里，这一行该点得动");
    assert!(insert_picture(&editor, picture.clone(), cx));
    redraw(cx);

    assert_eq!(
        visible_texts(&editor, cx),
        ["段落", IMAGE_LINE, "文字", UNDERLINED, "尾巴"],
        "图片行该自己成一块，夹在光标切开的那两段中间"
    );
    assert!(
        has_image_runtime(&editor, 1, cx),
        "插进来的那行没有图片运行时，只是字面文本"
    );
    assert!(
        dir.join("assets").join("pic.png").is_file(),
        "图片没按粘贴那条路收进文档旁边的 assets"
    );
    assert_eq!(
        buffer_text(&editor, cx),
        format!("段落\n\n{IMAGE_LINE}\n\n文字\n\n强调 __下划线__ 结尾\n\n尾巴\n"),
        "缓冲区字节不对，或者顺带把别处的 `__下划线__` 写法洗掉了"
    );

    undo(&editor, cx);
    assert_eq!(
        buffer_text(&editor, cx),
        DOC,
        "一步撤销要把图片行与切开的那两段一起放回去"
    );
}

#[gpui::test]
async fn the_written_picture_line_reads_back_as_the_same_structure(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let dir = scratch_dir(cx, "insert-image-reparse");
    let picture = picture_in(&dir);
    let (editor, cx) = open_document(DOC, &dir, cx);
    redraw(cx);

    // 顶到那一段开头插：前面不留空段，形状是「原段落 + 图片行 + 后两段」。
    put_caret(&editor, 1, 0, cx);
    assert!(insert_picture(&editor, picture.clone(), cx));
    redraw(cx);
    let text = buffer_text(&editor, cx);

    let reread = cx.new(|cx| Editor::from_markdown(cx, text.clone(), Some(dir.join("doc.md"))));
    assert_eq!(
        visible_texts(&reread, cx),
        ["段落文字", IMAGE_LINE, UNDERLINED, "尾巴"],
        "写下去的字节读回来不是同一套结构：{text:?}"
    );
    assert!(
        has_image_runtime(&reread, 1, cx),
        "读回来的那行没有图片运行时"
    );
}

#[gpui::test]
async fn the_insert_menu_offers_the_picture_row_on_the_same_terms(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let dir = scratch_dir(cx, "insert-image-menu");
    let (editor, cx) = open_document(DOC, &dir, cx);
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Insert), cx)
    });
    redraw(cx);
    for name in [
        "table",
        "insert-image",
        "insert-code-block",
        "insert-math-block",
        "insert-separator",
        "insert-toc",
        "insert-front-matter",
    ] {
        menu_row(name, cx);
    }

    let rows: Vec<(&'static str, bool)> = editor.read_with(cx, |editor, cx| {
        editor
            .document_submenu_rows(DocumentSubmenu::Insert, cx)
            .into_iter()
            .filter_map(|row| match row {
                DocumentMenuRow::Item { name, enabled, .. } => Some((name, enabled)),
                _ => None,
            })
            .collect()
    });
    assert_eq!(
        rows.iter()
            .find(|(name, _)| *name == "insert-image")
            .map(|(_, enabled)| *enabled),
        Some(available(&editor, cx)),
        "菜单那一行的置灰口径要与 `image_insert_is_available` 同源：{rows:?}"
    );
    assert_eq!(
        rows.iter().position(|(name, _)| *name == "insert-image"),
        Some(1),
        "图片排在表格之后、其余五类块之前：{rows:?}"
    );
    assert!(
        cx.update(|_window, cx| document_menu_shortcut(DocumentMenuCommand::InsertImage, cx))
            .is_some(),
        "「插入 → 图片」这一行的快捷键列要给出生效键位"
    );
}
