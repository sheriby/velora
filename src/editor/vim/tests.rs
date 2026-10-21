//! vim 模式行为测试。按族组织：模式切换、动作、操作符、寄存器、
//! 可视、标记、宏、替换模式、搜索与命令行、键映射（jj）。

use super::*;
use crate::config::preferences::{
    EditorSettings, VimKeymapAction, VimKeymapEntry, VimKeymapMode, VimPreferences,
};
use crate::editor::tests::common::{init_editor_test_app, redraw, temp_fixture_dir};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};

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
            .collect::<Vec<_>>()
    })
}

fn cursor_block(editor: &Entity<Editor>, cx: &VisualTestContext) -> usize {
    editor.read_with(cx, |editor, _cx| {
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

#[gpui::test]
async fn dd_removes_the_whole_block_and_keeps_the_next(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("d d");
    assert_eq!(visible_texts(&editor, &cx), vec!["two"]);
}

#[gpui::test]
async fn count_dd_removes_several_blocks(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("2 d d");
    assert_eq!(visible_texts(&editor, &cx), vec!["three"]);
}

#[gpui::test]
async fn cc_clears_the_line_and_enters_insert(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("c c");
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert);
    cx.simulate_input("beta");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "beta");
}

#[gpui::test]
async fn yy_then_p_duplicates_the_block(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one");
    cx.simulate_keystrokes("y y p");
    assert_eq!(visible_texts(&editor, &cx), vec!["one", "one"]);
}

#[gpui::test]
async fn capital_d_deletes_to_the_end_of_the_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("w D");
    assert_eq!(document_text(&editor, &cx), "alpha ");
}

#[gpui::test]
async fn capital_c_changes_to_the_end_of_the_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("w C");
    cx.simulate_input("BETA");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "alpha BETA");
}

#[gpui::test]
async fn s_substitutes_the_character_under_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("s");
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "Xlpha");
}

#[gpui::test]
async fn capital_s_clears_the_line_and_enters_insert(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("S");
    cx.simulate_input("beta");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "beta");
}

#[gpui::test]
async fn r_replaces_one_character_and_count_repeats(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcd");
    cx.simulate_keystrokes("r z");
    assert_eq!(document_text(&editor, &cx), "zbcd");
    cx.simulate_keystrokes("3 r y");
    assert_eq!(
        document_text(&editor, &cx),
        "yyyd",
        "3ry 应从光标起替换三个字符"
    );
}

#[gpui::test]
async fn tilde_swaps_the_case_of_the_character_under_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "aBc");
    cx.simulate_keystrokes("~");
    assert_eq!(document_text(&editor, &cx), "ABc");
    assert_eq!(first_block_caret(&editor, &cx), 1, "~ 后光标右移一格");
    cx.simulate_keystrokes("~");
    assert_eq!(document_text(&editor, &cx), "Abc", "大写字符应转小写");
}

#[gpui::test]
async fn capital_j_joins_the_next_block_with_a_space(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("J");
    assert_eq!(visible_texts(&editor, &cx), vec!["one two"]);
}

#[gpui::test]
async fn g_capital_j_joins_without_inserting_a_space(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("g J");
    assert_eq!(visible_texts(&editor, &cx), vec!["onetwo"]);
}

#[gpui::test]
async fn diw_deletes_the_inner_word(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("w");
    cx.simulate_keystrokes("d i w");
    assert_eq!(document_text(&editor, &cx), "alpha ");
}

#[gpui::test]
async fn daw_deletes_the_word_and_its_separator(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("w");
    cx.simulate_keystrokes("d a w");
    assert_eq!(document_text(&editor, &cx), "alpha");
}

#[gpui::test]
async fn ciw_changes_the_inner_word(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("c i w");
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "X beta");
}

#[gpui::test]
async fn di_quote_deletes_inside_quotes(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "say \"hello\" now");
    cx.simulate_keystrokes("f \"");
    cx.simulate_keystrokes("l");
    cx.simulate_keystrokes("d i \"");
    assert_eq!(document_text(&editor, &cx), "say \"\" now");
}

#[gpui::test]
async fn da_paren_deletes_around_parentheses(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "(abc)");
    cx.simulate_keystrokes("d a (");
    assert_eq!(document_text(&editor, &cx), "");
}

#[gpui::test]
async fn p_pastes_characterwise_after_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "ab");
    cx.simulate_keystrokes("x p");
    assert_eq!(document_text(&editor, &cx), "ba", "p 应粘在光标字符之后");
}

#[gpui::test]
async fn capital_p_pastes_before_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "ab");
    cx.simulate_keystrokes("x P");
    assert_eq!(document_text(&editor, &cx), "ab");
}

#[gpui::test]
async fn d_dollar_and_d_zero_clamp_to_the_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("w d $");
    assert_eq!(document_text(&editor, &cx), "alpha ");
}

#[gpui::test]
async fn character_visual_delete_removes_the_selection(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l l");
    assert!(matches!(current_mode(&editor, &cx), VimMode::Visual(VisualKind::Char)));
    cx.simulate_keystrokes("d");
    assert_eq!(document_text(&editor, &cx), "def");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn line_visual_delete_removes_the_whole_block(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("V d");
    assert_eq!(visible_texts(&editor, &cx), vec!["two"]);
}

#[gpui::test]
async fn character_visual_change_replaces_the_selection(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l l c");
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert);
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "Xdef");
}

