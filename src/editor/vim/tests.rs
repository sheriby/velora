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
) -> (Entity<Editor>, &'a mut VisualTestContext) {    vim_editor_with(
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

/// 真从临时文件打开的 vim 编辑器：`:wq`、`ZZ` 这类要落盘的命令用它。
/// 返回的第三个值是那个文件；配置根与文档同在一个临时目录下。
fn vim_editor_from_file<'a>(
    cx: &'a mut TestAppContext,
    markdown: &str,
) -> (Entity<Editor>, &'a mut VisualTestContext, std::path::PathBuf) {
    init_editor_test_app(cx);
    let root = temp_fixture_dir().join(format!("vim-file-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create vim file root");
    let path = root.join("note.md");
    std::fs::write(&path, markdown).expect("write fixture");
    {
        let _guard = crate::config::override_test_config_root(&root);
        cx.update(|cx| {
            EditorSettings::init(cx, true);
            EditorSettings::set_vim_in_memory(
                cx,
                VimPreferences {
                    enable: true,
                    ..Default::default()
                },
            );
        });
    }
    let window_path = path.clone();
    let (editor, window) = cx.add_window_view(move |_window, cx| {
        Editor::from_file_source(cx, markdown.to_string(), Some(window_path.clone()))
    });
    let workspace_root = root.clone();
    let tab_path = path.clone();
    window.update(|inner_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(workspace_root, cx);
            editor.open_workspace_file(tab_path, inner_window, cx);
        });
    });
    redraw(window);
    redraw(window);
    (editor, window, path)
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
async fn dd_on_the_last_line_keeps_the_next_block_intact(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(
        cx,
        "line one\nline two\nline three\n\n### heading\n\ntext a",
    );
    cx.simulate_keystrokes("j j d d");
    assert_eq!(
        document_text(&editor, &cx),
        "line one\nline two\n\n### heading\n\ntext a",
        "删末行只该删那一行：标题块不能并进段落"
    );
    assert_eq!(cursor_block(&editor, &cx), 1, "删除后光标落到下一行（标题块）");
}

#[gpui::test]
async fn dd_removes_the_whole_heading_block_and_keeps_the_next_kind(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "### heading\n\ntext a");
    cx.simulate_keystrokes("d d");
    assert_eq!(
        document_text(&editor, &cx),
        "text a",
        "删整块时后一块的块种类要跟着它自己走"
    );
}

#[gpui::test]
async fn linewise_delete_through_a_heading_leaves_no_empty_block(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "para\n\n### heading\n\ntext a");
    cx.simulate_keystrokes("d j");
    assert_eq!(
        document_text(&editor, &cx),
        "text a",
        "整块删除走结构删除，不该留下被转成段落的空块"
    );
}

#[gpui::test]
async fn dd_on_the_last_heading_leaves_an_empty_paragraph(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "para\n\n### heading");
    cx.simulate_keystrokes("j d d");
    assert_eq!(
        document_text(&editor, &cx),
        "para\n\n",
        "删空的标题要落成空段落，不能留下 `### `"
    );
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
async fn count_shift_right_indents_each_hard_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\ntwo\nthree");
    cx.simulate_keystrokes("2 > >");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["    one\n    two\nthree"],
        "2>> 要给两条硬行各加一级，不是把同一行加两遍"
    );
}

#[gpui::test]
async fn shift_right_and_left_work_on_code_block_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```\nfn main() {}\n```");
    cx.simulate_keystrokes("> >");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["    fn main() {}"],
        "代码块 >> 要在行首加一级，不是在光标处插空格"
    );
    cx.simulate_keystrokes("0");
    cx.simulate_keystrokes("< <");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["fn main() {}"],
        "代码块 << 要真去掉一级缩进"
    );
}

#[gpui::test]
async fn visual_line_shift_right_indents_each_selected_block(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("V j >");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["    one", "    two"],
        "行级可视的 > 要给每一块所选行都加一级"
    );
}

#[gpui::test]
async fn vim_search_highlights_hits_without_the_search_panel(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\ntwo again");
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    cx.run_until_parked();
    let highlighted = editor.read_with(&*cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .filter(|entry| !entry.entity.read(cx).search_highlight_ranges.is_empty())
            .count()
    });
    assert_eq!(
        highlighted, 2,
        "vim 的 / 要在正文里画出命中，不依赖搜索面板是否开着"
    );
}

#[gpui::test]
async fn nohl_clears_the_document_search_highlights(cx: &mut TestAppContext) {    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\ntwo again");
    let highlighted = |editor: &Entity<Editor>, window: &VisualTestContext| {
        editor.read_with(window, |editor, cx| {
            editor
                .document
                .visible_blocks()
                .iter()
                .filter(|entry| !entry.entity.read(cx).search_highlight_ranges.is_empty())
                .count()
        })
    };
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    cx.run_until_parked();
    assert_eq!(highlighted(&editor, &cx), 2, "夹具：先要画出命中");
    cx.simulate_keystrokes(":");
    cx.simulate_input("noh");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    cx.run_until_parked();
    assert_eq!(highlighted(&editor, &cx), 0, ":noh 要清掉正文里的全部命中高亮");
}


#[gpui::test]
async fn guu_lowercases_the_current_line(cx: &mut TestAppContext) {    let (editor, cx) = vim_editor(cx, "MiXeD");
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

/// 视口对齐类用例的公共布景：200 个单行段落，窗口 600×400。
fn tall_vim_document() -> String {
    (0..200)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn settle_scroll(cx: &mut VisualTestContext) {
    for _ in 0..40 {
        cx.executor().advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
        redraw(cx);
    }
}

fn active_block_vertical_bounds(
    editor: &Entity<Editor>,
    cx: &VisualTestContext,
) -> (f32, f32) {
    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .block_entity_by_id(editor.active_entity_id.expect("活动块"))
            .expect("活动块在文档树里");
        let bounds = editor
            .scroll_target_bounds(&block, true, cx)
            .expect("活动块已挂载");
        (f32::from(bounds.top()), f32::from(bounds.bottom()))
    })
}

