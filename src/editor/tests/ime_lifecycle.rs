use super::common::*;

/// 拼音预编辑是临时文本：它和已上屏汉字之间不能加中西文 autospacing，
/// 否则光标下的拼音看起来与正文断开。上屏后才该按真实边界加宽。
#[gpui::test]
async fn ime_preedit_skips_cjk_autospacing_until_committed(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "中文".into(), None));
    redraw(cx);
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    let end = "中文".len();
    block.update(cx, |block, _| block.selected_range = end..end);
    block.update_in(cx, |block, window, cx| {
        block.replace_and_mark_text_in_range(None, "pinyin", Some(6..6), window, cx)
    });
    redraw(cx);
    redraw(cx);
    let (marked_width, autospace) = block.read_with(cx, |block, _| {
        assert_eq!(block.marked_range, Some(end..end + "pinyin".len()));
        let memo = block.shape_memo_entry().expect("组合态应完成排版");
        (
            memo.lines[0].width(),
            px(f32::from_bits(memo.key.font_size) * 0.25),
        )
    });
    block.update_in(cx, |block, window, cx| {
        block.replace_text_in_range(None, "pinyin", window, cx)
    });
    redraw(cx);
    redraw(cx);
    let committed_width = block.read_with(cx, |block, _| {
        assert!(!block.has_marked_text());
        block
            .shape_memo_entry()
            .expect("上屏后应重新排版")
            .lines[0]
            .width()
    });
    let delta = committed_width - marked_width;
    assert!(
        (f32::from(delta) - f32::from(autospace)).abs() < 0.01,
        "上屏后只应多出一份中西文间隔 {autospace:?}，实际多出 {delta:?}"
    );
}

#[gpui::test]
async fn core_repair_same_text_ime_commit_separates_following_typing(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "note".into(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    block.update(cx, |block, _| block.selected_range = 4..4);
    block.update_in(cx, |block, window, cx| block.replace_and_mark_text_in_range(None, "中", Some(1..1), window, cx));
    redraw(cx);
    block.update_in(cx, |block, window, cx| block.replace_text_in_range(None, "中", window, cx));
    redraw(cx);
    block.update_in(cx, |block, window, cx| block.replace_text_in_range(None, "X", window, cx));
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), "note中X");
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), "note中");
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), "note");
}

/// 链接标签里的同字上屏也要把组合收尾。
///
/// 标签里的编辑走链接投影（`apply_link_projection_edit`）：那条早退分支把 `marked_range`
/// 置 `None` 后只按「kind 或 title 变了」发 `Changed` 就 return——提交文本与已 marked 的
/// 文本相同时标题没变，于是既无 `Changed` 也无 `CompositionChanged`；而 GPUI 的
/// `unmark_text` 拿到的 `marked_range` 已经是 `None`（`changed == false`）也不发。
/// `Editor::finish_ime_composition` 全仓唯一触发点就是 `block_event.rs` 的
/// `CompositionChanged` 臂，于是撤销组一直开着：下一次编辑的字节并进这个
/// `ImeCompositionCommit` 组，组内选区是组合开始前的旧现场，一次撤销退两步。
#[gpui::test]
async fn review_same_text_ime_commit_inside_a_link_label_finishes_the_composition(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = "[中](https://example.test)";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    // 标签“中”在展开后的显示坐标里是 1..4；选中整段标签再上屏，走的才是那条早退分支。
    let label = 1..1 + "中".len();
    block.update_in(cx, |block, window, cx| {
        block.focus_handle.focus(window, cx);
        block.selected_range = label.clone();
        block.sync_inline_projection_for_focus(true);
        // 组合开始：把整段标签换成同一个字，走的正是那条早退分支。
        block.replace_and_mark_text_in_range(Some(1..2), "中", None, window, cx);
    });
    redraw(cx);
    assert!(
        block.read_with(cx, |block, _| block.has_marked_text()),
        "夹具前提：标签里正组合着「中」"
    );

    // 同字上屏：提交文本 = 已 marked 文本，字节没变。
    block.update_in(cx, |block, window, cx| {
        block.replace_text_in_range(Some(1..2), "中", window, cx)
    });
    redraw(cx);

    assert!(!block.read_with(cx, |block, _| block.has_marked_text()));
    assert_eq!(editor.read_with(cx, |editor, _| editor.buffer.text()), source);
    assert!(
        editor.read_with(cx, |editor, _| editor.pending_undo_capture.is_none()),
        "组合结束了撤销组还开着：下一次编辑会并进这个组，一次撤销退两步"
    );
}