#[gpui::test]
async fn visual_yank_then_paste_repeats_the_selection(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l l y");
    cx.simulate_keystrokes("$");
    cx.simulate_keystrokes("p");
    assert_eq!(document_text(&editor, &cx), "abcdefabc");
}

#[gpui::test]
async fn escape_leaves_visual_mode(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l");
    assert!(matches!(current_mode(&editor, &cx), VimMode::Visual(_)));
    cx.simulate_keystrokes("escape");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
    assert_eq!(first_block_caret(&editor, &cx), 1, "退出可视后光标留在选区头");
}

#[gpui::test]
async fn o_swaps_the_visual_anchor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l l o");
    cx.simulate_keystrokes("d");
    assert_eq!(document_text(&editor, &cx), "def", "o 只交换端点，区间不变");
}

#[gpui::test]
async fn gv_reselects_the_last_visual_range(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l l escape");
    cx.simulate_keystrokes("g v");
    assert!(matches!(current_mode(&editor, &cx), VimMode::Visual(_)));
    cx.simulate_keystrokes("d");
    assert_eq!(document_text(&editor, &cx), "def");
}

#[gpui::test]
async fn named_register_yank_and_paste(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("\" a y y");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("\" a p");
    assert_eq!(visible_texts(&editor, &cx), vec!["one", "two", "one"]);
}

#[gpui::test]
async fn blackhole_register_keeps_the_unnamed_register(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("y y");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("\" _ d d");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["one", ""],
        "dd 掉末行后保留一个空行"
    );
    cx.simulate_keystrokes("p");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["one", "", "one"],
        "_ 删除不应覆盖无名寄存器"
    );
}

#[gpui::test]
async fn mark_and_backtick_restore_the_position(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three");
    cx.simulate_keystrokes("w");
    cx.simulate_keystrokes("m a");
    cx.simulate_keystrokes("w");
    assert_eq!(first_block_caret(&editor, &cx), 8);
    cx.simulate_keystrokes("` a");
    assert_eq!(first_block_caret(&editor, &cx), 4, "`a 应回到标记位置");
}

#[gpui::test]
async fn mark_linewise_jump_lands_on_first_non_blank(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("m a");
    cx.simulate_keystrokes("j");
    assert_eq!(cursor_block(&editor, &cx), 1);
    cx.simulate_keystrokes("' a");
    assert_eq!(cursor_block(&editor, &cx), 0, "'a 应回到标记所在行");
}

#[gpui::test]
async fn jumplist_back_returns_after_document_jump(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("G");
    assert_eq!(cursor_block(&editor, &cx), 2);
    cx.simulate_keystrokes("ctrl-o");
    assert_eq!(cursor_block(&editor, &cx), 0, "ctrl-o 应回跳到 G 之前");
}

#[gpui::test]
async fn macro_records_and_replays_keys(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("q a x q");
    assert_eq!(document_text(&editor, &cx), "bcdef");
    cx.simulate_keystrokes("@ a");
    assert_eq!(document_text(&editor, &cx), "cdef", "@a 应重放 x");
}

#[gpui::test]
async fn at_at_replays_the_last_macro(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("q a x q");
    cx.simulate_keystrokes("@ a");
    assert_eq!(document_text(&editor, &cx), "cdef");
    cx.simulate_keystrokes("@ @");
    assert_eq!(document_text(&editor, &cx), "def");
}

#[gpui::test]
async fn count_replays_a_macro_multiple_times(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("q a x q");
    cx.simulate_keystrokes("2 @ a");
    assert_eq!(
        document_text(&editor, &cx),
        "def",
        "录制时已删一个字符，2@a 再删两个"
    );
}

#[gpui::test]
async fn dot_repeats_the_last_change(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("x .");
    assert_eq!(document_text(&editor, &cx), "cdef");
}

#[gpui::test]
async fn dot_repeats_a_delete_operator(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three");
    cx.simulate_keystrokes("d w .");
    assert_eq!(document_text(&editor, &cx), "three");
}

#[gpui::test]
async fn undo_and_redo_restore_text(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abc");
    cx.simulate_keystrokes("x");
    assert_eq!(document_text(&editor, &cx), "bc");
    cx.simulate_keystrokes("u");
    assert_eq!(document_text(&editor, &cx), "abc", "u 应撤销删除");
    cx.simulate_keystrokes("ctrl-r");
    assert_eq!(document_text(&editor, &cx), "bc", "ctrl-r 应重做删除");
}

#[gpui::test]
async fn replace_mode_overtypes_without_shifting(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("R");
    assert_eq!(current_mode(&editor, &cx), VimMode::Replace);
    cx.simulate_input("XY");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "XYcdef");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn replace_mode_backspace_restores_the_original_character(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abc");
    cx.simulate_keystrokes("R");
    cx.simulate_input("X");
    assert_eq!(document_text(&editor, &cx), "Xbc");
    cx.simulate_keystrokes("backspace");
    assert_eq!(document_text(&editor, &cx), "abc", "退格还原原字符");
    cx.simulate_keystrokes("escape");
}

#[gpui::test]
async fn replace_mode_stops_at_the_end_of_the_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "ab");
    cx.simulate_keystrokes("R");
    cx.simulate_input("XYZ");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "XY", "替换不越过行尾");
}

#[gpui::test]
async fn search_finds_and_n_repeats_forward(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three two");
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    let first = editor.read_with(&*cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx).range.start
    });
    assert_eq!(first, 4, "首个命中应落在第一个 two");
    cx.simulate_keystrokes("n");
    let second = editor.read_with(&*cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx).range.start
    });
    assert_eq!(second, 14, "n 应跳到下一个 two");
}