fn viewport_vertical_bounds(editor: &Entity<Editor>, cx: &VisualTestContext) -> (f32, f32) {
    editor.read_with(cx, |editor, _| {
        let viewport = editor.scroll_handle.bounds();
        (f32::from(viewport.top()), f32::from(viewport.bottom()))
    })
}

#[gpui::test]
async fn zt_aligns_the_current_block_to_the_viewport_top(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, &tall_vim_document());
    for _ in 0..100 {
        cx.simulate_keystrokes("j");
    }
    settle_scroll(cx);
    cx.simulate_keystrokes("z t");
    settle_scroll(cx);
    let (block_top, _) = active_block_vertical_bounds(&editor, &cx);
    let (viewport_top, _) = viewport_vertical_bounds(&editor, &cx);
    assert!(
        (block_top - viewport_top).abs() <= 2.0,
        "zt 应把当前块贴到视口顶：块 {block_top} / 视口 {viewport_top}"
    );
}

/// 当前活动块的显示文本（表格用例用它看焦点落在哪一格）。
fn active_block_text(editor: &Entity<Editor>, cx: &VisualTestContext) -> String {
    editor.read_with(cx, |editor, cx| {
        editor
            .active_entity_id
            .and_then(|entity_id| editor.focusable_entity_by_id(entity_id))
            .map(|block| block.read(cx).display_text().to_string())
            .unwrap_or_default()
    })
}

#[gpui::test]
async fn table_cells_are_editable_from_vim_normal_mode(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "para\n\n| a | b |\n| --- | --- |\n| c | d |\n\ntail");
    cx.simulate_keystrokes("j");
    assert_eq!(active_block_text(&editor, &cx), "a", "j 应落进表头首格");
    cx.simulate_keystrokes("i");
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape j");
    assert_eq!(active_block_text(&editor, &cx), "c", "j 应在表内换到下一行");
    cx.simulate_keystrokes("i");
    cx.simulate_input("Y");
    cx.simulate_keystrokes("escape");
    assert_eq!(
        document_text(&editor, &cx),
        "para\n\n| Xa | b |\n| --- | --- |\n| Yc | d |\n\ntail",
        "表格单元格的插入编辑要写回原文"
    );
}

#[gpui::test]
async fn hjkl_moves_between_table_cells_and_leaves_at_the_edge(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "para\n\n| a | b |\n| --- | --- |\n| c | d |\n\ntail");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("l");
    assert_eq!(active_block_text(&editor, &cx), "b", "l 应换到右侧单元格");
    cx.simulate_keystrokes("j");
    assert_eq!(active_block_text(&editor, &cx), "d", "j 应换到下一行同一列");
    cx.simulate_keystrokes("h");
    assert_eq!(active_block_text(&editor, &cx), "c", "h 应换到左侧单元格");
    cx.simulate_keystrokes("k");
    assert_eq!(active_block_text(&editor, &cx), "a", "k 应换到上一行同一列");
    cx.simulate_keystrokes("2 l");
    assert_eq!(active_block_text(&editor, &cx), "c", "计数按格前进");
    cx.simulate_keystrokes("j j");
    assert_eq!(
        active_block_text(&editor, &cx),
        "tail",
        "越过表格最后一行应回到正文块"
    );
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn vim_insert_on_a_table_container_lands_in_the_first_cell(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "para\n\n| a | b |\n| --- | --- |\n| c | d |\n\ntail");
    let table = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .find(|entry| entry.entity.read(cx).kind() == crate::components::BlockKind::Table)
            .expect("table block")
            .entity
            .clone()
    });
    editor.update(cx, |editor, _| editor.focus_block(table.entity_id()));
    redraw(cx);
    cx.simulate_keystrokes("i");
    cx.simulate_input("Z");
    assert_eq!(
        document_text(&editor, &cx),
        "para\n\n| Za | b |\n| --- | --- |\n| c | d |\n\ntail",
        "表格容器上的 i 要落进首格并能输入"
    );
}

#[gpui::test]
async fn zz_centers_the_current_block_in_the_viewport(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, &tall_vim_document());
    for _ in 0..100 {
        cx.simulate_keystrokes("j");
    }
    settle_scroll(cx);
    cx.simulate_keystrokes("z z");
    settle_scroll(cx);
    let (block_top, block_bottom) = active_block_vertical_bounds(&editor, &cx);
    let (viewport_top, viewport_bottom) = viewport_vertical_bounds(&editor, &cx);
    let block_center = (block_top + block_bottom) * 0.5;
    let viewport_center = (viewport_top + viewport_bottom) * 0.5;
    assert!(
        (block_center - viewport_center).abs() <= 2.0,
        "zz 应把当前块放到视口中线：块中心 {block_center} / 视口中心 {viewport_center}"
    );
}

#[gpui::test]
async fn zb_aligns_the_current_block_to_the_viewport_bottom(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, &tall_vim_document());
    for _ in 0..100 {
        cx.simulate_keystrokes("j");
    }
    settle_scroll(cx);
    cx.simulate_keystrokes("z b");
    settle_scroll(cx);
    let (_, block_bottom) = active_block_vertical_bounds(&editor, &cx);
    let (_, viewport_bottom) = viewport_vertical_bounds(&editor, &cx);
    assert!(
        (block_bottom - viewport_bottom).abs() <= 2.0,
        "zb 应把当前块贴到视口底：块 {block_bottom} / 视口 {viewport_bottom}"
    );
}

