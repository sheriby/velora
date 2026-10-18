use super::{AiCommand, Editor, PanelMode, RunControl};
use gpui::{TestAppContext, VisualTestContext};

fn initialize(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::ai::settings::AiFeature::set_enabled(cx, true);
    });
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn menu_command_position(
    cx: &mut VisualTestContext,
    label: impl FnOnce(&crate::i18n::I18nStrings) -> &str,
) -> gpui::Point<gpui::Pixels> {
    use gpui_kit::test::TestWindowExt as _;
    cx.update(|window, cx| {
        let label = label(cx.global::<crate::i18n::I18nManager>().strings());
        let menu = window.within("popup-menu");
        (0usize..20)
            .filter_map(|index| menu.try_find(index))
            .find(|item| item.label() == Some(label))
            .unwrap_or_else(|| panic!("菜单应包含可访问的命令：{label}"))
            .bounds()
            .center()
    })
}

#[gpui::test]
fn ai_conversation_entry_has_no_redundant_quick_command_row(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor.ai_session.as_mut().unwrap().mode = PanelMode::Task;
    });
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-command-bar").is_none(), "浮动对话不再重复列出五项快捷指令");
    let panel = cx.debug_bounds("ai-assistant-panel").unwrap();
    assert!(panel.size.height <= gpui::px(180.0), "空白对话应保持紧凑：panel={panel:?}, composer={:?}, configure={:?}, error={:?}", cx.debug_bounds("ai-composer"), cx.debug_bounds("ai-configure"), cx.debug_bounds("ai-command-error"));
    assert!(
        cx.debug_bounds("ai-content").is_none(),
        "入口不应有对话滚动区"
    );
    assert!(cx.debug_bounds("ai-review-workspace").is_none());
}

#[gpui::test]
fn ai_revisions_use_the_document_area_and_acceptance_returns_to_the_original_editor(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let source = "# 一篇笔记\n\n原始文字。\n\n保留的下文。";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let target = session.saved.target.as_ref().unwrap().document_id.clone();
        let baseline = session.saved.workspace.documents[&target].clone();
        session.saved.workspace.changes.proposals.insert(
            target,
            crate::ai::Proposal::new(baseline, source.replace("原始文字", "清晰的文字")),
        );
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    cx.update(|window, cx| {
        use gpui_kit::test::TestWindowExt as _;
        let accept = window.find("ai-review-accept");
        assert_eq!(accept.role(), Some(gpui::Role::Button));
        assert_eq!(accept.label(), Some(cx.global::<crate::i18n::I18nManager>().strings().ai_panel_accept_edit.as_str()),
            "接受修改必须提供清楚的文字标签");
    });
    let surface = cx
        .debug_bounds("ai-review-workspace")
        .expect("修改应在正文区域审阅");
    let viewport = cx.update(|window, _| window.viewport_size());
    assert!(surface.size.width > viewport.width * 0.65);
    assert!(surface.size.height > viewport.height * 0.65);
    assert!(cx.debug_bounds("ai-task-bar").is_none());
    assert!(cx.debug_bounds("ai-floating-workbench").is_some());
    assert!(cx.debug_bounds("ai-review-document").is_some());
    editor.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), source));
    let accept = cx
        .debug_bounds("ai-review-accept")
        .expect("正文旁应有接受操作")
        .center();
    cx.simulate_click(accept, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-review-workspace").is_none());
    editor.update_in(cx, |editor, window, cx| {
        assert_eq!(
            editor.buffer.text(),
            source.replace("原始文字", "清晰的文字")
        );
        editor.on_undo(&crate::components::Undo, window, cx);
        assert_eq!(editor.buffer.text(), source);
    });
}

#[gpui::test]
fn ai_multiple_document_review_has_file_navigation_in_the_floating_dialog(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        for id in ["first.md", "second.md"] {
            session.saved.workspace.changes.proposals.insert(
                id.into(),
                crate::ai::Proposal::new(
                    crate::ai::DocumentSnapshot {
                        id: id.into(),
                        source: "原文".into(),
                    },
                    format!("# {id}\n\n新的正文"),
                ),
            );
        }
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    let files = cx
        .debug_bounds("ai-review-files")
        .expect("跨文档修改需要独立的文件导航");
    let panel = cx.debug_bounds("ai-floating-workbench").unwrap();
    assert!(files.left() >= panel.left() && files.right() <= panel.right());
    assert!(cx.debug_bounds("ai-review-document").is_some());
    let second = cx.debug_bounds("ai-review-file-1").unwrap().center();
    cx.simulate_click(second, gpui::Modifiers::none());
    draw(cx);
    assert!(cx.debug_bounds("ai-review-active-second.md").is_some());
    assert!(
        cx.debug_bounds("ai-copy-0").is_none(),
        "修改不经过复制回答操作"
    );
}

#[gpui::test]
fn ai_instruction_clipboard_shortcuts_do_not_consume_the_original_document_selection(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let original = "第一段原文\n\n第二段原文";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.into(), None));
    draw(cx);
    draw(cx);
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.select_document(cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session
            .input
            .update(cx, |input, cx| input.focus(window, cx));
    });
    draw(cx);
    draw(cx);
    cx.simulate_input("只复制这条指令");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a cmd-c"
    } else {
        "ctrl-a ctrl-c"
    });
    assert_eq!(
        cx.read(|cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some("只复制这条指令".into())
    );
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-x"
    } else {
        "ctrl-x"
    });
    editor.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), original));
}

#[gpui::test]
fn ai_review_document_supports_copy_without_modifying_the_original_or_the_draft(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let source = "原始的第一段\n\n保留的第二段";
    let proposed = source.replace("原始的第一段", "新的第一段");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    draw(cx);
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.select_document(cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let baseline = session.saved.workspace.documents[&id].clone();
        session
            .saved
            .workspace
            .changes
            .proposals
            .insert(id, crate::ai::Proposal::new(baseline, proposed.clone()));
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    let draft = editor.read_with(cx, |editor, _| {
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .tracked
            .clone()
    });
    let tracked_source = draft.read_with(cx, |editor, _| editor.buffer.text());
    let (start, end) = draft.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .visible_blocks()
            .iter()
            .find(|block| block.entity.read(cx).display_text() == "新的第一段")
            .unwrap()
            .entity
            .read(cx);
        let bounds = block.visible_range_bounds(0.."新的第一段".len()).unwrap();
        (
            gpui::point(bounds.left() + gpui::px(0.5), bounds.center().y),
            gpui::point(bounds.right() - gpui::px(0.5), bounds.center().y),
        )
    });
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::none());
    draw(cx);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    });
    assert_eq!(
        cx.read(|cx| cx.read_from_clipboard().and_then(|item| item.text())),
        Some("新的第一段".into())
    );
    cx.simulate_input("误输入");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-x backspace"
    } else {
        "ctrl-x backspace"
    });
    editor.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), source));
    draft.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), tracked_source)
    });
}

#[gpui::test]
fn ai_unchanged_checkpoints_keep_the_review_document_and_its_reading_position(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let baseline = session.saved.workspace.documents[&id].clone();
        session
            .saved
            .workspace
            .changes
            .proposals
            .insert(id, crate::ai::Proposal::new(baseline, "新文"));
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    let initial = editor.read_with(cx, |editor, _| {
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .proposed
            .clone()
    });
    editor.update(cx, |editor, cx| editor.refresh_ai_preview(cx));
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .review
                .as_ref()
                .unwrap()
                .proposed
                .entity_id(),
            initial.entity_id(),
            "未变化的检查点不能重建投影并重置阅读现场"
        );
    });
}

#[gpui::test]
fn ai_review_finishes_in_the_editor_without_leaving_the_old_selection_toolbar_or_task_bar(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "第一段\n\n第二段".into(), None));
    draw(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.select_document(cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let baseline = session.saved.workspace.documents[&id].clone();
        session.saved.workspace.changes.proposals.insert(
            id,
            crate::ai::Proposal::new(baseline, "修改后的内容"),
        );
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("editor-selection-toolbar").is_none(),
        "审阅时不能悬挂正式文档的旧选区工具栏"
    );
    let accept = cx.debug_bounds("ai-review-accept").unwrap().center();
    cx.simulate_click(accept, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-task-bar").is_none(),
        "最后一处处理完后归还正文空间"
    );
    assert!(cx.debug_bounds("ai-review-workspace").is_none());
}

#[gpui::test]
fn ai_opening_another_selection_preserves_the_previous_task_for_explicit_resumption(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "第一段\n\n第二段".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let task_id = session.saved.id.clone();
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let baseline = session.saved.workspace.documents[&id].clone();
        session.saved.workspace.changes.proposals.insert(
            id.clone(),
            crate::ai::Proposal::new(baseline, "待审阅修改"),
        );
        editor.select_document(cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_ref().unwrap();
        assert_ne!(session.saved.id, task_id, "新选区应发起独立任务");
        let previous = editor
            .ai_task_sessions
            .iter()
            .find(|session| session.saved.id == task_id)
            .unwrap();
        assert!(previous.saved.workspace.changes.proposals.contains_key(&id));
        let path = previous.saved.path().unwrap();
        editor.restore_ai_history(path, window, cx);
        let session = editor.ai_session.as_ref().unwrap();
        assert_eq!(session.saved.id, task_id);
        assert!(session.saved.workspace.changes.proposals.contains_key(&id));
    });
}

fn tab_review_fixture(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<Editor>,
    &mut VisualTestContext,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    initialize(cx);
    let directory = std::env::temp_dir().join(format!("velora-ai-tabs-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let first = directory.join("first.md");
    let second = directory.join("second.md");
    let middle = (0..24)
        .map(|index| format!("保留的段落 {index}。\n\n"))
        .collect::<String>();
    let source = format!("旧第一段。\n\n{middle}旧第二段。");
    std::fs::write(&first, &source).unwrap();
    std::fs::write(&second, "另外一篇").unwrap();
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, source, Some(first.clone())));
    editor.update_in(cx, |editor, window, cx| {
        crate::config::EditorSettings::install_test_settings(cx, false);
        crate::config::EditorSettings::set_autosave_in_memory(false, cx);
        editor.ensure_current_document_tab(cx);
        editor.open_workspace_file(second.clone(), window, cx);
        editor.open_workspace_file(first.clone(), window, cx);
    });
    draw(cx);
    draw(cx);
    (editor, cx, directory, first, second)
}

fn propose_tab_changes(editor: &gpui::Entity<Editor>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let baseline = session.saved.workspace.documents[&id].clone();
        let changed = baseline
            .source
            .replace("旧第一段。", "新第一段。")
            .replace("旧第二段。", "新第二段。");
        session
            .saved
            .workspace
            .changes
            .proposals
            .insert(id, crate::ai::Proposal::new(baseline, changed));
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    draw(cx);
}

#[gpui::test]
fn ai_returning_to_a_tab_restores_the_same_review_and_allows_acceptance_and_undo(
    cx: &mut TestAppContext,
) {
    let (editor, cx, directory, first, second) = tab_review_fixture(cx);
    let original = std::fs::read_to_string(&first).unwrap();
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx)
    });
    propose_tab_changes(&editor, cx);
    editor.update(cx, |editor, cx| editor.move_ai_review_change(true, cx));
    for _ in 0..5 {
        draw(cx);
    }
    let (tracked, position) = editor.read_with(cx, |editor, cx| {
        let review = editor.ai_session.as_ref().unwrap().review.as_ref().unwrap();
        assert_eq!(review.index, 1);
        (
            review.tracked.clone(),
            review.tracked.read(cx).scroll_handle.offset(),
        )
    });
    assert!(
        position.y < gpui::px(0.0),
        "夹具应定位到后面的修改，覆盖滚动恢复"
    );
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(second.clone(), window, cx)
    });
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-review-workspace").is_none());
    editor.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), "另外一篇"));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(first.clone(), window, cx)
    });
    draw(cx);
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-review-workspace").is_some(),
        "回到有待审阅修改的标签必须恢复红绿预览"
    );
    editor.read_with(cx, |editor, cx| {
        let review = editor.ai_session.as_ref().unwrap().review.as_ref().unwrap();
        assert_eq!(review.tracked.entity_id(), tracked.entity_id());
        assert_eq!(review.index, 1);
        assert_eq!(review.tracked.read(cx).scroll_handle.offset(), position);
        assert_eq!(editor.buffer.text(), original);
    });
    let accept = cx.debug_bounds("ai-review-accept").unwrap().center();
    cx.simulate_click(accept, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    editor.update_in(cx, |editor, window, cx| {
        assert_eq!(
            editor.buffer.text(),
            original.replace("旧第二段。", "新第二段。")
        );
        editor.on_undo(&crate::components::Undo, window, cx);
        assert_eq!(editor.buffer.text(), original);
    });
    assert_eq!(std::fs::read_to_string(first).unwrap(), original);
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_result_arriving_on_another_tab_waits_for_the_task_tab_to_be_reopened(
    cx: &mut TestAppContext,
) {
    let (editor, cx, directory, first, second) = tab_review_fixture(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.saved.running = true;
        editor.open_workspace_file(second.clone(), window, cx);
    });
    draw(cx);
    draw(cx);
    propose_tab_changes(&editor, cx);
    editor.update(cx, |editor, cx| {
        editor.ai_session.as_mut().unwrap().saved.running = false;
        cx.notify();
    });
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-review-workspace").is_none(),
        "迟到的结果不能覆盖其他标签的正文"
    );
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(first.clone(), window, cx)
    });
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-review-workspace").is_some());
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_explicitly_returning_to_the_document_stays_closed_after_a_tab_round_trip(
    cx: &mut TestAppContext,
) {
    let (editor, cx, directory, first, second) = tab_review_fixture(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx)
    });
    propose_tab_changes(&editor, cx);
    let options = cx.debug_bounds("ai-options").unwrap().center();
    cx.simulate_click(options, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    let leave = menu_command_position(cx, |strings| &strings.ai_task_return);
    cx.simulate_click(leave, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(second.clone(), window, cx);
        editor.open_workspace_file(first.clone(), window, cx);
    });
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-review-workspace").is_none(),
        "主动返回正文后不强制重新打开预览"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_returning_to_a_tab_in_a_split_group_restores_the_host_review(cx: &mut TestAppContext) {
    let (editor, cx, directory, first, second) = tab_review_fixture(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.split_active_document(
            crate::editor::workspace::split::SplitDirection::Right,
            window,
            cx,
        )
    });
    draw(cx);
    draw(cx);
    let active = editor.read_with(cx, |editor, _| editor.active_split_editor().unwrap());
    active.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx)
    });
    cx.run_until_parked();
    propose_tab_changes(&editor, cx);
    active.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(second.clone(), window, cx)
    });
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-review-workspace").is_none());
    active.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(first.clone(), window, cx)
    });
    draw(cx);
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-review-workspace").is_some(),
        "分屏标签回切也要恢复宿主中的审阅"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_reference_picker_keeps_multiple_selections_visible_until_done(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Context;
        session.context_candidates = vec!["alpha.md".into(), "beta.md".into()];
    });
    draw(cx);
    draw(cx);
    cx.update(|window, cx| window.click("ai-context-source:alpha.md", cx));
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.ai_session.as_ref().unwrap().mode == PanelMode::Context,
            "选择一篇资料后不应立即关闭选择器"
        )
    });
    cx.update(|window, cx| window.click("ai-context-source:beta.md", cx));
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.ai_session.as_ref().unwrap().saved.references,
            ["alpha.md", "beta.md"]
        )
    });
    cx.update(|window, cx| window.click("ai-context-source:alpha.md", cx));
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.ai_session.as_ref().unwrap().saved.references,
            ["beta.md"]
        )
    });
    let done = cx.debug_bounds("ai-context-done").unwrap().center();
    cx.simulate_click(done, gpui::Modifiers::none());
    draw(cx);
    assert!(cx.debug_bounds("ai-composer").is_some());
}