#[gpui::test]
async fn capital_n_repeats_search_backward(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three two");
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter n N");
    let position = editor.read_with(&*cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx).range.start
    });
    assert_eq!(position, 4, "N 应反向回到第一个 two");
}

#[gpui::test]
async fn star_matches_whole_words_only(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "two twothree two");
    cx.simulate_keystrokes("*");
    let position = editor.read_with(&*cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx).range.start
    });
    assert_eq!(position, 13, "* 应跳过 twothree 中的子串");
}

#[gpui::test]
async fn g_star_matches_substrings(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "two twothree two");
    cx.simulate_keystrokes("g *");
    let position = editor.read_with(&*cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx).range.start
    });
    assert_eq!(position, 4, "g* 应允许匹配词内子串");
}

#[gpui::test]
async fn star_searches_the_word_under_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three two");
    cx.simulate_keystrokes("w *");
    let position = editor.read_with(&*cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx).range.start
    });
    assert_eq!(position, 14, "* 应搜索光标下的词并跳到下一个命中");
}

#[gpui::test]
async fn substitute_replaces_on_the_current_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three two");
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/two/TWO/");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(document_text(&editor, &cx), "one TWO three two");
}

#[gpui::test]
async fn percent_substitute_replaces_every_match(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three two");
    cx.simulate_keystrokes(":");
    cx.simulate_input("%s/two/TWO/g");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(document_text(&editor, &cx), "one TWO three TWO");
}

#[gpui::test]
async fn nohl_clears_the_search_highlight_flag(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three two");
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    assert!(editor.read_with(&*cx, |editor, _| editor.vim.hlsearch));
    cx.simulate_keystrokes(":");
    cx.simulate_input("noh");
    cx.simulate_keystrokes("enter");
    assert!(!editor.read_with(&*cx, |editor, _| editor.vim.hlsearch));
}

#[gpui::test]
async fn normal_mode_mapping_can_enter_visual(cx: &mut TestAppContext) {
    let preferences = VimPreferences {
        enable: true,
        keymaps: vec![VimKeymapEntry {
            mode: VimKeymapMode::Normal,
            keys: "gh".into(),
            action: VimKeymapAction::Visual,
        }],
        ..Default::default()
    };
    let (editor, cx) = vim_editor_with(cx, "alpha", preferences);
    cx.simulate_keystrokes("g h");
    assert!(matches!(
        current_mode(&editor, &cx),
        VimMode::Visual(VisualKind::Char)
    ));
    assert_eq!(document_text(&editor, &cx), "alpha", "映射键不能落进正文");
}

#[gpui::test]
async fn visual_mode_mapping_can_escape(cx: &mut TestAppContext) {
    let preferences = VimPreferences {
        enable: true,
        keymaps: vec![VimKeymapEntry {
            mode: VimKeymapMode::Visual,
            keys: "Q".into(),
            action: VimKeymapAction::Esc,
        }],
        ..Default::default()
    };
    let (editor, cx) = vim_editor_with(cx, "alpha", preferences);
    cx.simulate_keystrokes("v");
    assert!(matches!(current_mode(&editor, &cx), VimMode::Visual(_)));
    cx.simulate_keystrokes("Q");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn insert_mapping_times_out_and_types_the_pending_key(cx: &mut TestAppContext) {
    let preferences = VimPreferences {
        enable: true,
        keymaps: vec![VimKeymapEntry {
            mode: VimKeymapMode::Insert,
            keys: "jk".into(),
            action: VimKeymapAction::Esc,
        }],
        ..Default::default()
    };
    let (editor, cx) = vim_editor_with(cx, "alpha", preferences);
    cx.simulate_keystrokes("i");
    cx.simulate_input("j");
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert);
    assert_eq!(
        document_text(&editor, &cx),
        "jalpha",
        "超时后缓冲键应作为普通文本落下"
    );
}

#[gpui::test]
async fn shift_right_indents_the_line_and_moves_to_first_non_blank(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("> >");
    assert_eq!(document_text(&editor, &cx), "    alpha");
    assert_eq!(first_block_caret(&editor, &cx), 4, ">> 后光标到首个非空白");
}

#[gpui::test]
async fn shift_left_removes_one_indent_level(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("> >");
    assert_eq!(document_text(&editor, &cx), "    alpha");
    cx.simulate_keystrokes("< <");
    assert_eq!(document_text(&editor, &cx), "alpha");
}

#[gpui::test]
async fn guu_lowercases_the_current_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "MiXeD");
    cx.simulate_keystrokes("g u u");
    assert_eq!(document_text(&editor, &cx), "mixed");
}

#[gpui::test]
async fn g_upper_u_uppercases_the_current_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "mixed");
    cx.simulate_keystrokes("g U U");
    assert_eq!(document_text(&editor, &cx), "MIXED");
}

#[gpui::test]
async fn g_tilde_swaps_case_over_a_motion(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "aBcD");
    cx.simulate_keystrokes("g ~ w");
    assert_eq!(document_text(&editor, &cx), "AbCd");
}

#[gpui::test]
async fn capital_g_i_inserts_before_the_first_column(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "  alpha");
    cx.simulate_keystrokes("g I");
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert);
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "X  alpha");
}

#[gpui::test]
async fn g_i_returns_to_the_last_insert_position(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("i");
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("$");
    cx.simulate_keystrokes("g i");
    cx.simulate_input("Y");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "XYabcdef", "gi 回到上次插入结束处");
}