#[gpui::test]
async fn gg_scrolls_the_viewport_back_to_the_document_start(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, &tall_vim_document());
    for _ in 0..40 {
        cx.simulate_keystrokes("j");
    }
    settle_scroll(cx);
    let scrolled = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    assert!(scrolled < gpui::px(0.0), "向下走了 40 行后视口应已滚离文首");
    cx.simulate_keystrokes("g g");
    settle_scroll(cx);
    let offset = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    assert!(
        offset > scrolled,
        "gg 后视口应回到文首方向：{scrolled} -> {offset}"
    );
    let (viewport_top, viewport_bottom) = viewport_vertical_bounds(&editor, &cx);
    let (first_top, first_bottom) = editor.read_with(cx, |editor, cx| {
        let first = editor.document.visible_blocks()[0].entity.clone();
        let bounds = editor
            .scroll_target_bounds(&first, true, cx)
            .expect("首块挂载");
        (f32::from(bounds.top()), f32::from(bounds.bottom()))
    });
    assert!(
        first_top < viewport_bottom && first_bottom > viewport_top,
        "gg 后首块应在视口内：块 {first_top}..{first_bottom} / 视口 {viewport_top}..{viewport_bottom}"
    );
}

#[gpui::test]
async fn capital_g_scrolls_the_viewport_to_the_document_end(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, &tall_vim_document());
    for _ in 0..4 {
        redraw(cx);
    }
    let before = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    cx.simulate_keystrokes("G");
    settle_scroll(cx);
    let offset = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    assert!(
        offset < before,
        "G 后视口应滚向文末：{before} -> {offset}"
    );
    let (viewport_top, viewport_bottom) = viewport_vertical_bounds(&editor, &cx);
    let (last_top, last_bottom) = editor.read_with(cx, |editor, cx| {
        let last = editor.document.visible_blocks().last().unwrap().entity.clone();
        let bounds = editor
            .scroll_target_bounds(&last, true, cx)
            .expect("末块挂载");
        (f32::from(bounds.top()), f32::from(bounds.bottom()))
    });
    assert!(
        last_top < viewport_bottom && last_bottom > viewport_top,
        "G 后末块应在视口内：块 {last_top}..{last_bottom} / 视口 {viewport_top}..{viewport_bottom}"
    );
}

#[gpui::test]
async fn j_inside_a_tall_code_block_scrolls_the_viewport(cx: &mut TestAppContext) {
    let code = (0..80)
        .map(|index| format!("let value_{index} = {index};"))
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = vim_editor(cx, &format!("```rust\n{code}\n```"));
    for _ in 0..4 {
        redraw(cx);
    }
    let before = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    for _ in 0..60 {
        cx.simulate_keystrokes("j");
    }
    settle_scroll(cx);
    let after = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    assert!(
        after < before,
        "代码块内下移应带动视口：{before} -> {after}"
    );
    let (viewport_top, viewport_bottom) = viewport_vertical_bounds(&editor, &cx);
    let (caret_top, caret_bottom) = active_block_vertical_bounds(&editor, &cx);
    assert!(
        caret_top >= viewport_top - 1.0 && caret_bottom <= viewport_bottom + 1.0,
        "代码块内光标应留在视口里：光标 {caret_top}..{caret_bottom} / 视口 {viewport_top}..{viewport_bottom}"
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

fn block_text_input_enabled(
    editor: &Entity<Editor>,
    cx: &VisualTestContext,
    index: usize,
) -> bool {
    editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .get(index)
            .map(|entry| entry.entity.read(cx).vim_accepts_text_input)
            .unwrap_or(true)
    })
}

#[gpui::test]
async fn vim_modes_toggle_block_text_input(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    assert!(
        !block_text_input_enabled(&editor, &cx, 0),
        "普通模式必须关闭系统文本输入"
    );
    cx.simulate_keystrokes("i");
    assert!(
        block_text_input_enabled(&editor, &cx, 0),
        "插入模式必须恢复文本输入"
    );
    cx.simulate_keystrokes("escape");
    assert!(
        !block_text_input_enabled(&editor, &cx, 0),
        "回到普通模式后必须再次关闭文本输入"
    );
}

