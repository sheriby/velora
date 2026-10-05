//! 「插入」那一档：五类块从光标所在块后面（Front Matter 在文档最前面）落进缓冲区。
//!
//! 每条用例看四件事：缓冲区字节（只在插入点那一处动）、块树的种类与顺序、光标落点、
//! 一步撤销回到原样。鼠标只用来钉「菜单那一行走的是同一条入口」。

use super::common::*;
use crate::components::Block;
use crate::editor::context_menu::DocumentSubmenu;
use crate::editor::insert_ops::InsertBlockTarget;
use gpui::{Entity, Modifiers, MouseButton, point};

const ONE_PARAGRAPH: &str = "正文甲\n";

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

fn kinds(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |editor, cx| {
        editor
            .document
            .root_blocks()
            .iter()
            .map(|block| format!("{:?}", block.read(cx).kind()))
            .collect()
    })
}

fn put_caret(editor: &Entity<Editor>, index: usize, offset: usize, cx: &mut VisualTestContext) {
    let block = visible_block(editor, index, cx);
    editor.update(cx, |editor, _cx| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _cx| {
        block.selected_range = offset..offset;
    });
    redraw(cx);
}

fn insert(editor: &Entity<Editor>, target: InsertBlockTarget, cx: &mut VisualTestContext) -> bool {
    editor.update(cx, |editor, cx| {
        editor.insert_block_after_selection(target, cx)
    })
}

fn available(
    editor: &Entity<Editor>,
    target: InsertBlockTarget,
    cx: &mut VisualTestContext,
) -> bool {
    editor.read_with(cx, |editor, cx| {
        editor.insert_block_target_is_available(target, cx)
    })
}

fn caret_of(
    editor: &Entity<Editor>,
    index: usize,
    cx: &mut VisualTestContext,
) -> std::ops::Range<usize> {
    visible_block(editor, index, cx).read_with(cx, |block, _cx| block.selected_range.clone())
}

fn focused_entity(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<gpui::EntityId> {
    editor.read_with(cx, |editor, _| editor.active_entity_id)
}

/// 建窗口：这一档的用例要看真实布局（右键按块边界派发）。
fn open_editor<'a>(
    text: &'static str,
    cx: &'a mut TestAppContext,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    cx.add_window_view(|_window, cx| Editor::from_markdown(cx, text.to_string(), None))
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

fn row_bounds(name: &'static str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    let selector: &'static str = Box::leak(format!("menu-item-{name}").into_boxed_str());
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("菜单里没渲染出 {name} 这一行"))
}

fn click_row(name: &'static str, cx: &mut VisualTestContext) {
    let bounds = row_bounds(name, cx);
    let center = point(
        bounds.left() + bounds.size.width * 0.5,
        bounds.top() + bounds.size.height * 0.5,
    );
    cx.simulate_click(center, Modifiers::none());
    redraw(cx);
}

#[gpui::test]
async fn inserting_a_code_block_lands_below_the_current_block_and_undoes_as_a_step(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(ONE_PARAGRAPH, cx);
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(insert(&editor, InsertBlockTarget::CodeBlock, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "正文甲\n\n```\n\n```\n\n",
        "接缝空一行，围栏与正文那一行各归各位；原文那三个字节不动"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec![
            "Paragraph".to_string(),
            "CodeBlock { language: None }".to_string(),
            "Paragraph".to_string()
        ],
        "末尾那个空段落是给光标留的退路，插完就该有一块能打字的地方"
    );
    assert_eq!(
        focused_entity(&editor, cx),
        Some(visible_block(&editor, 1, cx).entity_id()),
        "光标该交给新插的那一块"
    );
    assert_eq!(caret_of(&editor, 1, cx), 0..0, "光标该落在围栏里那一行");

    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        ONE_PARAGRAPH,
        "一步撤销该把整次插入退回"
    );
    assert_eq!(kinds(&editor, cx), vec!["Paragraph".to_string()]);
}

#[gpui::test]
async fn inserting_a_math_block_puts_the_caret_after_the_opening_marker(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(ONE_PARAGRAPH, cx);
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(insert(&editor, InsertBlockTarget::MathBlock, cx));
    redraw(cx);

    assert_eq!(buffer_text(&editor, cx), "正文甲\n\n$$\n$$\n\n");
    assert_eq!(
        kinds(&editor, cx),
        vec![
            "Paragraph".to_string(),
            "MathBlock".to_string(),
            "Paragraph".to_string()
        ],
        "块树与写回的字节不一致"
    );
    assert_eq!(caret_of(&editor, 1, cx), 3..3, "光标该落在开栏之后那一行");
}

#[gpui::test]
async fn inserting_a_thematic_break_does_not_touch_the_paragraph_above(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(ONE_PARAGRAPH, cx);
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(insert(&editor, InsertBlockTarget::Separator, cx));
    redraw(cx);

    assert_eq!(buffer_text(&editor, cx), "正文甲\n\n---\n\n");
    assert_eq!(
        kinds(&editor, cx),
        vec![
            "Paragraph".to_string(),
            "Separator".to_string(),
            "Paragraph".to_string()
        ],
        "块树与写回的字节不一致"
    );
}

#[gpui::test]
async fn inserting_a_toc_writes_the_marker_line(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(ONE_PARAGRAPH, cx);
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    assert!(insert(&editor, InsertBlockTarget::Toc, cx));
    redraw(cx);

    assert_eq!(buffer_text(&editor, cx), "正文甲\n\n[toc]\n");
    assert_eq!(
        kinds(&editor, cx),
        vec!["Paragraph".to_string(), "Paragraph".to_string()],
        "目录那一行就是 `[toc]` 那个写法，条目由渲染层现算"
    );
    assert_eq!(
        visible_block(&editor, 1, cx).read_with(cx, |block, _cx| block.display_text().to_string()),
        "[toc]",
        "插进去的那块内容对不上"
    );
    // 段落不 strand 光标，不该多补一块退路。
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(buffer_text(&editor, cx), ONE_PARAGRAPH);
}