fn native_ui_chain(cx: &mut TestAppContext, command: AiCommand) {
    let reading = command.is_reading_task();
    let writing = command == AiCommand::Polish;
    use serde_json::json;
    use std::io::Write;
    initialize(cx);
    let directory =
        std::env::temp_dir().join(format!("velora-ai-native-chain-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let source_path = directory.join("note.md");
    std::fs::write(&source_path, "# 原文\n\n保留的内容").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for tool in if writing {
            vec![false]
        } else {
            vec![true, false]
        } {
            let started = std::time::Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && started.elapsed() < std::time::Duration::from_secs(10) =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(2))
                    }
                    Err(error) => panic!("没有收到 UI 发起的模型请求：{error}"),
                }
            };
            requests.push(crate::ai::runtime::protocol_tests::read_request(&mut stream).1);
            let delta = if tool {
                json!({"role":"assistant","tool_calls":[{"index":0,"id":"create_note","type":"function","function":{"name":"propose_document","arguments":json!({"path":"generated.md","content":"# 新文档\n\n**完整排版的建议**"}).to_string()}}]})
            } else {
                json!({"role":"assistant","content":if writing {"这段表达已经清晰，无需修改。"} else {"已准备新文档。"}})
            };
            let first = json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            let last = json!({"choices":[{"index":0,"delta":{},"finish_reason":if tool {"tool_calls"} else {"stop"}}]});
            let body = format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n");
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    let endpoint = crate::ai::settings::Endpoint {
        id: "local-fixture".into(),
        name: "本地测试".into(),
        base_url: format!("http://{address}/v1"),
        model: "fixture".into(),
        ..Default::default()
    };
    crate::ai::settings::AiSettings {
        default_endpoint: endpoint.id.clone(),
        endpoints: vec![endpoint],
        ..Default::default()
    }
    .save_at(&crate::ai::settings::settings_path().unwrap())
    .unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "# 原文\n\n保留的内容".into(), Some(source_path.clone()))
    });
    draw(cx);
    draw(cx);
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        if command != AiCommand::Ask {
            editor.select_document(cx);
        }
        editor.open_ai_assistant(command, window, cx);
    });
    draw(cx);
    if command == AiCommand::Ask {
        cx.simulate_input("新建一篇笔记，等待我审阅");
        cx.simulate_keystrokes("enter");
    }
    for _ in 0..10 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        draw(cx);
    }
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), if writing { 1 } else { 2 });
    assert!(requests[0].to_string().contains("workspace_path"));
    assert!(requests[0].to_string().contains("open_tabs"));
    if writing {
        let prompt = requests[0]
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .unwrap()
            .iter()
            .filter(|message| {
                message.get("role").and_then(serde_json::Value::as_str) == Some("user")
            })
            .filter_map(|message| message.get("content").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            prompt.contains(crate::ai::instructions::default_instruction(AiCommand::Polish, "en-US")),
            "快捷指令必须把完整任务要求发给模型"
        );
        assert!(prompt.contains("propose_edit"));
        assert!(prompt.contains("Do not repeat"));
        assert!(prompt.contains("# 原文\n\n保留的内容"));
        let tools = requests[0].get("tools").unwrap().to_string();
        assert!(tools.contains("propose_edit"));
        assert!(!tools.contains("propose_document"));
        assert!(!tools.contains("propose_insert_at_cursor"));
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.buffer.text(), "# 原文\n\n保留的内容")
        });
        assert!(cx.debug_bounds("ai-review-workspace").is_none());
        editor.read_with(cx, |editor, _| {
            assert!(
                editor
                    .ai_session
                    .as_ref()
                    .unwrap()
                    .saved
                    .workspace
                    .changes
                    .proposals
                    .is_empty()
            )
        });
        assert_eq!(
            std::fs::read_to_string(source_path).unwrap(),
            "# 原文\n\n保留的内容"
        );
        std::fs::remove_dir_all(directory).unwrap();
        return;
    }
    if reading {
        let tools = requests[0]
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert!(
            !tools
                .iter()
                .any(|tool| tool.to_string().contains("propose_")
                    || tool.to_string().contains("reset_proposal"))
        );
        assert!(cx.debug_bounds("ai-dialog-result").is_some());
        assert!(cx.debug_bounds("ai-review-workspace").is_none());
        assert!(cx.debug_bounds("ai-review-file-accept").is_none());
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.buffer.text(), "# 原文\n\n保留的内容");
            assert!(
                editor
                    .ai_session
                    .as_ref()
                    .unwrap()
                    .saved
                    .workspace
                    .changes
                    .proposals
                    .is_empty()
            );
        });
        assert!(!directory.join("generated.md").exists());
        std::fs::remove_dir_all(directory).unwrap();
        return;
    }
    assert!(requests[1].to_string().contains("generated.md"));
    assert!(
        cx.debug_bounds("ai-review-workspace").is_some(),
        "模型的工具结果必须进入正文审阅区"
    );
    assert!(cx.debug_bounds("ai-content").is_none());
    assert!(!directory.join("generated.md").exists());
    let accept = cx.debug_bounds("ai-review-file-accept").unwrap().center();
    cx.simulate_click(accept, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-review-workspace").is_none());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "# 原文\n\n保留的内容");
        assert!(
            editor
                .ai_cached_document_sources()
                .iter()
                .any(|(path, content)| path.ends_with("generated.md")
                    && content.contains("**完整排版的建议**"))
        );
    });
    assert!(
        !directory.join("generated.md").exists(),
        "接受新文档也必须走未保存标签"
    );
    assert_eq!(
        std::fs::read_to_string(source_path).unwrap(),
        "# 原文\n\n保留的内容"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
fn ai_instruction_to_native_tool_to_document_review_completes_through_the_ui(
    cx: &mut TestAppContext,
) {
    native_ui_chain(cx, AiCommand::Ask);
}

#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
fn ai_reading_request_omits_write_tools_and_cannot_apply_an_unsolicited_model_edit(
    cx: &mut TestAppContext,
) {
    native_ui_chain(cx, AiCommand::Explain);
}

#[gpui::test]
fn ai_review_acceptance_from_the_split_workspace_updates_both_views(cx: &mut TestAppContext) {
    initialize(cx);
    let directory =
        std::env::temp_dir().join(format!("velora-ai-review-group-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("note.md");
    std::fs::write(&path, "原文").unwrap();
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文".into(), Some(path.clone())));
    editor.update_in(cx, |editor, window, cx| {
        editor.split_active_document(
            crate::editor::workspace::split::SplitDirection::Right,
            window,
            cx,
        )
    });
    draw(cx);
    draw(cx);
    let active = editor.read_with(cx, |editor, _| editor.active_split_editor().unwrap());
    active.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx)
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let baseline = session.saved.workspace.documents[&id].clone();
        session.saved.workspace.changes.proposals.insert(
            id,
            crate::ai::Proposal::new(baseline, "修订后的文字"),
        );
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-review-workspace").is_some());
    let accept = cx.debug_bounds("ai-review-accept").unwrap().center();
    cx.simulate_click(accept, gpui::Modifiers::none());
    cx.run_until_parked();
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-review-workspace").is_none());
    editor.read_with(cx, |editor, cx| {
        for group in editor.split_groups() {
            assert_eq!(group.read(cx).buffer.text(), "修订后的文字");
        }
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "原文");
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_review_accepts_the_focused_change_with_the_platform_enter_shortcut(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let baseline = session.saved.workspace.documents[&id].clone();
        session.saved.workspace.changes.proposals.insert(
            id,
            crate::ai::Proposal::new(baseline, "改写后的文字"),
        );
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    editor.update_in(cx, |editor, window, cx| {
        window.focus(&editor.ai_session.as_ref().unwrap().review_focus, cx)
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-enter"
    } else {
        "ctrl-enter"
    });
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "改写后的文字")
    });
    assert!(cx.debug_bounds("ai-review-workspace").is_none());
}

#[gpui::test]
fn ai_stopping_a_response_keeps_the_received_text_readable_in_the_dialog(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.saved.running = true;
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "# 已完成的部分\n\n保留这段说明。".into(),
            });
        editor.stop_ai_task(cx);
    });
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-dialog-result").is_some(),
        "停止后仍然可以在浮窗阅读已收到的内容"
    );
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "原文");
        assert!(editor.ai_session.as_ref().unwrap().saved.interrupted);
    });
}

#[gpui::test]
fn ai_command_validation_errors_are_visible_before_the_task_starts(cx: &mut TestAppContext) {
    initialize(cx);
    let endpoint = crate::ai::settings::Endpoint {
        id: "invalid-fixture".into(),
        base_url: "invalid-url".into(),
        ..Default::default()
    };
    crate::ai::settings::AiSettings {
        default_endpoint: endpoint.id.clone(),
        endpoints: vec![endpoint],
        ..Default::default()
    }
    .save_at(&crate::ai::settings::settings_path().unwrap())
    .unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
    });
    draw(cx);
    cx.simulate_input("总结本文");
    cx.simulate_keystrokes("enter");
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, _| {
        let session = editor.ai_session.as_ref().unwrap();
        assert!(session.error.is_some());
        assert!(session.saved.messages.is_empty());
    });
    assert!(
        cx.debug_bounds("ai-command-error").is_some(),
        "发送前的校验错误必须在指令栏显示，不能表现为点击无反应"
    );
}

fn reading_command_is_not_an_edit(cx: &mut TestAppContext, command: AiCommand) {
    initialize(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "选中的原文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.select_document(cx);
        editor.open_ai_assistant(command, window, cx);
        assert!(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .saved
                .workspace
                .read_only,
            "总结、解释、翻译只能生成阅读结果"
        );
        assert_eq!(editor.buffer.text(), "选中的原文");
    });
}

#[gpui::test]
fn ai_reading_summary_never_starts_a_document_edit(cx: &mut TestAppContext) {
    reading_command_is_not_an_edit(cx, AiCommand::Summarize);
}
#[gpui::test]
fn ai_reading_explanation_never_starts_a_document_edit(cx: &mut TestAppContext) {
    reading_command_is_not_an_edit(cx, AiCommand::Explain);
}
#[gpui::test]
fn ai_reading_translation_never_starts_a_document_edit(cx: &mut TestAppContext) {
    reading_command_is_not_an_edit(cx, AiCommand::Translate);
}

#[gpui::test]
fn ai_reading_result_stays_in_a_floating_window_with_no_insertion_or_bottom_bars(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文不能变成解释稿".into(), None));
    draw(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.saved.workspace.read_only = true;
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "## 解释\n\n只供阅读的结果。".into(),
            });
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-floating-workbench").is_some(),
        "结果应在浮动工作台中阅读"
    );
    assert!(cx.debug_bounds("ai-result-insert").is_none());
    assert!(cx.debug_bounds("ai-task-bar").is_none());
    assert!(
        cx.debug_bounds("ai-result-document").is_none(),
        "阅读结果不能顶替正文"
    );
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "原文不能变成解释稿")
    });
}