#[gpui::test]
async fn new_document_blocks_disable_text_input_in_normal_mode(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha");
    editor.update_in(cx, |editor, window, cx| {
        editor.new_workspace_document(window, cx)
    });
    redraw(cx);
    redraw(cx);
    assert!(
        !block_text_input_enabled(&editor, &cx, 0),
        "新建文档的块在普通模式下不能接受系统文本输入"
    );
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

fn active_entity_accepts_text_input(editor: &Entity<Editor>, cx: &VisualTestContext) -> bool {
    editor.read_with(cx, |editor, cx| {
        editor
            .active_entity_id
            .and_then(|entity_id| editor.focusable_entity_by_id(entity_id))
            .map(|block| block.read(cx).vim_accepts_text_input)
            .unwrap_or(true)
    })
}

#[gpui::test]
async fn table_cells_stop_accepting_text_input_in_normal_mode(cx: &mut TestAppContext) {
    let source = "para\n\n| a | b |\n| --- | --- |\n| c | d |\n\ntail";
    let (editor, cx) = vim_editor(cx, source);
    cx.simulate_keystrokes("j");
    assert_eq!(active_block_text(&editor, &cx), "a", "j 应落进表头首格");
    assert!(
        !active_entity_accepts_text_input(&editor, &cx),
        "普通模式下单元格必须关闭系统文本输入"
    );
    cx.simulate_input("jjj");
    assert_eq!(document_text(&editor, &cx), source, "普通模式的可打印字符不能经输入通道落进格子");
    cx.simulate_keystrokes("i");
    assert!(
        active_entity_accepts_text_input(&editor, &cx),
        "插入模式必须恢复单元格的文本输入"
    );
    cx.simulate_keystrokes("escape");
    assert!(
        !active_entity_accepts_text_input(&editor, &cx),
        "退出插入模式后要重新拦住单元格输入"
    );
}

#[gpui::test]
async fn rebuilt_table_cells_follow_the_current_vim_mode(cx: &mut TestAppContext) {
    let source = "para\n\n| a | b |\n| --- | --- |\n| c | d |\n\ntail";
    let (editor, cx) = vim_editor(cx, source);
    // 文档载入与流式续建会在普通模式下整批重建表格运行时。新格子默认
    // 接受系统输入，必须当场继承当前模式，不能等下一次广播。
    editor.update(cx, |editor, cx| editor.rebuild_table_runtimes(cx));
    redraw(cx);
    cx.simulate_keystrokes("j");
    assert_eq!(
        active_block_text(&editor, &cx),
        "a",
        "j 应落进重建后的表头首格"
    );
    assert!(
        !active_entity_accepts_text_input(&editor, &cx),
        "重建出的单元格也要关闭普通模式的系统文本输入"
    );
    cx.simulate_input("jjj");
    assert_eq!(
        document_text(&editor, &cx),
        source,
        "普通模式的可打印字符不能落进重建后的格子"
    );
}

#[gpui::test]
async fn normal_mode_x_edits_the_focused_table_cell_and_undoes(cx: &mut TestAppContext) {
    let source = "para\n\n| alpha | b |\n| --- | --- |\n| c | d |\n\ntail";
    let (editor, cx) = vim_editor(cx, source);
    cx.simulate_keystrokes("j");
    assert_eq!(active_block_text(&editor, &cx), "alpha");
    cx.simulate_keystrokes("x");
    assert_eq!(
        document_text(&editor, &cx),
        "para\n\n| lpha | b |\n| --- | --- |\n| c | d |\n\ntail",
        "x 只删格内一个字符，表前后的块不能被牵连"
    );
    cx.simulate_keystrokes("u");
    assert_eq!(document_text(&editor, &cx), source, "u 要撤销格内删除");
}

#[gpui::test]
async fn normal_mode_dw_edits_the_focused_table_cell_and_undoes(cx: &mut TestAppContext) {
    let source = "para\n\n| alpha | b |\n| --- | --- |\n| c | d |\n\ntail";
    let (editor, cx) = vim_editor(cx, source);
    cx.simulate_keystrokes("j");
    assert_eq!(active_block_text(&editor, &cx), "alpha");
    cx.simulate_keystrokes("d w");
    assert_eq!(
        document_text(&editor, &cx),
        "para\n\n|  | b |\n| --- | --- |\n| c | d |\n\ntail",
        "dw 只清掉焦点格的内容，不能落到表后的段落上"
    );
    cx.simulate_keystrokes("u");
    assert_eq!(document_text(&editor, &cx), source, "u 要撤销格内删除");
}

#[gpui::test]
async fn normal_mode_dd_clears_the_focused_table_cell_and_undoes(cx: &mut TestAppContext) {
    let source = "para\n\n| alpha | b |\n| --- | --- |\n| c | d |\n\ntail";
    let (editor, cx) = vim_editor(cx, source);
    cx.simulate_keystrokes("j");
    assert_eq!(active_block_text(&editor, &cx), "alpha");
    cx.simulate_keystrokes("d d");
    assert_eq!(
        document_text(&editor, &cx),
        "para\n\n|  | b |\n| --- | --- |\n| c | d |\n\ntail",
        "dd 只能清掉焦点格；单元格没有结构删除，整张表不能被收走"
    );
    cx.simulate_keystrokes("u");
    assert_eq!(document_text(&editor, &cx), source, "u 要撤销格内删除");
}

#[gpui::test]
async fn linewise_yank_paste_keeps_block_kinds(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "### 标题\n\n正文");
    cx.simulate_keystrokes("y y p");
    let (kinds, texts) = editor.read_with(cx, |editor, cx| {
        let blocks = editor.document.visible_blocks();
        (
            blocks
                .iter()
                .map(|entry| entry.entity.read(cx).kind())
                .collect::<Vec<_>>(),
            blocks
                .iter()
                .map(|entry| entry.entity.read(cx).display_text().to_string())
                .collect::<Vec<String>>(),
        )
    });
    assert_eq!(
        kinds,
        vec![
            crate::components::BlockKind::Heading { level: 3 },
            crate::components::BlockKind::Heading { level: 3 },
            crate::components::BlockKind::Paragraph,
        ],
        "行级复制粘贴要保住标题级别：{texts:?}"
    );
}

#[gpui::test]
async fn linewise_yank_paste_keeps_inline_markers_of_one_hard_line(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = vim_editor(cx, "第一行 `code`\n第二行\n\ntail");
    cx.simulate_keystrokes("y y p");
    let texts = visible_texts(&editor, &cx);
    assert_eq!(texts.len(), 3, "yy 只复制光标所在那一硬行：{texts:?}");
    assert_eq!(
        document_text(&editor, &cx),
        "第一行 `code`\n第二行\n\n第一行 `code`\n\ntail",
        "行级复制要带行内记号，粘贴出来的仍是一段"
    );
}

#[gpui::test]
async fn visual_line_yank_paste_duplicates_a_soft_wrapped_paragraph(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "第一行 `code`\n第二行\n\ntail");
    cx.simulate_keystrokes("V j y p");
    let texts = visible_texts(&editor, &cx);
    assert_eq!(
        texts.len(),
        3,
        "选中整段的软换行后，粘贴出来仍该是一块：{texts:?}"
    );
    assert_eq!(
        document_text(&editor, &cx),
        "第一行 `code`\n第二行\n\n第一行 `code`\n第二行\n\ntail",
        "整段行级复制要保住软换行与行内记号"
    );
}

// ---- 语言输入框、浮层优先与写回路径 ----

