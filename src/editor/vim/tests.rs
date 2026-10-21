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