#[gpui::test]
async fn colon_line_number_jumps_to_that_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "a\nb\nc");
    cx.simulate_keystrokes(":");
    cx.simulate_input("3");
    cx.simulate_keystrokes("enter");
    let position = editor.read_with(&*cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx).range.start
    });
    assert_eq!(position, 4, ":3 应跳到第三行行首");
}

#[gpui::test]
async fn block_visual_delete_removes_the_selected_column(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```\nab\ncd\n```");
    assert_eq!(visible_texts(&editor, &cx), vec!["ab\ncd"]);
    cx.simulate_keystrokes("ctrl-q");
    assert!(matches!(
        current_mode(&editor, &cx),
        VimMode::Visual(VisualKind::Block)
    ));
    cx.simulate_keystrokes("j d");
    assert_eq!(visible_texts(&editor, &cx), vec!["b\nd"]);
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn block_visual_insert_replicates_typed_text_on_every_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```\nab\ncd\n```");
    cx.simulate_keystrokes("ctrl-q j I");
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert);
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(visible_texts(&editor, &cx), vec!["Xab\nXcd"]);
}

#[gpui::test]
async fn disabled_vim_key_path_returns_before_touching_the_document(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor_with(
        cx,
        "alpha",
        VimPreferences {
            enable: false,
            ..Default::default()
        },
    );
    let before = editor.read_with(&*cx, |editor, _| {
        (
            editor.source_serializations.get(),
            editor.source_mapping_builds.get(),
            editor.word_count_scans.get(),
            editor.row_plan_rebuilds.get(),
        )
    });
    let keystroke = gpui::Keystroke::parse("x").expect("keystroke");
    editor.update_in(cx, |editor, window, cx| {
        assert!(
            !editor.vim_key_down(&keystroke, window, cx),
            "vim 关闭时按键必须直通"
        );
    });
    let after = editor.read_with(&*cx, |editor, _| {
        (
            editor.source_serializations.get(),
            editor.source_mapping_builds.get(),
            editor.word_count_scans.get(),
            editor.row_plan_rebuilds.get(),
        )
    });
    assert_eq!(after, before, "vim 关闭时按键路径不应做任何全文工作");
}

#[gpui::test]
async fn vim_motions_cost_no_more_than_normal_cursor_moves(cx: &mut TestAppContext) {
    let counters = |editor: &Entity<Editor>, cx: &VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            (
                editor.source_serializations.get(),
                editor.source_mapping_builds.get(),
                editor.row_plan_rebuilds.get(),
            )
        })
    };
    let delta = |before: (u64, u64, u64), after: (u64, u64, u64)| {
        (after.0 - before.0, after.1 - before.1, after.2 - before.2)
    };

    // 基线：不开 vim，用方向键走六步。
    let (baseline_editor, baseline_cx) = vim_editor_with(
        cx,
        "alpha beta gamma",
        VimPreferences { enable: false, ..Default::default() },
    );
    let baseline_before = counters(&baseline_editor, baseline_cx);
    baseline_cx.simulate_keystrokes("right right right right right right");
    let baseline = delta(baseline_before, counters(&baseline_editor, baseline_cx));

    // vim：同样的六次光标移动。
    let (editor, cx) = vim_editor(cx, "alpha beta gamma");
    let before = counters(&editor, &cx);
    cx.simulate_keystrokes("l l w b $ 0");
    let vim = delta(before, counters(&editor, &cx));

    assert_eq!(vim.0, 0, "普通模式移动不能触发整篇序列化");
    assert_eq!(vim.2, 0, "普通模式移动不能触发行计划重建");
    assert_eq!(
        vim.1, baseline.1,
        "vim 移动的映射重建次数不应超过普通方向键移动"
    );
}

#[gpui::test]
async fn ctrl_a_and_ctrl_x_increment_the_number_under_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "count 41");
    cx.simulate_keystrokes("w");
    assert_eq!(first_block_caret(&editor, &cx), 6, "w 应停在数字首位");
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(document_text(&editor, &cx), "count 42");
    assert_eq!(
        first_block_caret(&editor, &cx),
        7,
        "自增后光标应停在数字末位"
    );
    cx.simulate_keystrokes("ctrl-x");
    assert_eq!(document_text(&editor, &cx), "count 41");
}

#[gpui::test]
async fn ctrl_a_keeps_zero_padding_and_applies_count(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "step 007");
    cx.simulate_keystrokes("w 3 ctrl-a");
    assert_eq!(document_text(&editor, &cx), "step 010", "三位零填充应保留");
}

#[gpui::test]
async fn ctrl_a_skips_to_the_next_number_when_cursor_is_past_one(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "a 1 b 20");
    // 光标停在 `1` 之后的 `b` 上，应改后面的 20。
    cx.simulate_keystrokes("w w");
    assert_eq!(first_block_caret(&editor, &cx), 4);
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(document_text(&editor, &cx), "a 1 b 21");
}