#[gpui::test]
async fn normal_mode_yields_keys_to_the_code_language_field(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler as _;
    let (editor, cx) = vim_editor(cx, "```rust\nab\ncd\n```");
    let code = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[0].entity.clone()
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.focus_block(code.entity_id());
        code.update(cx, |block, block_cx| {
            block.code_language_focus_handle.focus(window, block_cx);
        });
    });
    redraw(cx);

    editor.update_in(cx, |_editor, window, cx| {
        // 重绘会把窗口焦点收回块本身，再点一次语言框才等价于真机上的点击。
        code.update(cx, |block, block_cx| {
            block.code_language_focus_handle.focus(window, block_cx);
        });
    });
    let keystroke = gpui::Keystroke::parse("u").expect("keystroke");
    editor.update_in(cx, |editor, window, cx| {
        assert!(
            !editor.vim_key_down(&keystroke, window, cx),
            "焦点在语言框时 vim 不能把可打印键当命令"
        );
        assert!(
            code.update(cx, |block, block_cx| block
                .accepts_text_input(window, block_cx)),
            "语言框聚焦时系统文本输入必须仍然开启，否则打不进语言名"
        );
    });
    cx.simulate_keystrokes("u");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["ab\ncd"],
        "语言框里的输入不该落进代码正文"
    );
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn escape_closes_an_open_completion_before_vim(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "$$\n\\al\n$$\n");
    let math = editor.read_with(cx, |editor, _cx| {
        editor.document.visible_blocks()[0].entity.clone()
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.focus_block(math.entity_id());
        math.update(cx, |block, block_cx| {
            block.focus_handle.focus(window, block_cx);
            let end = block.display_text().find("\\al").expect("\\al 在块里") + "\\al".len();
            block.move_to(end, block_cx);
        });
        editor.update_latex_completion_for_block(&math, cx);
    });
    redraw(cx);
    editor.read_with(cx, |editor, _cx| {
        assert!(editor.latex_completion_is_open(), "夹具应先弹出公式补全");
    });

    cx.simulate_keystrokes("escape");
    editor.read_with(cx, |editor, _cx| {
        assert!(
            !editor.latex_completion_is_open(),
            "Esc 应先收起补全浮层，而不是被 vim 吞掉"
        );
    });
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
}

#[gpui::test]
async fn single_block_vim_edits_write_back_without_reserializing(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(
        cx,
        "alpha beta\n\nsecond paragraph here\n\nthird paragraph here\n\nfourth line",
    );
    let before = editor.read_with(&*cx, |editor, _| editor.source_serializations.get());
    cx.simulate_keystrokes("x");
    cx.simulate_keystrokes("w x");
    cx.simulate_keystrokes("0 r z");
    cx.simulate_keystrokes("l ~");
    let after = editor.read_with(&*cx, |editor, _| editor.source_serializations.get());
    assert_eq!(
        after - before,
        0,
        "单块内的 vim 编辑要按区间写回，不能每次按键整篇序列化"
    );
    assert!(
        document_text(&editor, &cx).starts_with("zPha eta"),
        "改动本身要真的落到正文：{:?}",
        document_text(&editor, &cx)
    );
}

#[gpui::test]
async fn block_visual_indent_touches_every_selected_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```\nab\ncd\n```");
    cx.simulate_keystrokes("ctrl-q j >");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["    ab\n    cd"],
        "块状缩进要给每条被选硬行都加一级"
    );
}

#[gpui::test]
async fn block_visual_indent_on_a_paragraph_adds_one_level_per_line(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = vim_editor(cx, "one\ntwo");
    cx.simulate_keystrokes("ctrl-q j >");
    assert_eq!(
        document_text(&editor, &cx),
        "    one\n    two",
        "段落里的块状缩进每行只加一级，不能叠成八格"
    );
}

#[gpui::test]
async fn block_visual_outdent_removes_one_indent_level(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```\n    ab\n    cd\n```");
    cx.simulate_keystrokes("0 ctrl-q j <");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["ab\ncd"],
        "块状出缩进要真删掉缩进，不能是空操作"
    );
}

#[gpui::test]
async fn block_visual_indent_keeps_the_unnamed_register(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("y y");
    cx.simulate_keystrokes("ctrl-q j >");
    cx.simulate_keystrokes("G");
    cx.simulate_keystrokes("p");
    let texts = visible_texts(&editor, &cx);
    assert!(
        texts.iter().any(|text| text.contains("one")),
        "缩进不该覆盖无名寄存器：{texts:?}"
    );
}

#[gpui::test]
async fn block_visual_yank_does_not_mark_the_document_dirty(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```\nab\ncd\n```");
    cx.simulate_keystrokes("ctrl-q j y");
    assert_eq!(visible_texts(&editor, &cx), vec!["ab\ncd"]);
    editor.read_with(cx, |editor, _cx| {
        assert!(
            !editor.document_dirty,
            "块状复制只动寄存器，不该把文档标脏"
        );
    });
}

#[gpui::test]
async fn recursive_macro_is_refused_instead_of_overflowing_the_stack(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    editor.update(cx, |editor, _cx| {
        editor
            .vim
            .macro_registers
            .insert('a', vec![VimKey::Char('@'), VimKey::Char('a')]);
    });
    cx.simulate_keystrokes("@ a");
    assert_eq!(
        document_text(&editor, &cx),
        "abcdef",
        "自引用宏应像 E223 一样直接被拒，正文不该被递归改动"
    );
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal);
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.vim.macro_stack.is_empty(),
            "重放结束后宏栈必须清空"
        );
    });
}

// ---- 反向动作的操作符区间 ----

#[gpui::test]
async fn d_b_deletes_from_the_word_start_up_to_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("w l d b");
    assert_eq!(
        document_text(&editor, &cx),
        "alpha eta",
        "db 要删掉光标左侧到词首的那一个字素"
    );
}

#[gpui::test]
async fn d_zero_deletes_to_the_line_start(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("l l l d 0");
    assert_eq!(document_text(&editor, &cx), "def", "d0 要删到行首");
}

#[gpui::test]
async fn d_capital_f_deletes_from_the_found_char(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("l l l d F a");
    assert_eq!(document_text(&editor, &cx), "def", "dFa 要删掉 a 到光标之前");
}

#[gpui::test]
async fn y_b_yanks_the_characters_left_of_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("w l y b");
    cx.simulate_keystrokes("0 p");
    assert_eq!(
        document_text(&editor, &cx),
        "ablpha beta",
        "yb 复制的是光标左侧的词首片段，不是空串"
    );
}

#[gpui::test]
async fn c_zero_changes_up_to_the_line_start(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abcdef");
    cx.simulate_keystrokes("l l l c 0");
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert);
    cx.simulate_input("X");
    cx.simulate_keystrokes("escape");
    assert_eq!(document_text(&editor, &cx), "Xdef", "c0 要删到行首再输入");
}