#[gpui::test]
fn ai_editor_context_contains_workspace_tabs_active_cursor_and_selection(cx: &mut TestAppContext) {
    initialize(cx);
    let root =
        std::env::temp_dir().join(format!("velora-ai-context-state-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let first = root.join("first.md");
    let second = root.join("second.md");
    std::fs::write(&first, "第一篇").unwrap();
    std::fs::write(&second, "# 标题\n\n选中的文字").unwrap();
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "第一篇".into(), Some(first.clone())));
    editor.update_in(cx, |editor, window, cx| {
        editor.ensure_current_document_tab(cx);
        editor.open_workspace_file(second.clone(), window, cx);
        editor.select_document(cx);
        let context = editor.ai_editor_context(cx);
        assert_eq!(
            context["workspace_path"].as_str(),
            Some(root.canonicalize().unwrap().to_str().unwrap())
        );
        let tabs = context["open_tabs"]
            .as_array()
            .expect("应提供全部标签页元数据");
        assert_eq!(tabs.len(), 2);
        assert!(
            tabs.iter()
                .any(|tab| tab["path"].as_str() == first.to_str())
        );
        assert!(
            tabs.iter()
                .any(|tab| tab["path"].as_str() == second.to_str())
        );
        assert_eq!(context["active_tab"]["path"].as_str(), second.to_str());
        assert_eq!(context["cursor"]["line"], 3);
        assert_eq!(context["selection"]["text"], "# 标题\n\n选中的文字");
        assert!(context["cursor"]["byte_offset"].as_u64().unwrap() > 0);
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn ai_quick_commands_require_selection_except_summary(cx: &mut TestAppContext) {
    initialize(cx);
    for command in [
        AiCommand::Explain,
        AiCommand::Translate,
        AiCommand::Polish,
        AiCommand::Expand,
    ] {
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::from_markdown(cx, "# 当前文档\n\n需要处理的正文".into(), None)
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.open_ai_assistant(command, window, cx);
            assert!(
                editor.ai_session.is_none(),
                "必须明确选择处理范围：{command:?}"
            );
            assert!(editor.modal_is_open());
        });
    }
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "全文可以摘要".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Summarize, window, cx);
        assert!(editor.ai_session.is_some());
        assert!(!editor.modal_is_open());
    });
}

#[test]
fn ai_inline_review_keeps_deleted_and_inserted_blocks_at_their_document_position() {
    use super::inline_review::{MarkKind, tracked_document};
    let before_parts = ["# 标题\n\n", "旧段落。\n\n", "保留正文。\n"];
    let after_parts = [
        "# 标题\n\n",
        "新段落。\n\n",
        "保留正文。\n\n",
        "补充的内容。\n",
    ];
    let spans = |parts: &[&str]| {
        let mut offset = 0;
        parts
            .iter()
            .map(|part| {
                let start = offset;
                offset += part.len();
                start..offset
            })
            .collect::<Vec<_>>()
    };
    let before = before_parts.concat();
    let after = after_parts.concat();
    let proposal = crate::ai::Proposal::new(
        crate::ai::DocumentSnapshot {
            id: "note.md".into(),
            source: before.clone().into(),
        },
        after.clone(),
    );
    let (text, marks) = tracked_document(
        &before,
        &spans(&before_parts),
        &after,
        &spans(&after_parts),
        &proposal.edits(),
    )
    .unwrap();
    assert!(text.contains("旧段落。"), "删除部分必须留在原位置供审阅");
    assert!(text.find("旧段落。").unwrap() < text.find("新段落。").unwrap());
    // 保留正文的收尾空行在 after 里多了一个换行：仅空行的差异现在也以
    // 红/绿对呈现，正文本身因此出现两份（删除份与新增份）。
    assert_eq!(text.matches("保留正文。").count(), 2);
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Removed
        && text.get(mark.range.clone()).unwrap().contains("旧段落")));
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Added
        && text.get(mark.range.clone()).unwrap().contains("新段落")));
    assert!(marks.iter().any(|mark| mark.kind == MarkKind::Added
        && text.get(mark.range.clone()).unwrap().contains("补充")));
    assert!(marks.iter().all(|mark| !mark.edits.is_empty()));
}

#[test]
fn ai_inline_review_shows_a_pure_deletion_instead_of_an_empty_document() {
    use super::inline_review::{MarkKind, tracked_document};
    let before = "应被删除的段落。\n";
    let proposal = crate::ai::Proposal::new(
        crate::ai::DocumentSnapshot {
            id: "note.md".into(),
            source: before.into(),
        },
        String::new(),
    );
    let (text, marks) =
        tracked_document(before, &[0..before.len()], "", &[], &proposal.edits()).unwrap();
    assert_eq!(text, before);
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].kind, MarkKind::Removed);
}

#[gpui::test]
fn ai_conversation_keeps_secondary_commands_out_of_the_reading_surface(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "阅读结果".into(),
            });
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-command-bar").is_none(),
        "已开始对话后不重复铺满快捷指令"
    );
    assert!(cx.debug_bounds("ai-composer").is_some());
    assert!(cx.debug_bounds("ai-dialog-result").is_some());
}

#[gpui::test]
fn ai_floating_dialog_drag_moves_only_until_mouse_release(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor.ai_session.as_mut().unwrap().mode = PanelMode::Task;
    });
    draw(cx);
    draw(cx);
    let before = cx.debug_bounds("ai-floating-workbench").unwrap();
    let start = cx.debug_bounds("ai-float-titlebar").unwrap().center();
    let end = start + gpui::point(gpui::px(30.0), gpui::px(30.0));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    let after = cx.debug_bounds("ai-floating-workbench").unwrap();
    assert_eq!(
        after.origin,
        before.origin + gpui::point(gpui::px(30.0), gpui::px(30.0))
    );
    cx.simulate_mouse_move(
        end + gpui::point(gpui::px(20.0), gpui::px(20.0)),
        None,
        gpui::Modifiers::none(),
    );
    draw(cx);
    assert_eq!(
        cx.debug_bounds("ai-floating-workbench").unwrap().origin,
        after.origin
    );
    editor.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), "正文"));
}

#[gpui::test]
fn ai_selected_writing_accepts_each_change_and_undo_restores_only_that_change(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let source = "前文不变\n\n框架甲\n\n框架乙\n\n后文不变";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    editor.update_in(cx, |editor, window, cx| {
        let start = source.find("框架甲").unwrap();
        let end = source.find("框架乙").unwrap() + "框架乙".len();
        editor.jump_to_document_search_range(start..end, cx);
        editor.open_ai_assistant(AiCommand::Expand, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        assert_eq!(
            session
                .saved
                .workspace
                .selection_scope
                .as_ref()
                .unwrap()
                .range,
            start..end
        );
        session
            .saved
            .workspace
            .call(
                "propose_edit",
                &serde_json::json!({"path":id,"before":"框架甲","after":"框架甲与说明🙂"}),
            )
            .unwrap();
        session
            .saved
            .workspace
            .call(
                "propose_edit",
                &serde_json::json!({"path":id,"before":"框架乙","after":"框架乙与例子"}),
            )
            .unwrap();
        assert!(
            session
                .saved
                .workspace
                .call(
                    "propose_edit",
                    &serde_json::json!({"path":id,"before":"前文不变","after":"越界"})
                )
                .is_err()
        );
        editor
            .accept_ai_proposals(Some(&id), Some(0), window, cx)
            .unwrap();
        assert_eq!(
            editor.buffer.text(),
            source.replace("框架甲", "框架甲与说明🙂")
        );
        editor
            .accept_ai_proposals(Some(&id), Some(0), window, cx)
            .unwrap();
        assert_eq!(
            editor.buffer.text(),
            source
                .replace("框架甲", "框架甲与说明🙂")
                .replace("框架乙", "框架乙与例子")
        );
        editor.on_undo(&crate::components::Undo, window, cx);
        assert_eq!(
            editor.buffer.text(),
            source.replace("框架甲", "框架甲与说明🙂")
        );
        editor.on_undo(&crate::components::Undo, window, cx);
        assert_eq!(editor.buffer.text(), source);
    });
}

#[gpui::test]
fn ai_selected_writing_cannot_accept_an_out_of_scope_proposal(cx: &mut TestAppContext) {
    initialize(cx);
    let source = "前文不变\n\n选区内容\n\n后文不变";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    editor.update_in(cx, |editor, window, cx| {
        let start = source.find("选区内容").unwrap();
        editor.jump_to_document_search_range(start..start + "选区内容".len(), cx);
        editor.open_ai_assistant(AiCommand::Polish, window, cx);
        let workspace = &mut editor.ai_session.as_mut().unwrap().saved.workspace;
        workspace.changes.proposals.insert(
            "other.md".into(),
            crate::ai::Proposal::new(
                crate::ai::DocumentSnapshot {
                    id: "other.md".into(),
                    source: std::sync::Arc::from("其他文档"),
                },
                "越界修改",
            ),
        );
        let result = editor.accept_ai_proposals(None, None, window, cx);
        assert!(result.unwrap_err().to_string().contains("选区"));
        assert_eq!(editor.buffer.text(), source);
    });
}

#[gpui::test]
fn ai_floating_dialog_preserves_writing_width_and_docking_keeps_the_conversation(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "内容".into(), None));
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-assistant-panel").is_none());
    let initial = cx.debug_bounds("editor-scroll").unwrap();
    let id = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(crate::ai::AiCommand::Ask, window, cx);
        editor.ai_session.as_ref().unwrap().saved.id.clone()
    });
    draw(cx);
    draw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.scroll_handle.bounds().size.width),
        initial.size.width
    );
    let panel = cx.debug_bounds("ai-assistant-panel").unwrap();
    let viewport = cx.update(|window, _| window.viewport_size());
    assert!(panel.left() >= gpui::px(0.0) && panel.right() <= viewport.width);
    assert!(panel.top() >= gpui::px(0.0) && panel.bottom() <= viewport.height);
    if let Some(back) = cx.debug_bounds("ai-back") {
        cx.simulate_click(back.center(), gpui::Modifiers::none());
    }
    editor.update(cx, |editor, cx| {
        editor
            .ai_session
            .as_mut()
            .unwrap()
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "user".into(),
                text: "整理当前文档".into(),
            });
        editor.refresh_ai_preview(cx);
        cx.notify();
    });
    draw(cx);
    draw(cx);
    let conversation = editor.read_with(cx, |editor, _| {
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .conversation
            .clone()
            .unwrap()
    });
    let expand = cx.debug_bounds("ai-expand").unwrap().center();
    cx.simulate_click(expand, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    assert!(
        editor.read_with(cx, |editor, _| editor.scroll_handle.bounds().size.width)
            < initial.size.width
    );
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.ai_session.as_ref().unwrap().saved.id, id);
        assert_eq!(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .conversation
                .as_ref()
                .unwrap()
                .entity_id(),
            conversation.entity_id()
        )
    });
    let collapse = cx.debug_bounds("ai-expand").unwrap().center();
    cx.simulate_click(collapse, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.scroll_handle.bounds().size.width),
        initial.size.width
    );
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.ai_session.as_ref().unwrap().saved.id, id);
        assert_eq!(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .conversation
                .as_ref()
                .unwrap()
                .entity_id(),
            conversation.entity_id()
        )
    });
}

#[gpui::test]
fn ai_retargeted_conversation_does_not_keep_a_previous_selection_boundary(cx: &mut TestAppContext) {
    initialize(cx);
    let source = "第一段\n\n第二段";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.jump_to_document_search_range(0.."第一段".len(), cx);
        editor.open_ai_assistant(AiCommand::Polish, window, cx);
        let id = editor.ai_session.as_ref().unwrap().saved.id.clone();
        let start = source.find("第二段").unwrap();
        editor.jump_to_document_search_range(start..start, cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_ref().unwrap();
        assert_ne!(session.saved.id, id);
        assert!(
            session.saved.workspace.selection_scope.is_none(),
            "新目标不能被旧选区限制"
        );
        assert!(!session.saved.workspace.read_only);
    });
}

#[gpui::test]
fn ai_default_context_does_not_include_the_contents_of_an_excluded_active_document(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "不应发送的正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.select_document(cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        session.saved.workspace.excluded_paths = vec![id];
        let context = editor.ai_editor_context(cx);
        assert!(!context.to_string().contains("不应发送的正文"));
        assert!(context.get("active_tab").is_some());
    });
}

#[gpui::test]
fn ai_more_menu_preserves_the_conversation_and_escape_dismisses_only_the_menu(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session
            .input
            .update(cx, |input, cx| input.set_value("保留输入", window, cx));
    });
    draw(cx);
    draw(cx);
    let options = cx.debug_bounds("ai-options").unwrap().center();
    cx.simulate_click(options, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(editor.ai_session.as_ref().unwrap().mode == PanelMode::Task,
            "更多菜单应覆盖在对话上，不能切走对话")
    });
    cx.update(|window, _| {
        use gpui_kit::test::TestWindowExt as _;
        let menu = window.find("popup-menu");
        assert_eq!(menu.role(), Some(gpui::Role::Menu));
        assert_eq!(menu.focused(), Some(true), "原生菜单应获取键盘焦点");
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-composer").is_some(),
        "Esc 应先退出次级菜单"
    );
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.ai_session.as_ref().unwrap().input.read(cx).value(),
            "保留输入"
        )
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.debug_bounds("ai-floating-workbench").is_none());
}

#[gpui::test]
fn ai_discard_menu_removes_pending_revisions_and_preserves_the_document(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        session
            .saved
            .workspace
            .changes
            .propose(
                session.saved.workspace.documents[&id].clone(),
                "原文",
                "待审阅的修改",
            )
            .unwrap();
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    let options = cx.debug_bounds("ai-options").unwrap().center();
    cx.simulate_click(options, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    let menu_bounds = cx.debug_bounds("ai-assistant-panel").unwrap();
    draw(cx);
    assert_eq!(
        cx.debug_bounds("ai-assistant-panel").unwrap(),
        menu_bounds,
        "次级菜单不能在重绘时自行移动"
    );
    let discard = menu_command_position(cx, |strings| &strings.ai_panel_discard);
    cx.simulate_click(discard, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-review-workspace").is_none());
    assert!(cx.debug_bounds("ai-composer").is_some());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "原文");
        assert!(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .saved
                .workspace
                .changes
                .proposals
                .is_empty()
        );
    });
}

