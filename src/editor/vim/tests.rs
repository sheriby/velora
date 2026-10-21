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

fn visible_texts(editor: &Entity<Editor>, cx: &VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .map(|entry| entry.entity.read(cx).display_text().to_string())
            .collect()
    })
}

fn caret_in_block(editor: &Entity<Editor>, cx: &VisualTestContext, index: usize) -> usize {
    editor.read_with(cx, |editor, cx| {
        editor.document.visible_blocks()[index]
            .entity
            .read(cx)
            .cursor_offset()
    })
}

fn cursor_block(editor: &Entity<Editor>, cx: &VisualTestContext) -> usize {
    editor.read_with(cx, |editor, cx| {
        let active = editor.active_entity_id.expect("焦点块");
        editor
            .document
            .visible_blocks()
            .iter()
            .position(|entry| entry.entity.entity_id() == active)
            .expect("活动块在可见列表里")
    })
}

#[gpui::test]
async fn w_b_e_move_between_words(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three");
    cx.simulate_keystrokes("w");
    assert_eq!(first_block_caret(&editor, &cx), 4, "w 应落在下一个词首");
    cx.simulate_keystrokes("w");
    assert_eq!(first_block_caret(&editor, &cx), 8);
    cx.simulate_keystrokes("b");
    assert_eq!(first_block_caret(&editor, &cx), 4, "b 应回到词首");
    cx.simulate_keystrokes("e");
    assert_eq!(first_block_caret(&editor, &cx), 6, "e 应停在词尾");
}

#[gpui::test]
async fn zero_and_dollar_jump_to_line_edges(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three");
    cx.simulate_keystrokes("$");
    assert_eq!(first_block_caret(&editor, &cx), 12, "$ 应停在行尾");
    cx.simulate_keystrokes("0");
    assert_eq!(first_block_caret(&editor, &cx), 0, "0 应回到行首");
}

#[gpui::test]
async fn j_and_k_move_across_blocks(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    assert_eq!(cursor_block(&editor, &cx), 0);
    cx.simulate_keystrokes("j");
    assert_eq!(cursor_block(&editor, &cx), 1, "j 应下移一段");
    cx.simulate_keystrokes("j");
    assert_eq!(cursor_block(&editor, &cx), 2);
    cx.simulate_keystrokes("k");
    assert_eq!(cursor_block(&editor, &cx), 1, "k 应上移一段");
}

#[gpui::test]
async fn gg_and_capital_g_reach_document_edges(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("G");
    assert_eq!(cursor_block(&editor, &cx), 2, "G 应到文末");
    cx.simulate_keystrokes("g g");
    assert_eq!(cursor_block(&editor, &cx), 0, "gg 应回文首");
}

#[gpui::test]
async fn f_t_semicolon_and_comma_find_characters(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcabc");
    cx.simulate_keystrokes("f c");
    assert_eq!(first_block_caret(&editor, &cx), 2, "fc 应停在 c 上");
    cx.simulate_keystrokes(";");
    assert_eq!(first_block_caret(&editor, &cx), 5, "; 应重复查找");
    cx.simulate_keystrokes(",");
    assert_eq!(first_block_caret(&editor, &cx), 2, ", 应反向重复");
    cx.simulate_keystrokes("t a");
    assert_eq!(
        first_block_caret(&editor, &cx),
        2,
        "ta 应停在下一个 a 前一格"
    );
}

#[gpui::test]
async fn percent_jumps_between_matching_pairs(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "(abc)");
    cx.simulate_keystrokes("%");
    assert_eq!(first_block_caret(&editor, &cx), 4, "% 应跳到配对括号");
}

#[gpui::test]
async fn pipe_moves_to_a_column(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("4 |");
    assert_eq!(first_block_caret(&editor, &cx), 3, "4| 应到第 4 列（0 基 3）");
}

#[gpui::test]
async fn capital_i_and_a_insert_at_line_edges(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("A");
    cx.simulate_input(" end");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "alpha end");
    cx.simulate_keystrokes("I");
    cx.simulate_input("start ");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "start alpha end");
}

#[gpui::test]
async fn o_opens_a_new_block_below(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one");
    cx.simulate_keystrokes("o");
    cx.simulate_input("two");
    cx.simulate_keystrokes("escape");
    assert_eq!(visible_texts(&editor, &cx), vec!["one", "two"]);
    assert_eq!(cursor_block(&editor, &cx), 1, "o 后光标在新段");
}

#[gpui::test]
async fn enter_in_insert_mode_splits_the_block(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two");
    cx.simulate_keystrokes("A");
    cx.simulate_input(" more");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("next");
    cx.simulate_keystrokes("escape");
    assert_eq!(visible_texts(&editor, &cx), vec!["one two more", "next"]);
}

#[gpui::test]
async fn ctrl_w_deletes_the_previous_word_in_insert_mode(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("A");
    cx.simulate_keystrokes("ctrl-w");
    assert_eq!(document_text(&editor, &cx), "alpha ", "ctrl-w 应删掉一个词");
}

#[gpui::test]
async fn ctrl_u_deletes_to_line_start_in_insert_mode(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("A");
    cx.simulate_keystrokes("ctrl-u");
    assert_eq!(document_text(&editor, &cx), "", "ctrl-u 应删到行首");
}

#[gpui::test]
async fn ctrl_o_runs_one_normal_command_then_returns_to_insert(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two");
    cx.simulate_keystrokes("i");
    cx.simulate_keystrokes("ctrl-o");
    cx.simulate_keystrokes("w");
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert, "ctrl-o 后应回到插入");
    assert_eq!(first_block_caret(&editor, &cx), 4, "w 应已把光标移到下一个词");
}