// ---- 块末/行末的单字素操作 ----

#[gpui::test]
async fn r_at_the_last_grapheme_replaces_it(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abc");
    cx.simulate_keystrokes("$ r z");
    assert_eq!(document_text(&editor, &cx), "abz", "块末的 r 要真的换掉那个字素");
}

#[gpui::test]
async fn tilde_at_the_last_grapheme_swaps_case(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abC");
    cx.simulate_keystrokes("$ ~");
    assert_eq!(document_text(&editor, &cx), "abc", "块末的 ~ 要真的换大小写");
}

#[gpui::test]
async fn counted_replace_does_not_cross_the_hard_line_end(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```\nab\ncd\n```");
    cx.simulate_keystrokes("l");
    cx.simulate_keystrokes("2 r z");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["az\ncd"],
        "行末只剩一个字素时，2r 只换那一个，不越到下一行"
    );
}

// ---- 字符可视的端点与块状可视的角交换 ----

#[gpui::test]
async fn visual_then_delete_removes_the_cursor_grapheme(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "abc");
    cx.simulate_keystrokes("v d");
    assert_eq!(
        document_text(&editor, &cx),
        "bc",
        "刚按 v 时选区含光标所在字素，Vim 的 v d 删掉它"
    );
}

#[gpui::test]
async fn visual_inner_word_selects_exactly_the_word(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("v i w d");
    assert_eq!(
        document_text(&editor, &cx),
        " beta",
        "viw 只选 alpha 本身，不能多带一个词尾字素"
    );
}

#[gpui::test]
async fn visual_inner_quote_selects_exactly_the_inner_text(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "\"hello\"");
    cx.simulate_keystrokes("l l v i \" d");
    assert_eq!(
        document_text(&editor, &cx),
        "\"\"",
        "vi\" 选中两个引号之间的内容，引号要留下"
    );
}

#[gpui::test]
async fn block_visual_o_keeps_the_rectangle(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```\nabcd\nefgh\n```");
    cx.simulate_keystrokes("ctrl-q j l o");
    cx.simulate_keystrokes("d");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["cd\ngh"],
        "块状 o 交换两个角后矩形面积不变"
    );
}

#[gpui::test]
async fn block_visual_capital_o_keeps_the_rectangle(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "```\nabcd\nefgh\n```");
    cx.simulate_keystrokes("ctrl-q j l O");
    cx.simulate_keystrokes("d");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["cd\ngh"],
        "块状 O 交换两列后矩形面积不变"
    );
}

// ---- :wq / :x / ZZ 先落盘再关 ----

#[gpui::test]
async fn cmdline_wq_writes_the_dirty_document_and_closes(cx: &mut TestAppContext) {
    let (editor, cx, path) = vim_editor_from_file(cx, "alpha\n");
    cx.simulate_keystrokes("A");
    cx.simulate_input("x");
    cx.simulate_keystrokes("escape");
    assert!(
        editor.read_with(&*cx, |editor, _cx| editor.document_dirty),
        "改过之后文档应是脏的"
    );

    cx.simulate_keystrokes(":");
    cx.simulate_input("wq");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    cx.run_until_parked();

    let on_disk = std::fs::read_to_string(&path).expect("read the file back");
    assert_eq!(
        on_disk.trim(),
        "alphax",
        ":wq 要把改动写盘，而不是排队等下一帧"
    );
    editor.read_with(&*cx, |editor, _cx| {
        assert!(editor.modal.is_none(), ":wq 不该弹「未保存」确认框");
        assert_ne!(
            editor.file_path.as_deref(),
            Some(path.as_path()),
            ":wq 写盘成功后要关掉这一页"
        );
    });
}

#[gpui::test]
async fn zz_writes_the_dirty_document_and_closes(cx: &mut TestAppContext) {
    let (editor, cx, path) = vim_editor_from_file(cx, "alpha\n");
    cx.simulate_keystrokes("A");
    cx.simulate_input("y");
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("Z Z");
    redraw(cx);
    cx.run_until_parked();
    let on_disk = std::fs::read_to_string(&path).expect("read the file back");
    assert_eq!(on_disk.trim(), "alphay", "ZZ 等价 :x，要写盘再关");
    editor.read_with(&*cx, |editor, _cx| {
        assert!(editor.modal.is_none(), "ZZ 不该弹「未保存」确认框");
        assert_ne!(
            editor.file_path.as_deref(),
            Some(path.as_path()),
            "ZZ 写盘成功后要关掉这一页"
        );
    });
}

// ---- Ctrl-[ 与命令行的组合态 ----

#[gpui::test]
async fn ctrl_bracket_returns_to_normal_mode(cx: &mut TestAppContext) {    let (editor, cx) = vim_editor(cx, "alpha");
    cx.simulate_keystrokes("i");
    assert_eq!(current_mode(&editor, &cx), VimMode::Insert);
    cx.simulate_keystrokes("ctrl-[");
    assert_eq!(
        current_mode(&editor, &cx),
        VimMode::Normal,
        "Ctrl-[ 是终端里的 Esc，要能退出插入"
    );
    cx.simulate_keystrokes("ctrl-[");
    assert_eq!(current_mode(&editor, &cx), VimMode::Normal, "普通模式下 Ctrl-[ 无副作用");
}

// ---- Ctrl-a 进位与插入模式的 Ctrl-r ----

#[gpui::test]
async fn ctrl_a_increments_the_whole_number_under_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "value 1299");
    cx.simulate_keystrokes("w w l l");
    let caret = first_block_caret(&editor, &cx);
    assert_eq!(caret, 8, "夹具：光标要落在第一个 9 上");
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(
        document_text(&editor, &cx),
        "value 1300",
        "光标在多位数字中间时，进位要作用于整段数字"
    );
}

#[gpui::test]
async fn ctrl_r_in_insert_types_a_linewise_register_at_the_cursor(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    cx.simulate_keystrokes("\" a y y");
    cx.simulate_keystrokes("G");
    cx.simulate_keystrokes("i");
    cx.simulate_keystrokes("ctrl-r a");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["one", "onetwo"],
        "插入模式的 Ctrl-r 要把行级寄存器打进光标处，不另起一块"
    );
}