#[gpui::test]
fn ai_visible_controls_resolve_through_the_production_asset_source(cx: &mut TestAppContext) {
    use gpui::AssetSource;
    for path in [
        "icon/editor/ai.svg",
        "icons/arrow-left.svg",
        "icons/panel-right-open.svg",
        "icons/external-link.svg",
        "icons/ellipsis.svg",
        "icons/rotate-cw.svg",
        "icons/arrow-up.svg",
        "icons/square.svg",
        "icons/trash.svg",
    ] {
        let bytes = crate::VeloraAssets.load(path).unwrap();
        assert!(bytes.is_some(), "产品资源中缺少图标：{path}");
        let image = gpui::Image::from_bytes(gpui::ImageFormat::Svg, bytes.unwrap().into_owned());
        let pixels = cx
            .update(|cx| image.to_image_data(cx.svg_renderer()))
            .unwrap();
        assert!(
            pixels
                .as_bytes(0)
                .unwrap()
                .chunks_exact(4)
                .any(|pixel| pixel[3] > 0),
            "图标必须实际绘制可见像素：{path}"
        );
    }
}

fn writing_dialog(cx: &mut TestAppContext) -> (gpui::Entity<Editor>, &mut VisualTestContext) {
    initialize(cx);
    let source = "# 标题\n\n需要润色的原始段落。\n\n保留的下文。";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let base = session.saved.workspace.documents[&id].clone();
        session
            .saved
            .workspace
            .changes
            .propose(base, "需要润色的原始段落。", "润色后的段落。")
            .unwrap();
        session.saved.messages.extend([
            crate::ai::session::ChatMessage {
                role: "user".into(),
                text: "润色".into(),
            },
            crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "已提出一处修改。".into(),
            },
        ]);
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    (editor, cx)
}

#[gpui::test]
fn ai_writing_dialog_can_be_dragged_from_the_top_padding_through_multiple_frames(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = writing_dialog(cx);
    let before = cx.debug_bounds("ai-floating-workbench").unwrap();
    let start = before.origin + gpui::point(gpui::px(100.0), gpui::px(6.0));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::none());
    for (horizontal, vertical) in [(35.0, 70.0), (80.0, 90.0), (120.0, 60.0)] {
        let delta = gpui::point(gpui::px(horizontal), gpui::px(vertical));
        cx.simulate_mouse_move(
            start + delta,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::none(),
        );
        draw(cx);
        draw(cx);
        assert_eq!(
            cx.debug_bounds("ai-floating-workbench").unwrap().origin,
            before.origin + delta,
            "整个顶部应可抓取，正文或对话编辑器不能截断拖动"
        );
    }
    let end = start + gpui::point(gpui::px(120.0), gpui::px(60.0));
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::none());
    draw(cx);
    let released = cx.debug_bounds("ai-floating-workbench").unwrap();
    cx.simulate_mouse_move(
        end + gpui::point(gpui::px(40.0), gpui::px(40.0)),
        None,
        gpui::Modifiers::none(),
    );
    draw(cx);
    assert_eq!(cx.debug_bounds("ai-floating-workbench").unwrap(), released);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.buffer.text(),
            "# 标题\n\n需要润色的原始段落。\n\n保留的下文。"
        )
    });
}

#[gpui::test]
fn ai_writing_dialog_keeps_model_selection_in_settings_and_omits_single_change_navigation(
    cx: &mut TestAppContext,
) {
    let (_editor, cx) = writing_dialog(cx);
    assert!(
        cx.debug_bounds("ai-models").is_none(),
        "模型选择只放在设置里"
    );
    assert!(
        cx.debug_bounds("ai-review-files").is_none(),
        "单篇文件不重复显示文件列表"
    );
    assert!(cx.debug_bounds("ai-review-previous").is_none());
    assert!(cx.debug_bounds("ai-review-next").is_none());
    assert!(
        f32::from(
            cx.debug_bounds("ai-floating-workbench")
                .unwrap()
                .size
                .height
        ) <= 360.0,
        "正文已有差异时，浮窗应保留阅读空间"
    );
}

#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
fn ai_polish_sends_full_task_requirements_and_keeps_the_document_unchanged_without_a_proposal(
    cx: &mut TestAppContext,
) {
    native_ui_chain(cx, AiCommand::Polish);
}

#[gpui::test]
fn ai_sidebar_composer_uses_the_available_width_in_both_sidebar_sizes(cx: &mut TestAppContext) {
    let (_editor, cx) = writing_dialog(cx);
    let dock = cx.debug_bounds("ai-expand").unwrap().center();
    cx.simulate_click(dock, gpui::Modifiers::none());
    for width in [1920.0, 1000.0] {
        cx.simulate_resize(gpui::size(gpui::px(width), gpui::px(1080.0)));
        draw(cx);
        draw(cx);
        draw(cx);
        let sidebar = cx.debug_bounds("ai-sidebar").unwrap();
        let composer = cx.debug_bounds("ai-composer").unwrap();
        let input = cx.debug_bounds("ai-composer-input").unwrap();
        let send = cx.debug_bounds("ai-send").unwrap();
        assert!(
            input.size.width >= sidebar.size.width - gpui::px(64.0),
            "侧栏输入框不能按空内容收缩为窄条"
        );
        assert!(send.right() <= sidebar.right() - gpui::px(8.0));
        assert!(
            send.right() >= sidebar.right() - gpui::px(32.0),
            "发送按钮应靠齐侧栏右侧"
        );
        assert!(composer.size.height <= gpui::px(100.0));
        assert!(send.top() >= input.bottom(), "发送按钮在卡片底部，不挤占输入行");
        assert!(send.bottom() <= composer.bottom());
    }
}

#[gpui::test]
fn ai_inline_controls_stay_below_the_changed_text_and_sidebar_acceptance_aligns_right(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = writing_dialog(cx);
    let dock = cx.debug_bounds("ai-expand").unwrap().center();
    cx.simulate_click(dock, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    draw(cx);
    let block = editor.read_with(cx, |editor, cx| {
        let tracked = editor
            .ai_session
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .tracked
            .read(cx);
        tracked
            .document
            .root_blocks()
            .iter()
            .find(|block| block.read(cx).display_text() == "润色后的段落。")
            .unwrap()
            .read(cx)
            .last_bounds
            .unwrap()
    });
    let controls = cx.debug_bounds("ai-inline-controls").unwrap();
    assert!(
        controls.top() >= block.bottom(),
        "审阅操作不能浮在原文与改文之间或盖住段落"
    );
    let actions = cx.debug_bounds("ai-review-actions").unwrap();
    let accept = cx.debug_bounds("ai-review-file-accept").unwrap();
    assert!(
        (actions.right() - accept.right()).abs() <= gpui::px(1.0),
        "接受操作应在右侧，对齐输入区"
    );
}

#[gpui::test]
fn ai_review_can_be_left_and_reopened_from_the_more_menu(cx: &mut TestAppContext) {
    let (_editor, cx) = writing_dialog(cx);
    let dock = cx.debug_bounds("ai-expand").unwrap().center();
    cx.simulate_click(dock, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    let sidebar = cx.debug_bounds("ai-sidebar").unwrap();
    for visible in [false, true] {
        let options = cx.debug_bounds("ai-options").unwrap().center();
        cx.simulate_click(options, gpui::Modifiers::none());
        draw(cx);
        draw(cx);
        assert_eq!(
            cx.debug_bounds("ai-sidebar"),
            Some(sidebar),
            "侧栏菜单不能收走侧栏使正文重新排版"
        );
        assert!(
            cx.debug_bounds("ai-floating-workbench").is_none(),
            "停靠后在侧栏内切换菜单"
        );
        let toggle = menu_command_position(cx, |strings| {
            if visible { &strings.ai_review_title } else { &strings.ai_task_return }
        });
        cx.simulate_click(toggle, gpui::Modifiers::none());
        draw(cx);
        draw(cx);
        assert_eq!(cx.debug_bounds("ai-review-workspace").is_some(), visible);
        assert!(
            cx.debug_bounds("ai-review-return").is_none(),
            "底部不重复摆放含义不明的返回箭头"
        );
    }
}

#[gpui::test]
fn ai_cmd_j_in_another_tab_starts_from_its_current_selection_and_preserves_both_tasks(
    cx: &mut TestAppContext,
) {
    let (editor, cx, directory, first, second) = tab_review_fixture(cx);
    let first_task = editor.update_in(cx, |editor, window, cx| {
        editor.jump_to_document_search_range(0.."旧第一段。".len(), cx);
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "user".into(),
                text: "A 的指令".into(),
            });
        session
            .input
            .update(cx, |input, cx| input.set_value("A 的草稿", window, cx));
        session.saved.id.clone()
    });
    propose_tab_changes(&editor, cx);
    let first_review = editor.read_with(cx, |editor, _| {
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .tracked
            .entity_id()
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(second.clone(), window, cx);
        editor.jump_to_document_search_range(0.."另外".len(), cx);
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx);
    });
    draw(cx);
    draw(cx);
    let second_task = editor.update_in(cx, |editor, window, cx| {
        let session = editor.ai_session.as_mut().unwrap();
        let target = session.saved.target.as_ref().unwrap();
        assert_eq!(
            target.path.as_ref(),
            Some(&second),
            "B 的入口不能继续使用 A 的目标"
        );
        assert_eq!(target.range, 0.."另外".len());
        assert_eq!(target.selection, "另外");
        assert_ne!(session.saved.id, first_task);
        assert!(session.saved.history.is_empty());
        assert!(session.saved.messages.is_empty());
        assert!(session.saved.workspace.changes.proposals.is_empty());
        assert_eq!(session.input.read(cx).value(), "");
        session.mode = PanelMode::Task;
        session
            .input
            .update(cx, |input, cx| input.set_value("B 的草稿", window, cx));
        session.saved.id.clone()
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(first.clone(), window, cx)
    });
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, cx| {
        let session = editor.ai_session.as_ref().unwrap();
        assert_eq!(session.saved.id, first_task);
        assert_eq!(session.input.read(cx).value(), "A 的草稿");
        assert_eq!(&*session.saved.messages.first().unwrap().text, "A 的指令");
        assert_eq!(
            session.review.as_ref().unwrap().tracked.entity_id(),
            first_review
        );
        assert!(session.review_open);
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(second, window, cx)
    });
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, cx| {
        let session = editor.ai_session.as_ref().unwrap();
        assert_eq!(session.saved.id, second_task);
        assert_eq!(session.input.read(cx).value(), "B 的草稿");
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_cmd_j_does_not_reuse_a_finished_conversation_from_another_tab(cx: &mut TestAppContext) {
    let (editor, cx, directory, _first, second) = tab_review_fixture(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.visible = false;
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "user".into(),
                text: "解释 A".into(),
            });
        session
            .saved
            .history
            .push(serde_json::json!({"role":"user", "content":"解释 A"}));
        editor.open_workspace_file(second.clone(), window, cx);
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx);
        let session = editor.ai_session.as_ref().unwrap();
        assert!(session.saved.messages.is_empty(), "B 不应显示 A 的对话");
        assert!(
            session.saved.history.is_empty(),
            "B 的请求不能携带 A 的模型历史"
        );
        assert_eq!(
            session.saved.target.as_ref().unwrap().path.as_ref(),
            Some(&second)
        );
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_cmd_j_in_a_split_uses_the_active_leaf_target(cx: &mut TestAppContext) {
    let (editor, cx, directory, _first, second) = tab_review_fixture(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor.split_active_document(
            crate::editor::workspace::split::SplitDirection::Right,
            window,
            cx,
        );
    });
    draw(cx);
    draw(cx);
    let active = editor.read_with(cx, |editor, _| editor.active_split_editor().unwrap());
    active.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(second.clone(), window, cx);
        editor.jump_to_document_search_range("另".len().."另外一篇".len(), cx);
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx);
    });
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, _| {
        let target = editor
            .ai_session
            .as_ref()
            .unwrap()
            .saved
            .target
            .as_ref()
            .unwrap();
        assert_eq!(target.path.as_ref(), Some(&second));
        assert_eq!(target.selection, "外一篇");
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_cmd_j_in_a_new_selection_of_the_same_tab_starts_an_independent_task(cx: &mut TestAppContext) {
    initialize(cx);
    let source = "第一处原文。\n\n第二处原文。";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.jump_to_document_search_range(0.."第一处原文。".len(), cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let first_task = session.saved.id.clone();
        let target = session.saved.target.as_ref().unwrap().clone();
        session.saved.workspace.selection_scope = Some(crate::ai::workspace::SelectionScope {
            document_id: target.document_id.clone(),
            range: target.range,
            version: target.version,
        });
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "user".into(),
                text: "只润色第一处".into(),
            });
        let baseline = session.saved.workspace.documents[&target.document_id].clone();
        session.saved.workspace.changes.proposals.insert(
            target.document_id,
            crate::ai::Proposal::new(baseline, source.replace("第一处原文", "第一处建议")),
        );
        session
            .input
            .update(cx, |input, cx| input.set_value("第一处的补充要求", window, cx));
        let start = source.find("第二处").unwrap();
        editor.jump_to_document_search_range(start..source.len(), cx);
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx);
        let session = editor.ai_session.as_ref().unwrap();
        assert_ne!(
            session.saved.id, first_task,
            "同一标签的新选区不能沿用旧任务"
        );
        assert_eq!(
            session.saved.target.as_ref().unwrap().selection,
            "第二处原文。"
        );
        assert!(session.saved.messages.is_empty());
        assert!(session.saved.history.is_empty());
        assert!(session.saved.workspace.selection_scope.is_none());
        assert_eq!(session.input.read(cx).value(), "");
        assert_eq!(editor.buffer.text(), source);
        let previous = editor
            .ai_task_sessions
            .iter()
            .find(|session| session.saved.id == first_task)
            .unwrap();
        assert!(!previous.saved.workspace.changes.proposals.is_empty());
        assert_eq!(previous.input.read(cx).value(), "第一处的补充要求");
        let path = previous.saved.path().unwrap();
        editor.restore_ai_history(path, window, cx);
        assert_eq!(editor.ai_session.as_ref().unwrap().saved.id, first_task);
    });
}