fn composition_end_autosaves(cx: &mut TestAppContext, unmark: bool) {
    init_editor_test_app(cx);
    let path = temp_markdown_path("same-ime-save");
    fs::write(&path, "note").unwrap();
    cx.on_quit({ let path = path.clone(); move || fs::remove_file(path).unwrap() });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "note".into(), Some(path.clone())));
    let recovery = editor.read_with(cx, |editor, _| editor.recovery_id);
    cx.cx.on_quit(move || crate::config::remove_recovery_snapshot(recovery).unwrap());
    let block = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    block.update(cx, |block, _| block.selected_range = 4..4);
    block.update_in(cx, |block, window, cx| block.replace_and_mark_text_in_range(None, "中", Some(1..1), window, cx));
    redraw(cx);
    if unmark {
        block.update_in(cx, |block, window, cx| block.unmark_text(window, cx));
    } else {
        block.update_in(cx, |block, window, cx| block.replace_text_in_range(None, "中", window, cx));
    }
    redraw(cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(path).unwrap(), "note中");
}

#[gpui::test]
async fn core_repair_same_text_ime_commit_resumes_autosave(cx: &mut TestAppContext) {
    composition_end_autosaves(cx, false);
}

#[gpui::test]
async fn core_repair_unmark_resumes_autosave(cx: &mut TestAppContext) {
    composition_end_autosaves(cx, true);
}

#[gpui::test]
async fn core_repair_language_same_text_commit_resumes_autosave(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "```rust\ncode\n```";
    let path = temp_markdown_path("language-ime-save");
    fs::write(&path, source).unwrap();
    cx.on_quit({ let path = path.clone(); move || fs::remove_file(path).unwrap() });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), Some(path.clone())));
    let recovery = editor.read_with(cx, |editor, _| editor.recovery_id);
    cx.cx.on_quit(move || crate::config::remove_recovery_snapshot(recovery).unwrap());
    let block = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    block.update(cx, |block, cx| {
        let length = block.visible_len();
        block.replace_text_in_visible_range(0..length, "edited", None, false, cx);
    });
    block.update_in(cx, |block, window, cx| {
        block.code_language_focus_handle.focus(window, cx);
        block.replace_and_mark_text_in_range(Some(0..4), "rust", Some(4..4), window, cx);
    });
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    block.update_in(cx, |block, window, cx| block.replace_text_in_range(None, "rust", window, cx));
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(path).unwrap(), "```rust\nedited\n```");
}

enum SaveAttempt { Auto, Manual, Close }

fn table_preedit_is_not_saved(cx: &mut TestAppContext, attempt: SaveAttempt) {
    init_editor_test_app(cx);
    let source = "| a | b |\n| --- | --- |\n| c | d |";
    let path = temp_markdown_path("table-ime-save");
    fs::write(&path, source).unwrap();
    cx.on_quit({ let path = path.clone(); move || fs::remove_file(path).unwrap() });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), Some(path.clone())));
    let recovery = editor.read_with(cx, |editor, _| editor.recovery_id);
    cx.cx.on_quit(move || crate::config::remove_recovery_snapshot(recovery).unwrap());
    let cell = editor.read_with(cx, |editor, cx| {
        editor.document.first_root().unwrap().read(cx).table_runtime.as_ref().unwrap().header[0].clone()
    });
    cell.update_in(cx, |block, window, cx| block.replace_and_mark_text_in_range(None, "n", Some(1..1), window, cx));
    match attempt {
        SaveAttempt::Auto => cx.executor().advance_clock(Duration::from_secs(1)),
        SaveAttempt::Manual => { editor.update_in(cx, |editor, window, cx| editor.save_to_existing_path(&path, window, cx)); }
        SaveAttempt::Close => editor.update_in(cx, |editor, window, cx| editor.save_dirty_workspace_documents_and_close(window, cx)),
    }
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(path).unwrap(), source);
    assert!(!cx.cx.windows().is_empty());
}

#[gpui::test]
async fn core_repair_table_preedit_blocks_autosave(cx: &mut TestAppContext) {
    table_preedit_is_not_saved(cx, SaveAttempt::Auto);
}

#[gpui::test]
async fn core_repair_table_preedit_blocks_manual_save(cx: &mut TestAppContext) {
    table_preedit_is_not_saved(cx, SaveAttempt::Manual);
}

#[gpui::test]
async fn core_repair_table_preedit_blocks_save_and_close(cx: &mut TestAppContext) {
    table_preedit_is_not_saved(cx, SaveAttempt::Close);
}