#[gpui::test]
async fn reading_mode_edit_commands_leave_text_and_registers_alone(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo");
    editor.update(cx, |editor, cx| editor.toggle_reading_mode(cx));
    redraw(cx);
    cx.simulate_keystrokes("y y");
    let register = editor.read_with(&*cx, |editor, cx| {
        editor.vim.registers.read('"', cx, false).map(|content| content.text)
    });
    assert_eq!(
        register.as_deref(),
        Some("one\n"),
        "夹具：只读下 yank 仍要能用"
    );
    cx.simulate_keystrokes("G d d");
    cx.simulate_keystrokes("x");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["one", "two"],
        "只读模式里编辑命令不该改正文"
    );
    let register = editor.read_with(&*cx, |editor, cx| {
        editor.vim.registers.read('"', cx, false).map(|content| content.text)
    });
    assert_eq!(
        register.as_deref(),
        Some("one\n"),
        "被拒绝的删除不该把正文写进寄存器与剪贴板"
    );
}

#[gpui::test]
async fn normal_mode_double_click_only_moves_the_caret(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    let center = editor.read_with(&*cx, |editor, cx| {
        editor.document.visible_blocks()[0]
            .entity
            .read(cx)
            .last_bounds
            .expect("段落该有布局边界")
            .center()
    });
    cx.simulate_event(gpui::MouseDownEvent {
        position: center,
        button: gpui::MouseButton::Left,
        modifiers: gpui::Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_mouse_up(center, gpui::MouseButton::Left, gpui::Modifiers::none());
    redraw(cx);
    editor.read_with(&*cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        assert!(
            block.selected_range.is_empty(),
            "普通模式双击不该留下真实选区：{:?}",
            block.selected_range
        );
    });
    cx.simulate_keystrokes("A");
    cx.simulate_input("!");
    cx.simulate_keystrokes("escape");
    assert_eq!(
        document_text(&editor, &cx),
        "alpha beta!",
        "双击之后的输入不该替换整段"
    );
}

#[gpui::test]
async fn insert_mode_double_click_still_selects_the_word(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "alpha beta");
    cx.simulate_keystrokes("i");
    let center = editor.read_with(&*cx, |editor, cx| {
        editor.document.visible_blocks()[0]
            .entity
            .read(cx)
            .last_bounds
            .expect("段落该有布局边界")
            .center()
    });
    cx.simulate_event(gpui::MouseDownEvent {
        position: center,
        button: gpui::MouseButton::Left,
        modifiers: gpui::Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_mouse_up(center, gpui::MouseButton::Left, gpui::Modifiers::none());
    redraw(cx);
    editor.read_with(&*cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        assert!(
            !block.selected_range.is_empty(),
            "插入模式里双击仍要选词：{:?}",
            block.selected_range
        );
    });
}

#[gpui::test]
async fn ctrl_i_returns_to_the_position_left_by_ctrl_o(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    assert_eq!(cursor_block(&editor, &cx), 0);
    cx.simulate_keystrokes("G");
    assert_eq!(cursor_block(&editor, &cx), 2, "夹具：G 要跳到末块");
    cx.simulate_keystrokes("ctrl-o");
    assert_eq!(cursor_block(&editor, &cx), 0, "ctrl-o 回到跳之前");
    cx.simulate_keystrokes("ctrl-i");
    assert_eq!(
        cursor_block(&editor, &cx),
        2,
        "ctrl-i 要能回到刚离开的那一处"
    );
}

#[gpui::test]
async fn search_jump_is_recorded_in_the_jumplist(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("/");
    cx.simulate_input("three");
    cx.simulate_keystrokes("enter");
    assert_eq!(cursor_block(&editor, &cx), 2, "夹具：/ 要跳到命中所在块");
    cx.simulate_keystrokes("ctrl-o");
    assert_eq!(
        cursor_block(&editor, &cx),
        0,
        "搜索跳转也要进跳转表，ctrl-o 才回得到搜索前"
    );
}

#[gpui::test]
async fn cmdline_closes_when_focus_leaves_it(cx: &mut TestAppContext) {    let (editor, cx) = vim_editor(cx, "alpha beta");
    crate::editor::tests::common::activate_visual_window(cx);
    cx.simulate_keystrokes(":");
    redraw(cx);
    assert!(
        editor.read_with(&*cx, |editor, _cx| editor.vim.cmdline.is_some()),
        "夹具：冒号行应已打开"
    );
    let center = editor.read_with(&*cx, |editor, cx| {
        editor.document.visible_blocks()[0]
            .entity
            .read(cx)
            .last_bounds
            .expect("段落该有布局边界")
            .center()
    });
    cx.simulate_click(center, gpui::Modifiers::none());
    redraw(cx);
    assert!(
        editor.read_with(&*cx, |editor, _cx| editor.vim.cmdline.is_none()),
        "点了正文之后冒号行要收起，不再挂在底部吃键"
    );
}

// ---- 字素簇步进 ----

#[gpui::test]
async fn x_deletes_a_whole_zwj_family_cluster(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}x");
    cx.simulate_keystrokes("x");
    assert_eq!(
        document_text(&editor, &cx),
        "x",
        "x 要删掉整个 ZWJ 家族簇，不能只切掉一个 code point"
    );
}

#[gpui::test]
async fn l_and_h_step_over_combining_marks_as_one_grapheme(cx: &mut TestAppContext) {
    // "e\u{0301}" 是一个字素簇（e + 组合尖音符）：l 一步跨过整簇。
    let (editor, cx) = vim_editor(cx, "e\u{0301}abc");
    cx.simulate_keystrokes("l");
    assert_eq!(
        first_block_caret(&editor, &cx),
        3,
        "l 要停在组合字素之后的 a 上（e 与组合符共 3 字节）"
    );
    cx.simulate_keystrokes("h");
    assert_eq!(
        first_block_caret(&editor, &cx),
        0,
        "h 要退回组合字素的起点，不是停在组合符中间"
    );
}