#[gpui::test]
fn ai_late_events_and_snapshot_reads_belong_to_the_original_task_even_with_equal_generations(
    cx: &mut TestAppContext,
) {
    let (editor, cx, directory, first, second) = tab_review_fixture(cx);
    let (first_task, workspace, control) = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.saved.running = true;
        session.control = RunControl::default();
        session.generation = 1;
        (
            session.saved.id.clone(),
            session.saved.workspace.clone(),
            session.control.clone(),
        )
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(second.clone(), window, cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.saved.running = true;
        session.generation = 1;
        let second_task = session.saved.id.clone();
        assert!(
            !control.cancel.is_cancelled(),
            "新任务不能取消先前的后台任务"
        );
        let (response, receive) = std::sync::mpsc::sync_channel(1);
        let done = editor.receive_ai_session_events(
            &first_task,
            1,
            vec![
                super::WorkerEvent::Refresh {
                    id: "first.md".into(),
                    response,
                },
                super::WorkerEvent::Runtime(super::RunEvent::AssistantStart),
                super::WorkerEvent::Runtime(super::RunEvent::Text("A 的回答".into())),
                super::WorkerEvent::Runtime(super::RunEvent::Checkpoint {
                    workspace: Box::new(workspace),
                    history: Vec::new(),
                }),
                super::WorkerEvent::Finished(Ok(super::RunOutput::default())),
            ],
            cx,
        );
        assert!(done);
        assert_eq!(
            &*receive.try_recv().unwrap().unwrap().unwrap().source,
            std::fs::read_to_string(&first).unwrap()
        );
        let session = editor.ai_session.as_ref().unwrap();
        assert_eq!(session.saved.id, second_task);
        assert!(session.saved.running, "A 完成不能把 B 标记为已完成");
        assert!(session.saved.messages.is_empty(), "A 的回复不能出现在 B 中");
        assert_eq!(
            session.saved.target.as_ref().unwrap().path.as_ref(),
            Some(&second)
        );
        let previous = editor
            .ai_task_sessions
            .iter()
            .find(|session| session.saved.id == first_task)
            .unwrap();
        assert!(!previous.saved.running);
        assert_eq!(&*previous.saved.messages.last().unwrap().text, "A 的回答");
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_quick_result_has_no_follow_up_composer_in_either_surface(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "选中的原文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.select_document(cx);
        editor.open_ai_assistant(AiCommand::Polish, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "已完成润色".into(),
            });
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-dialog-result").is_some());
    assert!(
        cx.debug_bounds("ai-composer").is_none(),
        "快捷指令结果不能把用户输入接在旧选区与权限后面"
    );
    let dock = cx.debug_bounds("ai-expand").unwrap().center();
    cx.simulate_click(dock, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-sidebar").is_some());
    assert!(
        cx.debug_bounds("ai-composer").is_none(),
        "停靠不把快捷指令变成用户对话"
    );
}

#[gpui::test]
fn ai_user_conversation_accepts_cross_document_edits_and_keeps_its_history_when_docked(
    cx: &mut TestAppContext,
) {
    let (editor, cx, directory, _first, second) = tab_review_fixture(cx);
    let task = editor.update_in(cx, |editor, window, cx| {
        editor.jump_to_document_search_range(0.."旧第一段。".len(), cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.expanded = true;
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "user".into(),
                text: "一起整理两篇文档".into(),
            });
        session
            .saved
            .history
            .push(serde_json::json!({"role":"user", "content":"固定的上一轮"}));
        let id = session.saved.id.clone();
        assert!(session.saved.workspace.selection_scope.is_none());
        assert!(!session.saved.workspace.read_only);
        session
            .saved
            .workspace
            .call(
                "propose_edit",
                &serde_json::json!({
                    "path":"first.md", "before":"旧第二段。", "after":"选区外的建议"
                }),
            )
            .unwrap();
        session
            .saved
            .workspace
            .call(
                "propose_edit",
                &serde_json::json!({
                    "path":"second.md", "before":"另外一篇", "after":"第二篇建议"
                }),
            )
            .unwrap();
        editor.open_workspace_file(second, window, cx);
        id
    });
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-sidebar").is_some(),
        "明确停靠的用户对话应可跨标签继续"
    );
    assert!(cx.debug_bounds("ai-composer").is_some());
    editor.read_with(cx, |editor, _| {
        let session = editor.ai_session.as_ref().unwrap();
        assert_eq!(session.saved.id, task);
        assert_eq!(session.saved.history[0]["content"], "固定的上一轮");
        assert_eq!(
            &*session.saved.workspace.changes.proposals["second.md"].proposed,
            "第二篇建议"
        );
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_cmd_j_always_starts_a_new_conversation_even_at_the_same_position(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx);
        let previous = editor.ai_session.as_ref().unwrap().saved.id.clone();
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx);
        assert_ne!(
            editor.ai_session.as_ref().unwrap().saved.id,
            previous,
            "重新唤起默认重置；继续对话应在原输入框中进行"
        );
        assert!(editor.ai_session.as_ref().unwrap().visible);
    });
}

#[gpui::test]
fn ai_opt_in_disabling_cancels_active_and_retained_tasks_and_preserves_pending_history(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文".into(), None));
    let (previous, current, path) = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.control = RunControl::default();
        session.saved.running = true;
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let base = session.saved.workspace.documents[&id].clone();
        session.saved.workspace.changes.propose(base, "原文", "建议").unwrap();
        let previous = session.control.clone();
        let path = session.saved.path().unwrap();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.control = RunControl::default();
        session.saved.running = true;
        (previous, session.control.clone(), path)
    });
    cx.update(|_, cx| crate::ai::settings::AiFeature::set_enabled(cx, false));
    draw(cx);
    draw(cx);
    assert!(previous.cancel.is_cancelled() && current.cancel.is_cancelled());
    assert!(cx.debug_bounds("ai-assistant-panel").is_none());
    editor.read_with(cx, |editor, _| {
        assert!(editor.ai_session.is_none() && editor.ai_task_sessions.is_empty());
        assert_eq!(editor.buffer.text(), "原文");
    });
    // 销毁路径的落盘是后台线程，先等它写完再读文件。
    crate::ai::session::flush_pending_saves();
    let saved = crate::ai::session::SavedSession::load_at(&path, None).unwrap();
    assert_eq!(saved.workspace.changes.proposals.len(), 1);
    assert!(saved.interrupted);
}

#[gpui::test]
fn ai_settings_entry_opens_preferences_and_preserves_the_running_conversation(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    let (id, control) = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.control = RunControl::default();
        session.saved.running = true;
        session.generation = 20;
        session.input.update(cx, |field, cx| field.set_value("未发出的要求", window, cx));
        (session.saved.id.clone(), session.control.clone())
    });
    draw(cx); draw(cx);
    let more = cx.debug_bounds("ai-options").unwrap();
    cx.simulate_click(more.center(), gpui::Modifiers::none());
    draw(cx); draw(cx);
    let settings = menu_command_position(cx, |strings| &strings.ai_panel_settings);
    cx.simulate_click(settings, gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(cx.read(|cx| cx.windows().iter().any(|window| crate::ui::window_content::<crate::config::preferences::PreferencesWindow>(window, cx).is_some())),
        "AI 设置必须使用偏好设置，不在对话里编辑连接");
    assert!(!control.cancel.is_cancelled());
    editor.read_with(cx, |editor, cx| {
        let session = editor.ai_session.as_ref().unwrap();
        assert_eq!(session.saved.id, id);
        assert_eq!(session.generation, 20);
        assert_eq!(session.input.read(cx).value(), "未发出的要求");
        assert_eq!(editor.buffer.text(), "正文");
    });
}

#[gpui::test]
fn ai_settings_reuses_the_preferences_window(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update(cx, |editor, cx| {
        editor.open_ai_settings(cx);
        editor.open_ai_settings(cx);
    });
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| cx.windows().iter().filter(|window| crate::ui::window_content::<crate::config::preferences::PreferencesWindow>(*window, cx).is_some()).count()),
        1, "配置入口应复用偏好设置，避免多份未保存的连接草稿");
}

#[gpui::test]
fn ai_settings_saved_elsewhere_refreshes_configuration_without_resetting_the_conversation(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    let id = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.input.update(cx, |field, cx| field.set_value("尚未发出", window, cx));
        session.saved.id.clone()
    });
    let settings = crate::ai::settings::AiSettings {
        enabled:true,
        endpoints:vec![crate::ai::settings::Endpoint {id:"local".into(), name:"Local".into(),
            model:"new-model".into(), base_url:"http://localhost:11434/v1".into(), ..Default::default()}],
        ..Default::default()
    };
    settings.save_at(&crate::ai::settings::settings_path().unwrap()).unwrap();
    cx.update(|_, cx| crate::ai::settings::AiFeature::set_enabled(cx, true));
    draw(cx);
    editor.read_with(cx, |editor, cx| {
        let session = editor.ai_session.as_ref().unwrap();
        assert!(session.settings.active_endpoint().is_some(), "保存后应刷新已有对话的连接配置");
        assert_eq!(session.settings.active_endpoint().unwrap().model, "new-model");
        assert_eq!(session.saved.id, id);
        assert_eq!(session.input.read(cx).value(), "尚未发出");
    });
}

#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
fn ai_instruction_conversation_keeps_its_http_prefix_after_language_and_preferences_change(
    cx: &mut TestAppContext,
) {
    use std::io::Write;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let start = std::time::Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && start.elapsed() < std::time::Duration::from_secs(10) =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(2))
                    }
                    Err(error) => panic!("未收到指令请求：{error}"),
                }
            };
            requests.push(crate::ai::runtime::protocol_tests::read_request(&mut stream).1);
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                serde_json::json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"OK"},"finish_reason":null}]}),
                serde_json::json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})
            );
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        }
        requests
    });
    initialize(cx);
    let mut settings = crate::ai::settings::AiSettings {
        enabled: true,
        ..Default::default()
    };
    settings
        .set_endpoint(crate::ai::settings::Endpoint {
            id: "fixture".into(),
            name: "Fixture".into(),
            base_url: format!("http://{address}/v1"),
            model: "fixture".into(),
            ..Default::default()
        })
        .unwrap();
    settings
        .quick_instructions
        .insert("conversation".into(), "Original writing preference".into());
    let path = crate::ai::settings::settings_path().unwrap();
    settings.save_at(&path).unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .input
            .update(cx, |field, cx| field.set_value("First request", window, cx));
        editor.send_ai_task(window, cx);
    });
    for _ in 0..10 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        draw(cx);
    }
    assert!(!editor.read_with(cx, |editor, _| {
        editor.ai_session.as_ref().unwrap().saved.running
    }));
    settings
        .quick_instructions
        .insert("conversation".into(), "Changed preference".into());
    settings.save_at(&path).unwrap();
    cx.update(|_, cx| crate::i18n::I18nManager::init_with_language_id(cx, "zh-CN"));
    editor.update_in(cx, |editor, window, cx| {
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .input
            .update(cx, |field, cx| field.set_value("Second request", window, cx));
        editor.send_ai_task(window, cx);
    });
    for _ in 0..10 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        draw(cx);
    }
    let requests = server.join().unwrap();
    let first = requests[0]["messages"].as_array().unwrap();
    let second = requests[1]["messages"].as_array().unwrap();
    assert_eq!(
        first[0], second[0],
        "修改设置和界面语言不能重写已开始对话的系统前缀"
    );
    assert!(first[0].to_string().contains("Original writing preference"));
    assert!(second.starts_with(first), "继续对话只追加消息");
}

fn sidebar_menu_chain(cx: &mut TestAppContext, split: bool) {
    initialize(cx);
    cx.update(crate::app_menu::init);
    let action = crate::commands::commands()
        .iter()
        .find(|command| command.id == "ai_sidebar")
        .expect("视图菜单需要独立的 AI 侧边栏入口")
        .boxed_action();
    let directory =
        std::env::temp_dir().join(format!("velora-ai-sidebar-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("note.md");
    std::fs::write(&path, "alpha one").unwrap();
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "alpha one".into(), Some(path)));
    if split {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(
                crate::editor::workspace::split::SplitDirection::Right,
                window,
                cx,
            )
        });
        cx.run_until_parked();
    }
    let target = if split {
        editor
            .read_with(cx, |editor, _| editor.active_split_editor())
            .expect("必须实际创建分屏，不能回退成普通窗口测试")
    } else {
        editor.clone()
    };
    let block = target.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    target.update(cx, |editor, _| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _| block.selected_range = 0..5);
    let weak = editor.downgrade();
    cx.update(|window, cx| {
        window.activate_window();
        crate::app_menu::dispatch_menu_action(action.as_ref(), cx);
    });
    cx.run_until_parked();
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-sidebar").is_some(),
        "从视图打开应直接显示侧栏"
    );
    let (id, control) = editor.update_in(cx, |editor, window, cx| {
        let session = editor.ai_session.as_mut().unwrap();
        assert!(session.expanded && session.visible);
        assert_eq!(session.saved.target.as_ref().unwrap().selection, "alpha");
        session
            .input
            .update(cx, |input, cx| input.set_value("尚未发送的后续要求", window, cx));
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "已有回答".into(),
            });
        let document = session
            .saved
            .workspace
            .documents
            .values()
            .next()
            .unwrap()
            .clone();
        session
            .saved
            .workspace
            .changes
            .propose(document, "alpha", "ALPHA")
            .unwrap();
        session.saved.running = true;
        (session.saved.id.clone(), session.control.clone())
    });
    for visible in [false, true] {
        cx.update(|window, cx| {
            crate::app_menu::dispatch_menu_action_for_editor(action.as_ref(), &weak, window, cx)
        });
        cx.run_until_parked();
        draw(cx);
        draw(cx);
        assert_eq!(cx.debug_bounds("ai-sidebar").is_some(), visible);
        editor.read_with(cx, |editor, cx| {
            let session = editor.ai_session.as_ref().unwrap();
            assert_eq!(session.saved.id, id, "开合侧栏不能创建或重置任务");
            assert_eq!(session.input.read(cx).value(), "尚未发送的后续要求");
            assert_eq!(session.saved.messages.len(), 1);
            assert_eq!(session.saved.workspace.changes.proposals.len(), 1);
        });
        assert!(!control.cancel.is_cancelled(), "收起侧栏不能停止已有任务");
    }
    editor.update_in(cx, |editor, window, cx| editor.new_ai_task(window, cx));
    cx.run_until_parked();
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-sidebar").is_some(),
        "在侧栏中新建对话应保留侧栏布局"
    );
    editor.update_in(cx, |editor, window, cx| {
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx)
    });
    cx.run_until_parked();
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, _| {
        let session = editor.ai_session.as_ref().unwrap();
        assert_ne!(session.saved.id, id, "快捷键仍需发起独立对话");
        assert!(!session.expanded, "快捷键仍以浮动对话打开");
        assert!(
            editor
                .ai_task_sessions
                .iter()
                .any(|session| session.saved.id == id
                    && session.saved.workspace.changes.proposals.len() == 1)
        );
    });
    cx.update(|_, cx| crate::ai::settings::AiFeature::set_enabled(cx, false));
    draw(cx);
    cx.update(|window, cx| {
        crate::app_menu::dispatch_menu_action_for_editor(action.as_ref(), &weak, window, cx)
    });
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("ai-sidebar").is_none(),
        "AI 停用后不能通过旧动作打开侧栏"
    );
    cx.on_quit(move || std::fs::remove_dir_all(directory).unwrap());
}