#[gpui::test]
async fn ctrl_f_and_ctrl_b_scroll_by_a_page(cx: &mut TestAppContext) {
    let markdown = (0..200)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let (editor, cx) = vim_editor(cx, &markdown);
    cx.update(|window, _cx| {
        window.resize(gpui::size(gpui::px(600.0), gpui::px(400.0)))
    });
    for _ in 0..4 {
        redraw(cx);
    }
    let offset = |editor: &Entity<Editor>, cx: &VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y)
    };
    let before = offset(&editor, &cx);
    cx.simulate_keystrokes("ctrl-f");
    redraw(cx);
    let after_page_down = offset(&editor, &cx);
    let viewport = editor.read_with(cx, |editor, _| {
        (editor.scroll_handle.bounds().size.height, editor.scroll_handle.max_offset().y)
    });
    assert!(
        after_page_down < before,
        "ctrl-f 应向下翻页：{before} -> {after_page_down}，视口 {viewport:?}"
    );
    cx.simulate_keystrokes("ctrl-b");
    redraw(cx);
    let after_page_up = offset(&editor, &cx);
    assert!(
        after_page_up > after_page_down,
        "ctrl-b 应向上翻页：{after_page_down} -> {after_page_up}"
    );
}

#[gpui::test]
async fn ctrl_v_enters_block_visual(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("ctrl-v");
    assert_eq!(
        current_mode(&editor, &cx),
        VimMode::Visual(VisualKind::Block),
        "ctrl-v 应进入块状可视"
    );
}

#[gpui::test]
async fn unicode_word_motions_and_line_end_edits_stay_on_graphemes(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "你好 世界");
    cx.simulate_keystrokes("w");
    assert_eq!(
        first_block_caret(&editor, &cx),
        7,
        "w 应停在第二个中文词的起点"
    );
    cx.simulate_keystrokes("b");
    assert_eq!(first_block_caret(&editor, &cx), 0);

    cx.simulate_keystrokes("$");
    assert_eq!(
        first_block_caret(&editor, &cx),
        10,
        "$ 应停在最后一个字素的起点"
    );
    cx.simulate_keystrokes("x");
    assert_eq!(document_text(&editor, &cx), "你好 世");
    assert_eq!(
        first_block_caret(&editor, &cx),
        7,
        "删掉末字后光标回落到新末字"
    );
}

#[gpui::test]
async fn ctrl_a_after_unicode_text_increments_the_following_number(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "编号 41");
    cx.simulate_keystrokes("w");
    assert_eq!(first_block_caret(&editor, &cx), 7, "w 应跳过中文词与空格");
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(document_text(&editor, &cx), "编号 42");
}

#[gpui::test]
async fn count_delete_stops_at_the_end_of_the_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("$ 3 x");
    assert_eq!(
        document_text(&editor, &cx),
        "abcde",
        "行尾的 3x 只删当前字素，不越过换行"
    );
}

#[gpui::test]
async fn ctrl_v_switches_visual_kinds_and_exits(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("v");
    assert_eq!(current_mode(&editor, &cx), VimMode::Visual(VisualKind::Char));
    cx.simulate_keystrokes("ctrl-v");
    assert_eq!(
        current_mode(&editor, &cx),
        VimMode::Visual(VisualKind::Block),
        "字符可视里 ctrl-v 应切到块状可视"
    );
    cx.simulate_keystrokes("ctrl-v");
    assert_eq!(
        current_mode(&editor, &cx),
        VimMode::Normal,
        "块状可视里再按 ctrl-v 应退出可视"
    );
}

#[gpui::test]
async fn g0_returns_to_the_hard_line_start(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("$");
    assert_eq!(first_block_caret(&editor, &cx), 4);
    cx.simulate_keystrokes("g 0");
    assert_eq!(first_block_caret(&editor, &cx), 0);
}

#[gpui::test]
async fn capital_u_is_not_bound_to_redo(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("x");
    assert_eq!(document_text(&editor, &cx), "lpha");
    cx.simulate_keystrokes("u");
    assert_eq!(document_text(&editor, &cx), "alpha");
    cx.simulate_keystrokes("U");
    assert_eq!(
        document_text(&editor, &cx),
        "alpha",
        "U 不是重做键，不应恢复已撤销的删除"
    );
}

#[gpui::test]
async fn visual_cmdline_substitute_applies_to_the_selected_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three two");
    cx.simulate_keystrokes("v l l");
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/two/TWO/g");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(
        document_text(&editor, &cx),
        "one TWO three TWO",
        "可视选区按行扩展，替换覆盖整行"
    );
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn visual_cmdline_substitute_covers_all_selected_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two\n\nthree two");
    cx.simulate_keystrokes("v j");
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/two/TWO/g");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(
        document_text(&editor, &cx),
        "one TWO\n\nthree TWO",
        "跨块可视选区应覆盖两端所在的行"
    );
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn normal_mapping_timeout_flushes_the_pending_motion(cx: &mut TestAppContext) {
    let preferences = VimPreferences {
        enable: true,
        keymaps: vec![VimKeymapEntry {
            mode: VimKeymapMode::Normal,
            keys: "jk".into(),
            action: VimKeymapAction::Insert,
        }],
        ..Default::default()
    };
    let (editor, cx) = vim_editor_with(cx, "alpha\n\nbeta", preferences);
    cx.simulate_keystrokes("j");
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    redraw(cx);
    assert_eq!(
        cursor_block(&editor, &cx),
        1,
        "映射前缀单键超时后应作为普通 j 下移"
    );
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn d_j_deletes_the_current_and_next_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("d j");
    assert_eq!(visible_texts(&editor, &cx), vec!["three"], "dj 应删当前行与下一行");
}

#[gpui::test]
async fn d_k_deletes_the_current_and_previous_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("d k");
    assert_eq!(visible_texts(&editor, &cx), vec!["three"], "dk 应删当前行与上一行");
}