#[gpui::test]
async fn vim_search_ignores_the_panel_regex_and_fuzzy_switches(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "ab\n\na.");
    // 面板把正则开着：vim 的 / 仍按字面量匹配，不能被当成正则。
    editor.update(cx, |editor, _cx| {
        editor.apply_search_options(crate::editor::workspace::SearchOptions {
            match_case: true,
            whole_word: false,
            use_regex: true,
            fuzzy: true,
        });
    });
    cx.simulate_keystrokes("/");
    cx.simulate_input("a.");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    cx.run_until_parked();
    assert_eq!(
        cursor_block(&editor, &cx),
        1,
        "/ 要找字面量 a.，不是正则的 a 加任意字符（那会落在第一块的 ab）"
    );
    // 同一句再搜一次：模糊开着会把 a. 当成子序列匹配，命中位置会漂。
    cx.simulate_keystrokes("n");
    assert_eq!(
        cursor_block(&editor, &cx),
        1,
        "n 仍在同一处命中：面板的模糊开关不参与 vim 搜索"
    );
    let options = editor.read_with(&*cx, |editor, _cx| editor.search_options());
    assert!(
        !options.use_regex && !options.fuzzy,
        "面板开关要跟上实际命中表的口径，两处不能算出两样结果：{options:?}"
    );
}

// ---- 行级复制取 Markdown 源码 ----

fn unnamed_register(editor: &Entity<Editor>, cx: &VisualTestContext) -> Option<String> {
    editor.read_with(cx, |editor, cx| {
        editor
            .vim
            .registers
            .read('"', cx, false)
            .map(|content| content.text)
    })
}

#[gpui::test]
async fn linewise_yank_keeps_markers_of_a_later_hard_line(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "第一行\n第二行 `code`\n\n尾段");
    cx.simulate_keystrokes("j");
    cx.simulate_keystrokes("y y");
    assert_eq!(
        unnamed_register(&editor, &cx).as_deref(),
        Some("第二行 `code`\n"),
        "第二硬行的行级复制也要带行内记号，不能回落成显示文本"
    );
}

#[gpui::test]
async fn linewise_yank_of_adjacent_list_items_keeps_the_original_separator(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = vim_editor(cx, "- a\n- b\n\ntail");
    cx.simulate_keystrokes("V j y");
    assert_eq!(
        unnamed_register(&editor, &cx).as_deref(),
        Some("- a\n- b\n"),
        "紧邻列表项之间是原文的那一个换行，不是写死的空行"
    );
}

#[gpui::test]
async fn substitute_expands_the_whole_match_and_group_backreferences(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = vim_editor(cx, "abc");
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/A&/");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(
        document_text(&editor, &cx),
        "Aabc",
        "& 要展开成整个命中，而不是字面的 &"
    );
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/(b)/x\\1/");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(
        document_text(&editor, &cx),
        "Aaxbc",
        "\\1 要展开成第一个分组，而不是只剩字面反斜杠"
    );
}

#[gpui::test]
async fn cmdline_recalls_the_previous_search_with_up_arrow(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("/");
    cx.simulate_input("two");
    cx.simulate_keystrokes("enter");
    assert_eq!(cursor_block(&editor, &cx), 1, "夹具：先搜到 two");
    cx.simulate_keystrokes("/");
    cx.simulate_keystrokes("up");
    let value = editor.read_with(&*cx, |editor, cx| {
        editor
            .vim
            .cmdline
            .as_ref()
            .map(|line| line.input.read(cx).value().to_string())
    });
    assert_eq!(value.as_deref(), Some("/two"), "↑ 要取回上一次搜索");
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("G");
    cx.simulate_keystrokes("/");
    cx.simulate_keystrokes("up");
    cx.simulate_keystrokes("up");
    let value = editor.read_with(&*cx, |editor, cx| {
        editor
            .vim
            .cmdline
            .as_ref()
            .map(|line| line.input.read(cx).value().to_string())
    });
    assert_eq!(
        value.as_deref(),
        Some("/two"),
        "只有一句历史时再按 ↑ 停在同一条，不越界"
    );
    cx.simulate_keystrokes("escape");
}

#[gpui::test]
async fn count_before_the_repeated_operator_deletes_two_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree\n\nfour");
    cx.simulate_keystrokes("d 2 d");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["three", "four"],
        "d2d 等价 2dd：删掉当前行与下一行"
    );
}

#[gpui::test]
async fn count_inside_the_repeated_operator_indents_two_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "one\n\ntwo\n\nthree");
    cx.simulate_keystrokes("> 2 >");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["    one", "    two", "three"],
        ">2> 要给两行各加一级缩进"
    );
}

#[gpui::test]
async fn count_before_gj_joins_that_many_lines(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "a\nb\nc\nd");
    cx.simulate_keystrokes("3 g J");
    assert_eq!(
        visible_texts(&editor, &cx),
        vec!["abc\nd"],
        "3gJ 要并三行（外层计数不能被固定的两行吃掉）"
    );
}

#[gpui::test]
async fn open_bracket_lands_on_the_first_non_blank_of_the_sentence(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = vim_editor(cx, "Hello. World.");
    cx.simulate_keystrokes("l l l l l l l l l");
    assert_eq!(first_block_caret(&editor, &cx), 9, "夹具：光标落在 World 的 o 上");
    cx.simulate_keystrokes("(");
    assert_eq!(
        first_block_caret(&editor, &cx),
        7,
        "( 要落到句首的 W，不是句号后的那个空格"
    );
}

#[gpui::test]
async fn close_bracket_steps_over_a_run_of_terminators(cx: &mut TestAppContext) {
    let (editor, cx) = vim_editor(cx, "Really!? Yes.");
    cx.simulate_keystrokes(")");
    assert_eq!(
        first_block_caret(&editor, &cx),
        9,
        ") 要越过 !? 整段终结符与空白，落到下一句首"
    );
}