#[gpui::test]
fn ai_sidebar_menu_opens_and_restores_the_same_task(cx: &mut TestAppContext) {
    sidebar_menu_chain(cx, false);
}

#[gpui::test]
fn ai_sidebar_menu_in_a_split_routes_to_the_window_host(cx: &mut TestAppContext) {
    sidebar_menu_chain(cx, true);
}

#[gpui::test]
fn ai_sidebar_keeps_the_composer_at_the_bottom_across_empty_conversation_and_error_states(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor,cx) = cx.add_window_view(|_,cx| Editor::from_markdown(cx,"正文".into(),None));
    editor.update_in(cx, |editor,window,cx| {
        editor.open_ai_assistant(AiCommand::Ask,window,cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.expanded = true;
        session.settings.set_endpoint(crate::ai::settings::Endpoint { id:"fixture".into(), model:"fixture".into(), base_url:"http://localhost:8080/v1".into(), ..Default::default() }).unwrap();
    });
    draw(cx); draw(cx);
    assert!(cx.debug_bounds("ai-command-bar").is_none(), "侧栏也不能重复列出快捷指令");
    let sidebar = cx.debug_bounds("ai-sidebar").unwrap();
    let composer = cx.debug_bounds("ai-composer").unwrap();
    assert!(composer.bottom() > sidebar.bottom()-gpui::px(24.0), "空白会话的输入也应固定在侧栏底部");
    let empty_composer = composer;
    let input = cx.debug_bounds("ai-composer-input").unwrap();
    for selector in ["ai-context", "ai-send"] {
        let control = cx.debug_bounds(selector).unwrap();
        assert!(control.top() >= input.bottom(), "引用与发送放在输入卡片底部");
        assert!(control.left() >= composer.left() && control.right() <= composer.right());
        assert!(control.bottom() <= composer.bottom());
    }
    let send = cx.debug_bounds("ai-send").unwrap();
    cx.simulate_click(send.center(),gpui::Modifiers::none());
    editor.read_with(cx, |editor,_| assert!(editor.ai_session.as_ref().unwrap().saved.messages.is_empty()));
    editor.update(cx, |editor,cx| {
        editor.ai_session.as_mut().unwrap().saved.messages.push(crate::ai::session::ChatMessage{role:"user".into(),text:"已有对话".into()});
        cx.notify();
    });
    draw(cx); draw(cx);
    let composer = cx.debug_bounds("ai-composer").unwrap();
    assert_eq!(composer,empty_composer,"第一条消息出现不能移动输入卡片");
    editor.update(cx, |editor,cx| {
        editor.ai_session.as_mut().unwrap().error = Some("连接暂时不可用".into());
        cx.notify();
    });
    draw(cx); draw(cx);
    assert_eq!(cx.debug_bounds("ai-composer"),Some(empty_composer),"错误和重试按钮不能挤占输入卡片的底部位置");
}

#[test]
fn ai_inline_review_marks_whitespace_only_changes_between_blocks() {
    use super::inline_review::{MarkKind, tracked_document};
    // 只把块间空行从一行加到两行：行级 diff 有编辑，块键 trim 掉换行后
    // 全部 Equal——投影必须给这条编辑可见的标记，不能零标记地混进正文。
    let before = "甲段\n\n乙段\n";
    let after = "甲段\n\n\n乙段\n";
    let proposal = crate::ai::Proposal::new(
        crate::ai::DocumentSnapshot {
            id: "note.md".into(),
            source: before.into(),
        },
        after,
    );
    let edits = proposal.edits();
    assert!(!edits.is_empty(), "行级差异应产出待审编辑");
    // 块边界取自两份文档的解析树：差异落在前一块的收尾换行上。
    let (text, marks) = tracked_document(
        before,
        &[before.find("乙段").unwrap()..before.len()],
        after,
        &[after.find("乙段").unwrap()..after.len()],
        &edits,
    )
    .unwrap();
    assert!(
        marks.iter().any(|mark| mark.kind == MarkKind::Added),
        "仅空行的变化也要有可见标记：{text:?} / {marks:?}"
    );
    assert!(
        marks.iter().all(|mark| !mark.edits.is_empty()),
        "每条标记都要指向待审编辑"
    );
}

#[test]
fn ai_inline_review_does_not_attribute_unrelated_hunks_to_a_nearby_edit() {
    use super::inline_review::covered_edits;
    use crate::ai::ProposedEdit;
    // hunk 覆盖 10..20，两条编辑分别在 0..4 与 30..34：无重叠时不得
    // 强行归属到邻近编辑，否则重复段落下行内按钮会作用于错位的修改。
    let edits = vec![
        ProposedEdit {
            range: 0..4,
            replacement: "一".into(),
        },
        ProposedEdit {
            range: 30..34,
            replacement: "二".into(),
        },
    ];
    let attributed = covered_edits(&edits, 10..20);
    assert!(
        attributed.is_empty(),
        "无重叠的块差异不应归属到任何编辑：{attributed:?}"
    );
}

#[test]
fn ai_retry_rolls_back_the_failed_conversation_round_before_sending_again() {
    use crate::ai::session::{ChatMessage, SavedSession, SessionKind};
    let mut saved = SavedSession::new(
        crate::ai::workspace::AiWorkspace::new(None, Vec::new()).unwrap(),
    );
    saved.kind = SessionKind::Conversation;
    saved.messages = vec![
        ChatMessage {
            role: "user".into(),
            text: "旧问题".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            text: "旧回答".into(),
        },
        ChatMessage {
            role: "user".into(),
            text: "失败轮的指令".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            text: String::new().into(),
        },
    ];
    saved.rollback_failed_conversation_round();
    assert_eq!(
        saved
            .messages
            .iter()
            .map(|message| message.text.to_string())
            .collect::<Vec<_>>(),
        vec!["旧问题".to_string(), "旧回答".to_string()],
        "重试前应回滚失败轮的用户消息与空回复，发送后再追加一次"
    );
}

#[gpui::test]
fn ai_accepting_a_proposal_for_a_non_active_external_pane_updates_that_pane(cx: &mut TestAppContext) {
    initialize(cx);
    let workspace = std::env::temp_dir().join(format!("velora-ai-accept-ws-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("note.md"), "工作区原文").unwrap();
    let external = std::env::temp_dir().join(format!("velora-ai-accept-ext-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&external).unwrap();
    let outside = external.join("outside.md");
    std::fs::write(&outside, "外部原文").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "工作区原文".into(), Some(workspace.join("note.md")))
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.split_active_document(
            crate::editor::workspace::split::SplitDirection::Right,
            window,
            cx,
        )
    });
    let panes = editor.read_with(cx, |editor, _| editor.split_groups());
    assert_eq!(panes.len(), 2);
    // 非活动面板换成工作区外的外部文档；活动面板仍是工作区文档。
    panes[0].update_in(cx, |pane, window, cx| {
        pane.open_workspace_file(outside.clone(), window, cx)
    });
    cx.run_until_parked();
    let external_id = panes[0].read_with(cx, |pane, _| {
        super::ai_document_id(
            pane.file_path.as_deref(),
            None,
            pane.recovery_id,
        )
    });
    assert!(external_id.starts_with("external:"), "{external_id}");
    let baseline = panes[0].read_with(cx, |pane, _| pane.buffer.text());
    let active = editor.read_with(cx, |editor, _| editor.active_split_editor().unwrap());
    assert_ne!(active.entity_id(), panes[0].entity_id(), "活动面板应是工作区文档");
    active.update_in(cx, |leaf, window, cx| {
        leaf.open_ai_assistant(AiCommand::Ask, window, cx);
    });
    // 分屏把会话交给宿主持有；接受也从宿主派发到活动组。
    editor.update(cx, |editor, cx| {
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        session.saved.workspace.live_documents.insert(external_id.clone());
        session.saved.workspace.changes.proposals.insert(
            external_id.clone(),
            crate::ai::Proposal::new(
                crate::ai::DocumentSnapshot {
                    id: external_id.clone(),
                    source: baseline.into(),
                },
                "外部修订",
            ),
        );
        editor.refresh_ai_preview(cx);
    });
    let result = editor.update_in(cx, |editor, window, cx| {
        editor.accept_ai_proposals(None, None, window, cx)
    });
    assert!(result.is_ok(), "{result:?}");
    cx.run_until_parked();
    assert_eq!(
        panes[0].read_with(cx, |pane, _| pane.buffer.text()),
        "外部修订",
        "对非活动外部文档的提议应写进持有它的面板，而不是整批失败"
    );
    assert_eq!(panes[1].read_with(cx, |pane, _| pane.buffer.text()), "工作区原文");
    std::fs::remove_dir_all(workspace).unwrap();
    std::fs::remove_dir_all(external).unwrap();
}

#[gpui::test]
fn ai_restoring_history_does_not_overwrite_a_stashed_session(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "第一段\n\n第二段".into(), None));
    // 前台任务 B：待审修改与草稿都要在暂存后原样保留。
    let history_id = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Task;
        let task_id = session.saved.id.clone();
        let id = session.saved.target.as_ref().unwrap().document_id.clone();
        let baseline = session.saved.workspace.documents[&id].clone();
        session.saved.workspace.changes.proposals.insert(
            id.clone(),
            crate::ai::Proposal::new(baseline, "B 的待审修改"),
        );
        session.saved.messages.push(crate::ai::session::ChatMessage {
            role: "user".into(),
            text: "B 的指令".into(),
        });
        editor.persist_ai_session(cx);
        task_id
    });
    // 另写一份不在内存里的历史会话 X，触发磁盘加载路径。
    let mut foreign = crate::ai::session::SavedSession::new(
        crate::ai::workspace::AiWorkspace::new(None, Vec::new()).unwrap(),
    );
    foreign.messages.push(crate::ai::session::ChatMessage {
        role: "user".into(),
        text: "X 的消息".into(),
    });
    let foreign_path = foreign.path().unwrap();
    foreign.save_at(&foreign_path).unwrap();
    editor.update_in(cx, |editor, window, cx| {
        editor.restore_ai_history(foreign_path.clone(), window, cx);
        // 加载在途时用户唤起新任务，原任务被暂存。
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        let stashed = editor
            .ai_task_sessions
            .iter()
            .find(|session| session.saved.id == history_id)
            .unwrap_or_else(|| panic!("暂存任务必须保留：{:?}", editor.ai_task_sessions.len()));
        assert_eq!(
            stashed.saved.messages.last().unwrap().text.to_string(),
            "B 的指令",
            "迟到的历史加载不得整体替换暂存会话"
        );
        assert!(stashed.saved.workspace.changes.proposals.len() == 1);
    });
    std::fs::remove_file(foreign_path).unwrap();
}

#[gpui::test]
fn ai_panel_actions_are_buttons_and_can_be_closed_from_the_keyboard(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui::AppContext as _;
    initialize(cx);
    let handle = cx.update(|cx| crate::ui::open_window(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                Default::default(), gpui::size(gpui::px(1024.0), gpui::px(768.0)),
            ))),
            ..Default::default()
        }, cx, |_, cx| cx.new(|cx| Editor::from_markdown(cx, "保留正文。".into(), None)),
    ).unwrap());
    handle.update(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_ref().unwrap();
        window.focus(&session.panel_focus, cx);
    }).unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    draw(&mut visual);
    draw(&mut visual);
    visual.update(|window, cx| {
        window.render_frame(cx);
        assert_eq!(window.try_find("ai-hide").and_then(|button| button.role()), Some(gpui::Role::Button), "AI 操作应使用可聚焦的标准按钮");
        for _ in 0..32 {
            window.press("tab", cx);
            window.render_frame(cx);
            if window.find("ai-hide").focused() == Some(true) { break; }
        }
        assert_eq!(window.find("ai-hide").focused(), Some(true), "关闭按钮应能从键盘到达");
        window.press("space", cx);
    });
    visual.run_until_parked();
    handle.read_with(cx, |editor, _| {
        assert!(!editor.ai_session.as_ref().unwrap().visible);
        assert_eq!(editor.buffer.text(), "保留正文。");
    }).unwrap();
}