#[gpui::test]
async fn capital_g_after_an_operator_deletes_to_the_document_end(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("d G");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["one", ""],
        "dG 应删到文末；删除整行后编辑器保留一个空块"
    );
}

#[gpui::test]
async fn capital_g_after_yank_yanks_through_the_last_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("y G");
    cx.simulate_keystrokes("G p");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["one", "two", "one", "two"],
        "yG 应把光标行到文末整段放进寄存器"
    );
}

#[gpui::test]
async fn g_g_after_an_operator_reaches_the_document_start(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("d g g");
    assert_eq!(visible_texts(&editor, &cx), vec!["three"], "dgg 应删到文首");
}

#[gpui::test]
async fn visual_p_replaces_the_character_selection(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l y");
    cx.simulate_keystrokes("l l");
    cx.simulate_keystrokes("v l");
    cx.simulate_keystrokes("p");
    assert_eq!(
        document_text(&editor, &cx),
        "ababef",
        "可视 p 应用寄存器内容替换选中的 cd"
    );
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn visual_line_p_replaces_the_selected_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("y y");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("V p");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["one", "one", "three"],
        "行可视 p 应用整行内容替换选中行"
    );
}

#[gpui::test]
async fn ys_iw_wraps_the_word_in_quotes(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("y s i w \"");
    assert_eq!(document_text(&editor, &cx), "\"alpha\" beta");
}

#[gpui::test]
async fn ys_iw_wraps_the_word_in_parentheses(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("y s i w )");
    assert_eq!(document_text(&editor, &cx), "(alpha) beta");
}

#[gpui::test]
async fn ys_s_wraps_the_current_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("y s s \"");
    assert_eq!(document_text(&editor, &cx), "\"alpha\"");
}

#[gpui::test]
async fn ys_dollar_wraps_to_the_line_end(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("y s $ \"");
    assert_eq!(document_text(&editor, &cx), "\"alpha beta\"");
}

#[gpui::test]
async fn ds_removes_the_surrounding_quotes(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "\"alpha\" beta");
    cx.simulate_keystrokes("l d s \"");
    assert_eq!(document_text(&editor, &cx), "alpha beta");
}

#[gpui::test]
async fn cs_changes_the_surrounding_quotes(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "\"alpha\" beta");
    cx.simulate_keystrokes("l c s \" '");
    assert_eq!(document_text(&editor, &cx), "'alpha' beta");
}

#[gpui::test]
async fn visual_s_wraps_the_selection(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l S \"");
    assert_eq!(document_text(&editor, &cx), "\"ab\"cdef");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn dot_repeats_the_surround_command(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("y s i w \"");
    cx.simulate_keystrokes("w w .");
    assert_eq!(document_text(&editor, &cx), "\"alpha\" \"beta\"");
}

#[gpui::test]
async fn undo_removes_the_added_surround(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("y s i w \"");
    cx.simulate_keystrokes("u");
    assert_eq!(document_text(&editor, &cx), "alpha beta");
}

#[gpui::test]
async fn ds_without_a_surrounding_pair_keeps_the_text(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("d s \"");
    assert_eq!(document_text(&editor, &cx), "alpha beta");
}

#[gpui::test]
async fn block_visual_p_pastes_into_each_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\nab\ncd\nef\n```");
    cx.simulate_keystrokes("y l");
    cx.simulate_keystrokes("ctrl-v j l p");
    assert_eq!(visible_texts(&editor, &cx), vec!["a\na\nef"]);
}

#[gpui::test]
async fn block_visual_s_wraps_each_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\nab\ncd\nef\n```");
    cx.simulate_keystrokes("ctrl-v j l S \"");
    assert_eq!(visible_texts(&editor, &cx), vec!["\"ab\"\n\"cd\"\nef"]);
}

#[gpui::test]
async fn block_visual_p_undo_restores_the_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\nab\ncd\nef\n```");
    cx.simulate_keystrokes("y l");
    cx.simulate_keystrokes("ctrl-v j l p");
    cx.simulate_keystrokes("u");
    assert_eq!(visible_texts(&editor, &cx), vec!["ab\ncd\nef"]);
}

#[gpui::test]
async fn dot_repeats_visual_surround(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l S \"");
    assert_eq!(document_text(&editor, &cx), "\"ab\"cdef");
    cx.simulate_keystrokes("l l l .");
    assert_eq!(document_text(&editor, &cx), "\"ab\"\"cd\"ef");
}

#[gpui::test]
async fn dot_repeats_visual_comment_toggle(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\nab\ncd\nef\ngh\n```");
    cx.simulate_keystrokes("V j g c");
    assert_eq!(visible_texts(&editor, &cx), vec!["// ab\n// cd\nef\ngh"]);
    cx.simulate_keystrokes("j j .");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["// ab\n// cd\n// ef\n// gh"]
    );
}

#[gpui::test]
async fn dot_repeats_visual_character_delete(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("v l d");
    assert_eq!(document_text(&editor, &cx), "cdef");
    cx.simulate_keystrokes(".");
    assert_eq!(document_text(&editor, &cx), "ef", "可视删除的 . 应重建同宽选区");
}

#[gpui::test]
async fn dot_repeats_visual_line_delete(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("V d");
    assert_eq!(visible_texts(&editor, &cx), vec!["two", "three"]);
    cx.simulate_keystrokes(".");
    assert_eq!(visible_texts(&editor, &cx), vec!["three"]);
}