/// Front Matter 只能在文档最前面，而且一篇只能有一份。
#[gpui::test]
async fn front_matter_goes_to_the_very_top_and_only_once(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor("正文甲\n\n正文乙\n", cx);
    redraw(cx);

    put_caret(&editor, 1, 3, cx);
    assert!(insert(&editor, InsertBlockTarget::FrontMatter, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "---\n\n---\n\n正文甲\n\n正文乙\n",
        "顶到第一行，后面那两块的原样不动"
    );
    assert_eq!(
        kinds(&editor, cx),
        vec![
            "FrontMatter".to_string(),
            "Paragraph".to_string(),
            "Paragraph".to_string()
        ],
        "块树与写回的字节不一致"
    );
    assert_eq!(
        caret_of(&editor, 0, cx),
        4..4,
        "光标该落在两条 --- 中间那一行"
    );

    assert!(
        !available(&editor, InsertBlockTarget::FrontMatter, cx),
        "已经有 Front Matter 了，菜单那一行该置灰"
    );
    let before = buffer_text(&editor, cx);
    assert!(
        !insert(&editor, InsertBlockTarget::FrontMatter, cx),
        "第二份 Front Matter 不该插进去"
    );
    assert_eq!(buffer_text(&editor, cx), before, "被拒的插入动了字节");
}

/// 跨块选完两段再插：新块落在整段选区下面，不是在光标那一块中间。
#[gpui::test]
async fn an_insertion_lands_below_the_whole_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor("甲一\n\n乙二\n", cx);
    redraw(cx);

    let (first, last) = (visible_block(&editor, 0, cx), visible_block(&editor, 1, cx));
    editor.update(cx, |editor, _cx| {
        editor.cross_block_selection = Some(crate::editor::CrossBlockSelection {
            anchor: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: first.entity_id(),
                offset: 0,
            },
            focus: crate::editor::CrossBlockSelectionEndpoint {
                entity_id: last.entity_id(),
                offset: 2,
            },
        });
    });

    assert!(insert(&editor, InsertBlockTarget::Toc, cx));
    redraw(cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "甲一\n\n乙二\n\n[toc]\n",
        "目录该落在两段下面，两段自己的字节不动"
    );
}

/// 插入走的是区段写回：没碰过的那块 Setext 标题必须一个字节都不动。
/// 这条在钉「不是整篇重投影」——一旦退回重投影，`===` 会被拼成 `# `。
#[gpui::test]
async fn an_insertion_leaves_another_block_s_original_writing_alone(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor("标题甲\n===\n\n正文乙\n", cx);
    redraw(cx);

    put_caret(&editor, 1, 3, cx);
    assert!(insert(&editor, InsertBlockTarget::Toc, cx));
    redraw(cx);

    assert_eq!(
        buffer_text(&editor, cx),
        "标题甲\n===\n\n正文乙\n\n[toc]\n",
        "写回把没插过的那块也按模型的写法重排了"
    );
}

/// 菜单那一行与编辑器层入口是同一条：右键 → 插入 → 代码块，落点与字节一样。
#[gpui::test]
async fn the_insert_row_in_the_context_menu_reaches_the_same_entry(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = open_editor(ONE_PARAGRAPH, cx);
    redraw(cx);

    put_caret(&editor, 0, 3, cx);
    right_click(&editor, 0, cx);
    editor.update(cx, |editor, cx| {
        editor.set_document_menu_hover(true, Some(DocumentSubmenu::Insert), cx)
    });
    redraw(cx);
    for name in [
        "table",
        "insert-code-block",
        "insert-math-block",
        "insert-separator",
        "insert-toc",
        "insert-front-matter",
    ] {
        row_bounds(name, cx);
    }

    click_row("insert-toc", cx);
    assert_eq!(
        buffer_text(&editor, cx),
        "正文甲\n\n[toc]\n",
        "菜单里的「目录」没落到缓冲区"
    );
    assert!(
        !editor.read_with(cx, |editor, _| editor.context_menu.is_some()),
        "点完一行菜单该收起"
    );
}

/// 五类各插一次：写下去的字节重新读一遍，根块种类序列必须与块树一致。
#[gpui::test]
async fn every_inserted_block_reads_back_the_same_tree(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    for target in [
        InsertBlockTarget::CodeBlock,
        InsertBlockTarget::MathBlock,
        InsertBlockTarget::Separator,
        InsertBlockTarget::Toc,
        InsertBlockTarget::FrontMatter,
    ] {
        let (editor, cx2) = open_editor("正文甲\n", cx);
        redraw(cx2);
        put_caret(&editor, 0, 3, cx2);
        assert!(insert(&editor, target, cx2), "{target:?} 报没插进去");
        redraw(cx2);
        let text = buffer_text(&editor, cx2);
        let before = kinds(&editor, cx2);
        let reread = cx2.new(|cx| Editor::from_markdown(cx, text.clone(), None));
        let after = reread.read_with(cx2, |editor, cx| {
            editor
                .document
                .root_blocks()
                .iter()
                .map(|block| format!("{:?}", block.read(cx).kind()))
                .collect::<Vec<_>>()
        });
        assert_eq!(
            after, before,
            "{target:?} 写下去的字节读不回同一个结构：{text:?}"
        );
    }
}