#[gpui::test]
fn ai_composer_uses_native_multiline_input_and_does_not_submit_ime_composition(
    cx: &mut TestAppContext,
) {
    use gpui::{AppContext as _, EntityInputHandler as _};
    use gpui_kit::test::TestWindowExt as _;
    initialize(cx);
    let mut settings = crate::ai::settings::AiSettings {
        enabled: true,
        ..Default::default()
    };
    settings
        .set_endpoint(
            crate::ai::providers::ProviderPreset::get("deepseek")
                .unwrap()
                .endpoint(),
        )
        .unwrap();
    settings
        .save_at(&crate::ai::settings::settings_path().unwrap())
        .unwrap();
    let handle = cx.update(|cx| {
        crate::ui::open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                    Default::default(),
                    gpui::size(gpui::px(1024.0), gpui::px(768.0)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| Editor::from_markdown(cx, "保留正文。".into(), None)),
        )
        .unwrap()
    });
    let input = handle
        .update(cx, |editor, window, cx| {
            editor.open_ai_assistant(AiCommand::Ask, window, cx);
            editor.ai_session.as_ref().unwrap().input.clone()
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    draw(&mut visual);
    draw(&mut visual);
    visual.update(|window, cx| {
        assert_eq!(
            window
                .try_find(("input", input.entity_id()))
                .and_then(|field| field.role()),
            Some(gpui::Role::MultilineTextInput),
            "对话输入应使用标准多行控件"
        );
        input.update(cx, |input, cx| input.focus(window, cx));
        window.input("第一行", cx);
        window.press("shift-enter", cx);
        window.input("第二行", cx);
    });
    visual.run_until_parked();
    assert_eq!(
        handle
            .read_with(cx, |editor, cx| editor
                .ai_session
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .value()
                .to_string())
            .unwrap(),
        "第一行\n第二行"
    );
    visual.update(|window, cx| {
        window.press("secondary-a", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("第一行\n第二行".into())
        );
        window.press("secondary-x", cx);
    });
    visual.run_until_parked();
    assert!(
        handle
            .read_with(cx, |editor, cx| editor
                .ai_session
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .value()
                .is_empty())
            .unwrap()
    );
    visual.update(|window, cx| window.press("secondary-z", cx));
    visual.run_until_parked();
    assert_eq!(
        handle
            .read_with(cx, |editor, cx| editor
                .ai_session
                .as_ref()
                .unwrap()
                .input
                .read(cx)
                .value()
                .to_string())
            .unwrap(),
        "第一行\n第二行"
    );
    visual.update(|window, cx| {
        window.press("secondary-a", cx);
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "拼", Some(0..1), window, cx)
        });
        window.press("enter", cx);
    });
    visual.run_until_parked();
    handle
        .read_with(cx, |editor, _| {
            let session = editor.ai_session.as_ref().unwrap();
            assert!(session.saved.messages.is_empty());
            assert!(session.error.is_none(), "组词回车不能触发发送或连接校验");
        })
        .unwrap();
    visual.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "拼音", window, cx)
        });
        window.press("enter", cx);
    });
    visual.run_until_parked();
    handle
        .read_with(cx, |editor, cx| {
            let session = editor.ai_session.as_ref().unwrap();
            assert!(
                session.error.is_some(),
                "普通回车应触发缺少密钥的校验：task={}, draft={:?}, messages={}",
                session.mode == PanelMode::Task,
                session.input.read(cx).value(),
                session.saved.messages.len()
            );
            assert_eq!(
                session.input.read(cx).value(),
                "拼音",
                "发送校验失败应保留草稿且不插入换行"
            );
            assert_eq!(editor.buffer.text(), "保留正文。");
        })
        .unwrap();
}

#[gpui::test]
fn ai_auxiliary_fields_have_native_roles_and_keep_edits_out_of_the_document(
    cx: &mut TestAppContext,
) {
    use gpui::AppContext as _;
    use gpui_kit::test::TestWindowExt as _;
    initialize(cx);
    let handle = cx.update(|cx| {
        crate::ui::open_window(Default::default(), cx, |_, cx| {
            cx.new(|cx| Editor::from_markdown(cx, "保留正文。".into(), None))
        })
        .unwrap()
    });
    let (name, body, context, language) = handle
        .update(cx, |editor, window, cx| {
            editor.open_ai_assistant(AiCommand::Ask, window, cx);
            let session = editor.ai_session.as_mut().unwrap();
            session.mode = PanelMode::Instruction;
            (
                session.instruction_name.clone(),
                session.instruction_body.clone(),
                session.context_input.clone(),
                session.language.clone(),
            )
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    draw(&mut visual);
    draw(&mut visual);
    visual.update(|window, cx| {
        assert_eq!(
            window
                .try_find(("input", name.entity_id()))
                .and_then(|field| field.role()),
            Some(gpui::Role::TextInput),
            "指令名称应使用标准单行输入"
        );
        assert_eq!(
            window
                .try_find(("input", body.entity_id()))
                .and_then(|field| field.role()),
            Some(gpui::Role::MultilineTextInput),
            "指令内容应使用标准多行输入"
        );
        body.update(cx, |field, cx| field.focus(window, cx));
        window.input("保留原意", cx);
        window.press("enter", cx);
        window.input("修正语法", cx);
        window.press("secondary-a", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("保留原意\n修正语法".into())
        );
        window.press("secondary-x", cx);
        window.press("secondary-z", cx);
    });
    visual.run_until_parked();
    handle
        .update(cx, |editor, _, cx| {
            assert_eq!(body.read(cx).value(), "保留原意\n修正语法");
            assert_eq!(editor.buffer.text(), "保留正文。");
            assert!(
                editor
                    .ai_session
                    .as_ref()
                    .unwrap()
                    .saved
                    .messages
                    .is_empty(),
                "指令内容中的回车不能发送 AI 请求"
            );
            editor.ai_session.as_mut().unwrap().mode = PanelMode::Context;
            cx.notify();
        })
        .unwrap();
    draw(&mut visual);
    visual.update(|window, cx| {
        assert_eq!(
            window
                .try_find(("input", context.entity_id()))
                .and_then(|field| field.role()),
            Some(gpui::Role::TextInput)
        );
        context.update(cx, |field, cx| field.focus(window, cx));
        window.input("@\"folder name/note.md\"", cx);
    });
    visual.run_until_parked();
    handle
        .update(cx, |editor, _, cx| {
            let session = editor.ai_session.as_mut().unwrap();
            assert_eq!(
                session.context_input.read(cx).value(),
                "@\"folder name/note.md\""
            );
        })
        .unwrap();
    draw(&mut visual);
    visual.update(|window, cx| window.click("ai-context-add", cx));
    visual.run_until_parked();
    handle
        .update(cx, |editor, _, cx| {
            let session = editor.ai_session.as_mut().unwrap();
            assert_eq!(session.saved.references, ["folder name/note.md"]);
            assert!(session.context_input.read(cx).value().is_empty());
            session.mode = PanelMode::Task;
            session.command = AiCommand::Translate;
            cx.notify();
        })
        .unwrap();
    draw(&mut visual);
    visual.update(|window, cx| {
        assert_eq!(
            window
                .try_find(("input", language.entity_id()))
                .and_then(|field| field.role()),
            Some(gpui::Role::TextInput)
        );
        language.update(cx, |field, cx| field.focus(window, cx));
        window.press("secondary-a", cx);
        window.input("日本語", cx);
    });
    visual.run_until_parked();
    handle
        .read_with(cx, |editor, cx| {
            assert_eq!(language.read(cx).value(), "日本語");
            assert_eq!(editor.buffer.text(), "保留正文。");
        })
        .unwrap();
}

#[gpui::test]
fn ai_context_sources_are_checkable_and_share_one_selection_list(cx: &mut TestAppContext) {
    use gpui::AppContext as _;
    use gpui_kit::test::TestWindowExt as _;
    initialize(cx);
    let handle = cx.update(|cx| {
        crate::ui::open_window(Default::default(), cx, |_, cx| {
            cx.new(|cx| Editor::from_markdown(cx, "保留正文。".into(), None))
        })
        .unwrap()
    });
    let editor = handle.update(cx, |_, _, cx| cx.entity()).unwrap();
    let cx = &mut VisualTestContext::from_window(handle.into(), cx);
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.mode = PanelMode::Context;
        session.saved.references = vec!["a.md".into()];
        session.context_candidates = vec!["a.md".into(), "b.md".into()];
        window.focus(&session.panel_focus, cx);
        cx.notify();
    });
    draw(cx);
    draw(cx);
    cx.update(|window, cx| {
        assert_eq!(
            window
                .try_find("ai-context-source:a.md")
                .and_then(|row| row.role()),
            Some(gpui::Role::CheckBox),
            "资料应显示为可勾选的同一份列表"
        );
        assert!(
            window.try_find(("ai-context-remove", 0usize)).is_none(),
            "所选资料不再重复显示为另一排按钮"
        );
        window.click("ai-context-source:a.md", cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .saved
                .references
                .is_empty()
        )
    });
    draw(cx);
    cx.update(|window, cx| window.click("ai-context-source:b.md", cx));
    draw(cx);
    cx.update(|window, cx| {
        // Kit 的 Checkbox 保留鼠标操作前的焦点，键盘路径通过 Tab 进入。
        for _ in 0..16 {
            window.press("tab", cx);
            if window.find("ai-context-source:b.md").focused() == Some(true) {
                break;
            }
        }
        assert_eq!(
            window.find("ai-context-source:b.md").focused(),
            Some(true),
            "资料选择应能通过 Tab 到达"
        );
        window.press("space", cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .saved
                .references
                .is_empty(),
            "再次按空格应取消选中"
        );
        assert_eq!(editor.buffer.text(), "保留正文。");
    });
}

#[gpui::test]
fn ai_streaming_and_completed_messages_use_the_same_native_transcript(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    initialize(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "保留正文。".into(), None));
    let session_id = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.expanded = true;
        session.saved.running = true;
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "**第一段**".into(),
            });
        let id = session.saved.id.clone();
        editor.refresh_ai_preview(cx);
        cx.notify();
        id
    });
    draw(cx);
    draw(cx);
    cx.update(|window, _| {
        assert_eq!(
            window
                .try_find("ai-message-stream")
                .and_then(|list| list.role()),
            Some(gpui::Role::Group),
            "生成中也应使用同一套消息列表"
        );
        assert_eq!(
            window
                .try_find(format!("ai-message:{session_id}:0"))
                .and_then(|row| row.role()),
            Some(gpui::Role::ListItem)
        );
    });
    let original = editor.read_with(cx, |editor, _| {
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .conversation
            .clone()
            .unwrap()
    });
    editor.update(cx, |editor, cx| {
        let session = editor.ai_session.as_mut().unwrap();
        session.saved.messages[0].text = "**第一段**\n\n第二段。".into();
        session.saved.running = false;
        editor.refresh_ai_preview(cx);
        cx.notify();
    });
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .conversation
                .as_ref()
                .unwrap()
                .entity_id(),
            original.entity_id(),
            "生成结束不能重建阅读视图或丢失滚动位置"
        );
        assert_eq!(editor.buffer.text(), "保留正文。");
    });
    cx.update(|window, _| {
        assert_eq!(
            window.find("ai-message-stream").role(),
            Some(gpui::Role::Group)
        )
    });
}

#[gpui::test]
fn ai_message_updates_do_not_pull_the_reader_away_from_earlier_messages(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    initialize(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "保留正文。".into(), None));
    let session_id = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.expanded = true;
        session.saved.running = true;
        session.saved.messages = (0..40)
            .map(|index| crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: format!("## 第 {index} 条\n\n{}", "需要阅读的内容。".repeat(20)).into(),
            })
            .collect();
        let id = session.saved.id.clone();
        editor.refresh_ai_preview(cx);
        cx.notify();
        id
    });
    draw(cx);
    draw(cx);
    let prepared = editor.read_with(cx, |editor, cx| {
        let view = editor
            .ai_session
            .as_ref()
            .unwrap()
            .conversation
            .as_ref()
            .unwrap()
            .read(cx);
        (0..40)
            .filter(|index| view.message_state(*index).is_some())
            .count()
    });
    assert!(
        prepared < 20,
        "不能为未显示的整段历史预建富文本状态：{prepared}"
    );
    cx.update(|window, _| {
        assert_eq!(
            window
                .try_find("ai-message-stream")
                .and_then(|list| list.role()),
            Some(gpui::Role::Group)
        );
        assert!(
            window
                .try_find(format!("ai-message:{session_id}:39"))
                .is_some(),
            "首次显示应跟随最新消息"
        );
    });
    editor.update(cx, |editor, cx| {
        editor
            .ai_session
            .as_mut()
            .unwrap()
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "新的回答。".into(),
            });
        editor.refresh_ai_preview(cx);
        cx.notify();
    });
    draw(cx);
    draw(cx);
    cx.update(|window, _| {
        assert!(
            window
                .try_find(format!("ai-message:{session_id}:40"))
                .is_some(),
            "处于最新位置时追加消息应自动跟随"
        )
    });
    let first = format!("ai-message:{session_id}:0");
    // 连续滚动会让惰性列表逐步量出先前尚未显示的消息。
    for _ in 0..40 {
        cx.update(|window, cx| {
            window.scroll(
                "ai-message-stream",
                gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.0), gpui::px(400.0))),
                cx,
            )
        });
        draw(cx);
        if cx.update(|window, _| window.try_find(first.clone()).is_some()) {
            break;
        }
    }
    cx.update(|window, _| {
        assert!(
            window.try_find(first.clone()).is_some(),
            "向上滚动后应能看到早期消息"
        )
    });
    editor.update(cx, |editor, cx| {
        let session = editor.ai_session.as_mut().unwrap();
        session.saved.messages.last_mut().unwrap().text = "追加的最新内容。\n\n".repeat(100).into();
        editor.refresh_ai_preview(cx);
        cx.notify();
    });
    draw(cx);
    draw(cx);
    cx.update(|window, _| {
        assert!(
            window.try_find(first.clone()).is_some(),
            "最新消息变化不能抢走早期消息的阅读位置"
        )
    });
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "保留正文。")
    });
}