#[gpui::test]
async fn dot_repeats_visual_line_change_with_insert(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("V c");
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(visible_texts(&editor, &cx), vec!["X", "two"]);
    cx.simulate_keystrokes("j .");
    assert_eq!(visible_texts(&editor, &cx), vec!["X", "X"]);
}

#[gpui::test]
async fn j_moves_between_lines_inside_a_code_block(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\nab\ncd\n```");
    cx.simulate_keystrokes("j");
    assert_eq!(first_block_caret(&editor, &cx), 3, "代码块内 j 应下移一硬行");
}

#[gpui::test]
async fn gcc_comments_and_uncomments_a_rust_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\nlet a = 1;\n```");
    cx.simulate_keystrokes("g c c");
    assert_eq!(visible_texts(&editor, &cx), vec!["// let a = 1;"]);
    cx.simulate_keystrokes("g c c");
    assert_eq!(visible_texts(&editor, &cx), vec!["let a = 1;"]);
}

#[gpui::test]
async fn gcc_uses_the_python_comment_token(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```python\n    print(1)\n```");
    cx.simulate_keystrokes("g c c");
    assert_eq!(visible_texts(&editor, &cx), vec!["    # print(1)"]);
}

#[gpui::test]
async fn gcc_is_a_noop_outside_code_blocks(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("g c c");
    assert_eq!(document_text(&editor, &cx), "alpha beta");
}

