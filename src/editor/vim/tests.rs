//! vim 模式行为测试。按族组织：模式切换、动作、操作符、寄存器、
//! 可视、标记、宏、替换模式、搜索与命令行、键映射（jj）。

use super::*;
use crate::config::preferences::{
    EditorSettings, VimKeymapAction, VimKeymapEntry, VimKeymapMode, VimPreferences,
};
use crate::editor::tests::common::{init_editor_test_app, redraw, temp_fixture_dir};
use gpui::{Entity, TestAppContext, VisualTestContext};

/// 建一个开了 vim 的编辑器并完成首帧聚焦。配置文件落在独立临时根，
/// 避免读写开发者本机的 config.toml。
fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    markdown: &str,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    vim_editor_with(
        cx,
        markdown,
        VimPreferences {
            enable: true,
            ..Default::default()
        },
    )
}

fn vim_editor_with<'a>(
    cx: &'a mut TestAppContext,
    markdown: &str,
    preferences: VimPreferences,
) -> (Entity<Editor>, &'a mut VisualTestContext) {
    init_editor_test_app(cx);
    let root = temp_fixture_dir().join(format!("vim-config-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create vim test config root");
    {
        let _guard = crate::config::override_test_config_root(&root);
        cx.update(|cx| {
            EditorSettings::init(cx, true);
            EditorSettings::set_vim_in_memory(cx, preferences);
        });
    }
    let text = markdown.to_string();
    let (editor, cx) = cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, text, None));
    redraw(cx);
    redraw(cx);
    (editor, cx)
}

fn document_text(editor: &Entity<Editor>, cx: &VisualTestContext) -> String {
    editor.read_with(cx, |editor, _cx| editor.buffer.text().to_string())
}

fn current_mode(editor: &Entity<Editor>, cx: &VisualTestContext) -> VimMode {
    editor.read_with(cx, |editor, _cx| editor.vim.mode)
}

fn first_block_caret(editor: &Entity<Editor>, cx: &VisualTestContext) -> usize {
    editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .first()
            .expect("文档至少有一块")
            .entity
            .read(cx)
            .cursor_offset()
    })
}

#[gpui::test]
async fn vim_disabled_by_default_keeps_insert_semantics(cx: &mut gpui::TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "hello\n".to_string(), None)
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(!editor.vim_enabled);
        assert_eq!(editor.vim.mode, VimMode::Insert);
    });
}

#[gpui::test]
async fn enabling_vim_starts_in_normal_mode(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    assert_eq!(
        current_mode(&editor, &cx),
        VimMode::Normal,
        "开启 vim 的编辑器应以普通模式启动"
    );
}

#[gpui::test]
async fn i_enters_insert_and_escape_returns_to_normal(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("i");
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert);
    cx.simulate_input("hi ");
    assert_eq!(document_text(&editor, &cx), "hi alpha");
    cx.simulate_keystrokes("escape");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
    assert_eq!(
        first_block_caret(&editor, &cx),
        2,
        "Esc 后光标应左移一格停在插入的最后一个字符上"
    );
}

#[gpui::test]
async fn a_appends_after_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("l l a");
    cx.simulate_input("!");
    assert_eq!(document_text(&editor, &cx), "alp!ha");
}

#[gpui::test]
async fn h_and_l_move_within_the_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("l l l");
    assert_eq!(first_block_caret(&editor, &cx), 3);
    cx.simulate_keystrokes("h");
    assert_eq!(first_block_caret(&editor, &cx), 2);
}

#[gpui::test]
async fn count_repeats_a_motion(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdefgh");
    cx.simulate_keystrokes("3 l");
    assert_eq!(first_block_caret(&editor, &cx), 3, "3l 应前进三格");
}

#[gpui::test]
async fn x_deletes_the_character_under_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("x");
    assert_eq!(document_text(&editor, &cx), "lpha beta");
    assert_eq!(first_block_caret(&editor, &cx), 0);
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn count_multiplies_x(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("3 x");
    assert_eq!(
        document_text(&editor, &cx),
        "ha beta",
        "3x 应删除三个字符"
    );
}

#[gpui::test]
async fn dw_deletes_a_word(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta gamma");
    cx.simulate_keystrokes("d w");
    assert_eq!(document_text(&editor, &cx), "beta gamma");
}

#[gpui::test]
async fn dd_removes_the_current_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "first");
    cx.simulate_keystrokes("d d");
    assert_eq!(document_text(&editor, &cx), "", "dd 应删掉当前行");
}

#[gpui::test]
async fn jj_mapping_escapes_insert_mode_without_typing(cx: &mut TestAppContext) {
    let preferences = VimPreferences {
        enable: true,
        keymaps: vec![VimKeymapEntry {
            mode: VimKeymapMode::Insert,
            keys: "jj".into(),
            action: VimKeymapAction::Esc,
        }],
        ..Default::default()
    };
    let (editor, cx) = vim_editor_with(cx, "alpha", preferences);
    cx.simulate_keystrokes("i");
    cx.simulate_input("jj");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal, "jj 应退出插入模式");
    assert_eq!(document_text(&editor, &cx), "alpha", "映射键不能落进正文");
}