#[gpui::test]
fn ai_native_message_selection_copies_rendered_text_and_never_edits_the_document(
    cx: &mut TestAppContext,
) {
    use gpui::AppContext as _;
    use gpui_kit::test::TestWindowExt as _;
    initialize(cx);
    let handle = cx.update(|cx| {
        crate::ui::open_window(Default::default(), cx, |_, cx| {
            cx.new(|cx| Editor::from_markdown(cx, "正文不可修改。".into(), None))
        })
        .unwrap()
    });
    let editor = handle.update(cx, |_, _, cx| cx.entity()).unwrap();
    let cx = &mut VisualTestContext::from_window(handle.into(), cx);
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor
            .ai_session
            .as_mut()
            .unwrap()
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "**结果** 与 `代码`。".into(),
            });
        editor.refresh_ai_preview(cx);
        cx.notify();
    });
    draw(cx);
    draw(cx);
    let content = editor.read_with(cx, |editor, cx| {
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .conversation
            .as_ref()
            .unwrap()
            .read(cx)
            .message_state(0)
            .unwrap()
    });
    cx.update(|window, cx| {
        let focus = content.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        window.press("secondary-a", cx);
        assert_eq!(
            content.read(cx).selected_text().trim(),
            "结果 与 代码。",
            "选区应属于结果视图"
        );
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("结果 与 代码。".into())
        );
        window.input("不应输入", cx);
    });
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "正文不可修改。")
    });
    content.read_with(cx, |content, _| {
        assert_eq!(content.rendered_text().as_str().trim(), "结果 与 代码。")
    });
}

#[gpui::test]
fn ai_native_messages_keep_inline_math_display_math_and_mermaid_preview(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor
            .ai_session
            .as_mut()
            .unwrap()
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "行内 $E=mc^2$。\n\n$$\nx^2\n$$\n\n```mermaid\nflowchart LR\n A-->B\n```"
                    .into(),
            });
        editor.refresh_ai_preview(cx);
        cx.notify();
    });
    for _ in 0..8 {
        draw(cx);
    }
    assert!(
        cx.debug_bounds("ai-math-inline").is_some(),
        "行内公式应复用现有渲染器"
    );
    assert!(
        cx.debug_bounds("ai-math-display").is_some(),
        "块公式应继续渲染"
    );
    let diagram = cx
        .debug_bounds("ai-mermaid-diagram")
        .expect("图表应在资源完成后出现")
        .center();
    cx.simulate_click(diagram, gpui::Modifiers::none());
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.mermaid_preview.is_some(),
            "阅读消息中的图表仍能打开预览"
        );
        assert_eq!(editor.buffer.text(), "正文");
    });
}

#[gpui::test]
fn ai_native_messages_keep_parenthesized_math_in_readonly_paragraphs(cx: &mut TestAppContext) {
    use gpui::AppContext as _;
    use gpui_kit::test::TestWindowExt as _;
    initialize(cx);
    let handle = cx.update(|cx| {
        crate::ui::open_window(Default::default(), cx, |_, cx| {
            cx.new(|cx| Editor::from_markdown(cx, "正文不可修改。".into(), None))
        })
        .unwrap()
    });
    let editor = handle.update(cx, |_, _, cx| cx.entity()).unwrap();
    let cx = &mut VisualTestContext::from_window(handle.into(), cx);
    let source = "保留 \\(x^2\\) 与正文。";
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor
            .ai_session
            .as_mut()
            .unwrap()
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: source.into(),
            });
        editor.refresh_ai_preview(cx);
        cx.notify();
    });
    for _ in 0..6 {
        draw(cx);
    }
    let paragraph = cx
        .debug_bounds("ai-parenthesized-math")
        .expect("混排的括号公式应继续按原语法渲染")
        .center();
    cx.simulate_click(paragraph, gpui::Modifiers::none());
    draw(cx);
    cx.update(|window, cx| {
        window.press("secondary-a", cx);
        window.input("误改", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(source.into()),
            "阅读结果仍应可复制而不能输入修改"
        );
    });
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "正文不可修改。")
    });
}

#[gpui::test]
fn ai_message_repaint_reuses_code_highlights_and_code_copy_keeps_the_source(
    cx: &mut TestAppContext,
) {
    use crate::components::markdown::code_highlight::CODE_HIGHLIGHT_CALLS;
    use gpui_kit::test::TestWindowExt as _;
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    let code = "fn count(text: &str) -> usize { text.len() }";
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor
            .ai_session
            .as_mut()
            .unwrap()
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: format!("```rust\n{code}\n```").into(),
            });
        editor.refresh_ai_preview(cx);
        cx.notify();
    });
    for _ in 0..4 {
        draw(cx);
    }
    let before = CODE_HIGHLIGHT_CALLS.with(std::cell::Cell::get);
    for _ in 0..6 {
        draw(cx);
    }
    let after = CODE_HIGHLIGHT_CALLS.with(std::cell::Cell::get);
    assert_eq!(after, before, "无内容或主题变化的绘制不能重复执行代码高亮");
    cx.update(|window, cx| {
        window.click("ai-code-copy", cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some(code.into())
        );
    });
    editor.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), "正文"));
}

#[gpui::test]
fn ai_more_menu_keyboard_opens_history_and_returns_to_the_same_draft(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    let identity = editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_ref().unwrap();
        session
            .input
            .update(cx, |input, cx| input.set_value("草稿", window, cx));
        session.saved.id.clone()
    });
    draw(cx);
    draw(cx);
    let trigger = cx.debug_bounds("ai-options").unwrap().center();
    cx.simulate_click(trigger, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    cx.simulate_keystrokes("down down enter");
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(editor.ai_session.as_ref().unwrap().mode == PanelMode::History);
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    draw(cx);
    editor.read_with(cx, |editor, cx| {
        let session = editor.ai_session.as_ref().unwrap();
        assert!(session.mode == PanelMode::Task);
        assert!(session.visible);
        assert_eq!(session.saved.id, identity);
        assert_eq!(session.input.read(cx).value(), "草稿");
    });
}

#[gpui::test]
fn ai_sidebar_resize_preserves_draft_and_width_when_floated_and_docked(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    let identity = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.expanded = true;
        session
            .input
            .update(cx, |input, cx| input.set_value("未发送的草稿", window, cx));
        session.saved.id.clone()
    });
    draw(cx);
    cx.simulate_resize(gpui::size(gpui::px(1200.0), gpui::px(800.0)));
    for _ in 0..4 {
        draw(cx);
    }
    let original = cx.debug_bounds("ai-sidebar").unwrap();
    let start = gpui::point(original.left() + gpui::px(1.0), original.center().y);
    let end = start - gpui::point(gpui::px(100.0), gpui::px(0.0));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    draw(cx);
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::none());
    for _ in 0..4 {
        draw(cx);
    }
    let resized = cx.debug_bounds("ai-sidebar").unwrap();
    assert!(
        resized.size.width >= original.size.width + gpui::px(80.0),
        "拖动侧栏边界应改变宽度：{original:?} -> {resized:?}"
    );
    for _ in 0..2 {
        let dock = cx.debug_bounds("ai-expand").unwrap().center();
        cx.simulate_click(dock, gpui::Modifiers::none());
        for _ in 0..4 {
            draw(cx);
        }
    }
    assert_eq!(
        cx.debug_bounds("ai-sidebar").unwrap().size.width,
        resized.size.width
    );
    editor.read_with(cx, |editor, cx| {
        let session = editor.ai_session.as_ref().unwrap();
        assert_eq!(session.saved.id, identity);
        assert_eq!(session.input.read(cx).value(), "未发送的草稿");
        assert_eq!(editor.buffer.text(), "正文");
    });
}

#[gpui::test]
fn ai_sidebar_separator_supports_keyboard_resize_and_clamps_at_narrow_windows(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor.ai_session.as_mut().unwrap().expanded = true;
    });
    cx.simulate_resize(gpui::size(gpui::px(1200.0), gpui::px(800.0)));
    for _ in 0..4 {
        draw(cx);
    }
    editor.update_in(cx, |editor, window, cx| {
        let focus = editor.ai_sidebar_layout.as_ref().unwrap().focus.clone();
        window.focus(&focus, cx);
    });
    draw(cx);
    let original = cx.debug_bounds("ai-sidebar").unwrap().size.width;
    cx.simulate_keystrokes("left");
    for _ in 0..3 {
        draw(cx);
    }
    let enlarged = cx.debug_bounds("ai-sidebar").unwrap().size.width;
    assert!(enlarged > original, "左箭头应向左扩大右侧面板");
    cx.simulate_keystrokes("home right");
    for _ in 0..3 {
        draw(cx);
    }
    let minimum = cx.debug_bounds("ai-sidebar").unwrap().size.width;
    cx.simulate_keystrokes("right");
    for _ in 0..3 {
        draw(cx);
    }
    assert_eq!(cx.debug_bounds("ai-sidebar").unwrap().size.width, minimum);
    cx.simulate_keystrokes("end left");
    for _ in 0..3 {
        draw(cx);
    }
    let maximum = cx.debug_bounds("ai-sidebar").unwrap().size.width;
    cx.simulate_keystrokes("left");
    for _ in 0..3 {
        draw(cx);
    }
    assert_eq!(cx.debug_bounds("ai-sidebar").unwrap().size.width, maximum);
    for (width, document_minimum) in [(800.0, 480.0), (640.0, 384.0), (1200.0, 512.0)] {
        cx.simulate_resize(gpui::size(gpui::px(width), gpui::px(800.0)));
        for _ in 0..4 {
            draw(cx);
        }
        let sidebar = cx.debug_bounds("ai-sidebar").unwrap();
        assert_eq!(sidebar.right(), gpui::px(width), "侧栏应始终靠齐窗口右侧");
        assert!(
            sidebar.left() >= gpui::px(document_minimum),
            "窄窗口应优先保留正文空间：{width}，{sidebar:?}"
        );
        assert!(sidebar.size.width >= gpui::px(240.0));
    }
    editor.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), "正文"));
}

#[gpui::test]
fn ai_split_workspace_uses_the_same_resizable_sidebar_and_menu_escape_scope(
    cx: &mut TestAppContext,
) {
    let (editor, cx, directory, _, _) = tab_review_fixture(cx);
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.split_active_document(
            crate::editor::workspace::split::SplitDirection::Right,
            window,
            cx,
        );
        assert!(editor.active_split_editor().is_some());
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.ai_session.as_mut().unwrap().expanded = true;
        cx.notify();
    });
    cx.simulate_resize(gpui::size(gpui::px(1400.0), gpui::px(900.0)));
    for _ in 0..4 {
        draw(cx);
    }
    assert!(
        cx.debug_bounds("ai-sidebar-resize").is_some(),
        "分屏窗口也必须使用 Kit 分隔条"
    );
    let sidebar = cx.debug_bounds("ai-sidebar").unwrap();
    assert!(sidebar.size.width < gpui::px(700.0));
    let options = cx.debug_bounds("ai-options").unwrap().center();
    cx.simulate_click(options, gpui::Modifiers::none());
    draw(cx);
    draw(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    draw(cx);
    assert_eq!(
        cx.debug_bounds("ai-sidebar"),
        Some(sidebar),
        "Esc 只关闭菜单，不隐藏分屏侧栏"
    );
    cx.on_quit(move || std::fs::remove_dir_all(directory).unwrap());
}

fn navigation_returns_focus_to_ai_review(cx: &mut TestAppContext, split: bool) {
    let (editor, cx, directory, _, _) = tab_review_fixture(cx);
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        if split {
            editor.split_active_document(
                crate::editor::workspace::split::SplitDirection::Right,
                window,
                cx,
            );
        }
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
    });
    cx.run_until_parked();
    cx.simulate_resize(gpui::size(gpui::px(1400.0), gpui::px(600.0)));
    propose_tab_changes(&editor, cx);
    draw(cx);
    draw(cx);
    let review = editor.read_with(cx, |editor, _| {
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .tracked
            .clone()
    });
    let handles = review.read_with(cx, |review, cx| {
        review
            .document
            .visible_blocks()
            .iter()
            .map(|block| block.entity.read(cx).focus_handle.clone())
            .collect::<Vec<_>>()
    });
    review.update(cx, |review, cx| {
        assert!(
            review.scroll_handle.max_offset().y > gpui::px(100.0),
            "夹具需要可滚动的审阅正文"
        );
        review
            .scroll_handle
            .set_offset(gpui::point(gpui::px(0.0), gpui::px(-100.0)));
        cx.notify();
    });
    for _ in 0..3 {
        draw(cx);
    }
    let reading_offset = review.read_with(cx, |review, _| review.scroll_handle.offset());
    let files = cx.debug_bounds("activity-files").unwrap().center();
    cx.simulate_click(files, gpui::Modifiers::none());
    for _ in 0..3 {
        draw(cx);
    }
    assert!(cx.debug_bounds("workspace-panel").is_none());
    let focused = cx.update(|window, cx| window.focused(cx));
    assert!(
        focused
            .as_ref()
            .is_some_and(|focus| handles.contains(focus)),
        "当前显示 AI 审阅时，应归还审阅正文焦点：{focused:?}"
    );
    assert!(cx.debug_bounds("ai-review-workspace").is_some());
    assert_eq!(
        review.read_with(cx, |review, _| review.scroll_handle.offset()),
        reading_offset,
        "归还焦点不应把审阅滚回修改位置"
    );
    cx.on_quit(move || std::fs::remove_dir_all(directory).unwrap());
}

#[gpui::test]
fn kit_workspace_navigation_closing_returns_focus_to_visible_ai_review(cx: &mut TestAppContext) {
    navigation_returns_focus_to_ai_review(cx, false);
}

#[gpui::test]
fn kit_workspace_navigation_closing_in_split_returns_focus_to_visible_ai_review(
    cx: &mut TestAppContext,
) {
    navigation_returns_focus_to_ai_review(cx, true);
}