#[gpui::test]
async fn gc_motion_comments_the_covered_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\nab\ncd\nef\n```");
    cx.simulate_keystrokes("g c j");
    assert_eq!(visible_texts(&editor, &cx), vec!["// ab\n// cd\nef"]);
}

#[gpui::test]
async fn visual_gc_comments_the_selected_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\nab\ncd\n```");
    cx.simulate_keystrokes("V j g c");
    assert_eq!(visible_texts(&editor, &cx), vec!["// ab\n// cd"]);
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn dot_repeats_gcc(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\na\nb\n```");
    cx.simulate_keystrokes("g c c");
    cx.simulate_keystrokes("j .");
    assert_eq!(visible_texts(&editor, &cx), vec!["// a\n// b"]);
}

#[gpui::test]
async fn undo_restores_the_commented_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```rust\nlet a = 1;\n```");
    cx.simulate_keystrokes("g c c");
    assert_eq!(visible_texts(&editor, &cx), vec!["// let a = 1;"]);
    cx.simulate_keystrokes("u");
    assert_eq!(visible_texts(&editor, &cx), vec!["let a = 1;"]);
}

#[gpui::test]
async fn gr_w_replaces_the_word_with_the_unnamed_register(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("y i w");
    cx.simulate_keystrokes("w");
    cx.simulate_keystrokes("g r w");
    assert_eq!(document_text(&editor, &cx), "alpha alpha");
}

#[gpui::test]
async fn gr_uses_the_named_register(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("\" a y i w");
    cx.simulate_keystrokes("w");
    cx.simulate_keystrokes("\" a g r w");
    assert_eq!(document_text(&editor, &cx), "alpha alpha");
    cx.simulate_keystrokes("$ \" a p");
    assert_eq!(
        document_text(&editor, &cx),
        "alpha alphaalpha",
        "替换掉的文本不能覆盖源寄存器"
    );
}

#[gpui::test]
async fn undo_restores_text_replaced_by_gr(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("y i w");
    cx.simulate_keystrokes("w");
    cx.simulate_keystrokes("g r w");
    assert_eq!(document_text(&editor, &cx), "alpha alpha");
    cx.simulate_keystrokes("u");
    assert_eq!(document_text(&editor, &cx), "alpha beta");
}

#[gpui::test]
async fn gr_r_replaces_the_current_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("y y");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("g r r");
    assert_eq!(visible_texts(&editor, &cx), vec!["one", "one"]);
}

#[gpui::test]
async fn gr_r_replaces_a_middle_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("y y");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("g r r");
    assert_eq!(visible_texts(&editor, &cx), vec!["one", "one", "three"]);
}

#[gpui::test]
async fn dot_repeats_gr(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta gamma");
    cx.simulate_keystrokes("y i w");
    cx.simulate_keystrokes("w");
    cx.simulate_keystrokes("g r w");
    cx.simulate_keystrokes("w .");
    assert_eq!(document_text(&editor, &cx), "alpha alpha alpha");
}

#[gpui::test]
async fn dot_repeats_a_change_operator_with_its_insert_text(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three");
    cx.simulate_keystrokes("c w");
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "X two three");
    cx.simulate_keystrokes("w .");
    assert_eq!(document_text(&editor, &cx), "X X three", "cw 的输入应随 . 一起重放");
}

#[gpui::test]
async fn visual_dollar_delete_includes_the_last_character(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("l l v $ d");
    assert_eq!(document_text(&editor, &cx), "ab");
}

#[gpui::test]
async fn gn_selects_the_current_search_match(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two two");
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("g n");
    assert!(matches!(
        current_mode(&editor, &cx),
        VimMode::Visual(VisualKind::Char)
    ));
    cx.simulate_keystrokes("d");
    assert_eq!(document_text(&editor, &cx), "one  two", "gn 应选中当前命中");
}

#[gpui::test]
async fn gN_selects_the_previous_search_match(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two two");
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("G");
    cx.simulate_keystrokes("g N");
    cx.simulate_keystrokes("d");
    assert_eq!(document_text(&editor, &cx), "one two ", "gN 应从光标处向前选命中");
}

#[gpui::test]
async fn cgn_changes_the_match_and_dot_repeats(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two two");
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("c g n");
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "one X two");
    cx.simulate_keystrokes(".");
    assert_eq!(document_text(&editor, &cx), "one X X", "cgn 应由 . 重放");
}

#[gpui::test]
async fn dgn_deletes_matches_and_dot_repeats(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two two");
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("d g n");
    assert_eq!(document_text(&editor, &cx), "one  two");
    cx.simulate_keystrokes(".");
    assert_eq!(document_text(&editor, &cx), "one  ", "dgn 应由 . 重放");
}

#[gpui::test]
async fn g_star_searches_the_word_under_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one two three two");
    cx.simulate_keystrokes("w g *");
    let position = editor.read_with(&*cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx).range.start
    });
    assert_eq!(position, 14, "g* 应搜索光标下的词并跳到下一个命中");
}

#[gpui::test]
async fn gn_without_a_previous_search_is_a_noop(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("g n");
    assert_eq!(document_text(&editor, &cx), "alpha beta");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn ex_command_runs_a_registered_command(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes(":");
    cx.simulate_input("toggle_vim_mode");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    cx.run_until_parked();
    assert!(
        !editor.read_with(&*cx, |editor, _| editor.vim_enabled),
        "冒号行应执行命令注册表里的 toggle_vim_mode"
    );
}

#[gpui::test]
async fn command_mapping_runs_the_registered_command(cx: &mut TestAppContext) {
    let preferences = VimPreferences {
        enable: true,
        keymaps: vec![VimKeymapEntry {
            mode: VimKeymapMode::Normal,
            keys: " d".into(),
            action: VimKeymapAction::Command("toggle_vim_mode".into()),
        }],
        ..Default::default()
    };
    let (editor, cx) = vim_editor_with(cx, "alpha", preferences);
    assert!(editor.read_with(&*cx, |editor, _| editor.vim_enabled));
    cx.simulate_keystrokes("space d");
    redraw(cx);
    cx.run_until_parked();
    assert!(
        !editor.read_with(&*cx, |editor, _| editor.vim_enabled),
        "映射应执行命令注册表里的 toggle_vim_mode"
    );
}

#[gpui::test]
async fn ctrl_a_reinserts_the_last_inserted_text(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("i");
    cx.simulate_input("XY");
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("a");
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "XYXYalpha");
}

#[gpui::test]
async fn ctrl_v_inserts_the_next_key_literally(cx: &mut TestAppContext) {
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
    cx.simulate_keystrokes("ctrl-v j j escape");
    assert_eq!(
        document_text(&editor, &cx),
        "jjalpha",
        "Ctrl-v 后的 j 不应参与 jj 映射"
    );
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn normal_mode_swallows_typing_in_a_new_document_tab(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    editor.update_in(cx, |editor, window, cx| {
        editor.new_workspace_document(window, cx)
    });
    redraw(cx);
    redraw(cx);
    cx.simulate_keystrokes("l l");
    assert_eq!(
        editor.read_with(&*cx, |editor, _| editor.buffer.text().to_string()),
        "",
        "新建文档标签的普通模式不能把字符落进正文"
    );
}

#[gpui::test]
async fn normal_mode_swallows_typing_in_an_empty_document(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "");
    cx.simulate_keystrokes("l l");
    assert_eq!(
        document_text(&editor, &cx),
        "",
        "空文档的普通模式不能把字符落进正文"
    );
    cx.simulate_keystrokes("i");
    cx.simulate_input("X");
    assert_eq!(document_text(&editor, &cx), "X", "i 应能进入插入并落字");
}

#[gpui::test]
async fn focused_split_group_still_swallows_normal_mode_keys(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    let parent = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, _| {
        editor.split_parent = Some(parent.downgrade());
        editor.split_active = false;
    });
    redraw(cx);
    redraw(cx);
    cx.simulate_keystrokes("l l");
    assert_eq!(
        document_text(&editor, &cx),
        "alpha",
        "聚焦的分屏组即使 split_active 为假也必须由 vim 吞键"
    );
}

#[gpui::test]
async fn normal_mode_swallows_typing_after_runtime_enable(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor_with(
        cx,
        "alpha",
        VimPreferences {
            enable: false,
            ..Default::default()
        },
    );
    editor.update(cx, |editor, cx| editor.vim_set_enabled(true, cx));
    redraw(cx);
    redraw(cx);
    cx.simulate_keystrokes("l l");
    assert_eq!(
        document_text(&editor, &cx),
        "alpha",
        "运行时开启 vim 后普通模式不能把字符落进正文"
    );
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn normal_mode_does_not_insert_text_in_source_view(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    redraw(cx);
    redraw(cx);
    let before = document_text(&editor, &cx);
    cx.simulate_keystrokes("l l");
    assert_eq!(
        document_text(&editor, &cx),
        before,
        "普通模式在源码视图也不能把字符落进正文"
    );
    cx.simulate_keystrokes("i");
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
    assert_ne!(document_text(&editor, &cx), before, "i 应能进入插入并落字");
}

#[gpui::test]
async fn toggling_vim_at_runtime_switches_modes_and_passthrough(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
    editor.update(cx, |editor, cx| editor.vim_set_enabled(false, cx));
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert);
    cx.simulate_input("X");
    assert_eq!(document_text(&editor, &cx), "Xalpha", "关闭后输入直接落字");
    editor.update(cx, |editor, cx| editor.vim_set_enabled(true, cx));
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}
