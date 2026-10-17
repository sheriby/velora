use super::common::*;
use crate::editor::context_menu::DocumentMenuRow;

#[gpui::test]
fn ai_selection_toolbar_opens_the_assistant_without_changing_the_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "alpha one\n\nbeta two".into(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    editor.update(cx, |editor, _| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _| block.selected_range = 0..5);
    redraw(cx);
    let button = cx.debug_bounds("toolbar-ai");
    assert!(button.is_some(), "选中文字后的工具栏必须有 AI 入口");
    let center = button.unwrap().center();
    cx.simulate_click(center, gpui::Modifiers::none());
    redraw(cx);
    assert!(cx.debug_bounds("ai-assistant-panel").is_some());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "alpha one\n\nbeta two")
    });
}

#[gpui::test]
fn ai_context_menu_exposes_common_commands_and_an_empty_selection_has_clear_scope(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "alpha one".into(), None));
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    editor.update(cx, |editor, _| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _| block.selected_range = 0..5);
    editor.read_with(cx, |editor, cx| {
        let names: Vec<_> = editor
            .document_menu_rows(cx)
            .into_iter()
            .filter_map(|row| match row {
                DocumentMenuRow::Item { name, .. } => Some(name),
                _ => None,
            })
            .collect();
        for name in ["ai-polish", "ai-translate", "ai-summarize", "ai-ask"] {
            assert!(names.contains(&name), "缺少常用指令：{name}");
        }
    });
    block.update(cx, |block, _| block.selected_range = 0..0);
    editor.read_with(cx, |editor, cx| {
        let names: Vec<_> = editor
            .document_menu_rows(cx)
            .into_iter()
            .filter_map(|row| match row {
                DocumentMenuRow::Item { name, .. } => Some(name),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"ai-continue"));
        assert!(!names.contains(&"ai-polish"), "无选区不能默默改成润色全文");
    });
}

#[gpui::test]
fn ai_reading_mode_still_offers_selection_assistance(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "alpha one".into(), None));
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    editor.update(cx, |editor, cx| {
        editor.focus_block(block.entity_id());
        editor.toggle_reading_mode(cx);
    });
    block.update(cx, |block, _| block.selected_range = 0..5);
    redraw(cx);
    assert!(cx.debug_bounds("toolbar-ai").is_some());
    assert!(cx.debug_bounds("toolbar-bold").is_none());
}

#[gpui::test]
fn ai_window_commands_capture_the_active_split_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-ai-command-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("note.md");
    std::fs::write(&path, "目标文档").unwrap();
    let (editor, cx) = cx
        .add_window_view(|_, cx| Editor::from_markdown(cx, "目标文档".into(), Some(path.clone())));
    editor.update_in(cx, |editor, window, cx| {
        editor.split_active_document(
            crate::editor::workspace::split::SplitDirection::Right,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let identity = editor.read_with(cx, |editor, cx| {
        editor
            .active_split_editor()
            .unwrap()
            .read(cx)
            .buffer
            .identity()
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(crate::ai::AiCommand::Ask, window, cx)
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        let target = editor
            .ai_session
            .as_ref()
            .unwrap()
            .saved
            .target
            .as_ref()
            .unwrap();
        assert_eq!(
            target.identity, identity,
            "窗口级入口必须锚定活动分屏，不能取窗口宿主的空缓冲区"
        );
        assert_eq!(target.document_id, "note.md");
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn ai_selection_commands_do_not_silently_expand_to_the_entire_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "完整文档".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(crate::ai::AiCommand::Polish, window, cx);
        assert!(editor.modal_is_open(), "无选区时应提示先选择文字");
        assert!(editor.ai_session.is_none());
        assert_eq!(editor.buffer.text(), "完整文档");
    });
}

#[gpui::test]
fn ai_closing_the_window_preserves_partial_output_and_cancels_execution(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文".into(), None));
    let (path, control) = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(crate::ai::AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.saved.running = true;
        session
            .saved
            .messages
            .push(crate::ai::session::ChatMessage {
                role: "assistant".into(),
                text: "尚未完成的回答".into(),
            });
        (session.saved.path().unwrap(), session.control_for_test())
    });
    editor.update(cx, |editor, _| {
        drop(editor.ai_session.take());
    });
    assert!(control.cancel.is_cancelled());
    assert!(path.exists(), "窗口关闭不能丢失最后一轮的输出");
    let restored = crate::ai::session::SavedSession::load_at(&path, None).unwrap();
    assert!(restored.interrupted);
    assert_eq!(restored.messages[0].text, "尚未完成的回答");
    std::fs::remove_file(path).unwrap();
}

#[gpui::test]
fn ai_answer_links_open_workspace_sources_without_making_the_answer_editable(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-ai-links-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("note.md"), "# Source\n\nContent").unwrap();
    let root = root.canonicalize().unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "当前文档".into(), Some(root.join("nested/current.md")))
    });
    let preview =
        editor.update_in(cx, |editor, window, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.open_ai_assistant(crate::ai::AiCommand::Ask, window, cx);
            editor.ai_session.as_mut().unwrap().saved.messages.push(
                crate::ai::session::ChatMessage {
                    role: "assistant".into(),
                    text: "参考 [note](note.md)".into(),
                },
            );
            editor.refresh_ai_preview(cx);
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .previews
                .values()
                .next_back()
                .unwrap()
                .read(cx)
                .editor
                .clone()
        });
    preview.update(cx, |preview, cx| {
        assert!(preview.is_read_only());
        preview.defer_open_link("note.md".into(), cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.file_path.as_deref(),
            Some(root.join("note.md").as_path())
        )
    });
    preview.read_with(cx, |preview, _| {
        assert_eq!(preview.buffer.text(), "参考 [note](note.md)")
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn ai_task_keeps_the_original_selection_when_the_user_continues_editing(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原始选区 后文".into(), None));
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    editor.update(cx, |editor, _| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _| block.selected_range = 0..12);
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(crate::ai::AiCommand::Ask, window, cx);
        let target = serde_json::to_value(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .saved
                .target
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(target["selection"], "原始选区");
    });
}

#[gpui::test]
fn ai_floating_panel_preserves_the_writing_width_and_expanding_keeps_the_same_task(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "内容".into(), None));
    redraw(cx);
    redraw(cx);
    assert!(cx.debug_bounds("ai-assistant-panel").is_none());
    let initial = cx.debug_bounds("editor-scroll").unwrap();
    let id = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(crate::ai::AiCommand::Ask, window, cx);
        editor.ai_session.as_ref().unwrap().saved.id.clone()
    });
    redraw(cx);
    redraw(cx);
    assert_eq!(
        cx.debug_bounds("editor-scroll").unwrap().size.width,
        initial.size.width
    );
    let panel = cx.debug_bounds("ai-assistant-panel").unwrap();
    let viewport = cx.update(|window, _| window.viewport_size());
    assert!(panel.left() >= gpui::px(0.0) && panel.right() <= viewport.width);
    assert!(panel.top() >= gpui::px(0.0) && panel.bottom() <= viewport.height);
    let expand = cx.debug_bounds("ai-expand").unwrap().center();
    cx.simulate_click(expand, gpui::Modifiers::none());
    redraw(cx);
    redraw(cx);
    assert!(cx.debug_bounds("editor-scroll").unwrap().size.width < initial.size.width);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.ai_session.as_ref().unwrap().saved.id, id)
    });
    let collapse = cx.debug_bounds("ai-expand").unwrap().center();
    cx.simulate_click(collapse, gpui::Modifiers::none());
    redraw(cx);
    redraw(cx);
    assert_eq!(
        cx.debug_bounds("editor-scroll").unwrap().size.width,
        initial.size.width
    );
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.ai_session.as_ref().unwrap().saved.id, id)
    });
}

#[test]
fn ai_documents_outside_the_workspace_do_not_alias_files_with_the_same_name() {
    let base = std::env::temp_dir().join(format!("velora-ai-identity-{}", uuid::Uuid::new_v4()));
    let first = crate::editor::ai_assistant::ai_document_id(
        Some(&base.join("one/note.md")),
        Some(&base.join("workspace")),
        uuid::Uuid::new_v4(),
    );
    let second = crate::editor::ai_assistant::ai_document_id(
        Some(&base.join("two/note.md")),
        Some(&base.join("workspace")),
        uuid::Uuid::new_v4(),
    );
    assert_ne!(first, second, "不同目录的同名文件不能共享提议身份");
}

#[gpui::test]
fn ai_completed_answers_all_keep_their_markdown_previews(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(crate::ai::AiCommand::Ask, window, cx);
        editor
            .ai_session
            .as_mut()
            .unwrap()
            .saved
            .messages
            .extend(
                ["**第一条**", "`第二条`"].map(|text| crate::ai::session::ChatMessage {
                    role: "assistant".into(),
                    text: text.into(),
                }),
            );
        editor.refresh_ai_preview(cx);
        assert_eq!(editor.ai_session.as_ref().unwrap().previews.len(), 2);
    });
}

#[gpui::test]
fn ai_prompt_keyboard_shortcuts_do_not_edit_or_undo_the_document(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    redraw(cx);
    editor.update_in(cx, |editor, window, cx| {
        editor.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        editor.write_back_cross_block_source_edit(0..0, "用户编辑", cx);
        editor.mark_dirty_written_back(cx);
        editor.finalize_pending_undo_capture(cx);
        window.activate_window();
        editor.open_ai_assistant(crate::ai::AiCommand::Ask, window, cx);
    });
    redraw(cx);
    redraw(cx);
    cx.simulate_input("只编辑指令");
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a cmd-x cmd-z"
    } else {
        "ctrl-a ctrl-x ctrl-z"
    });
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "用户编辑正文")
    });
}

#[gpui::test]
fn ai_selection_button_opens_after_a_real_drag_and_tooltip_hover(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "# Velora\n\nA native Markdown editor with a paragraph that can be selected with the mouse.";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.into(), None));
    cx.update(|window, _| window.activate_window());
    redraw(cx);
    redraw(cx);
    let (start, end) = editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[1].entity.read(cx);
        let bounds = block.visible_range_bounds(0..24).unwrap();
        (
            gpui::point(bounds.left() + gpui::px(0.5), bounds.center().y),
            gpui::point(bounds.right() - gpui::px(0.5), bounds.center().y),
        )
    });
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::none());
    redraw(cx);
    redraw(cx);
    let button = cx
        .debug_bounds("toolbar-ai")
        .expect("真实选区必须显示 AI 按钮")
        .center();
    cx.simulate_mouse_move(button, None, gpui::Modifiers::none());
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(800));
    cx.run_until_parked();
    redraw(cx);
    cx.simulate_click(button, gpui::Modifiers::none());
    redraw(cx);
    redraw(cx);
    assert!(
        cx.debug_bounds("ai-assistant-panel").is_some(),
        "悬停提示出现后，点击仍需打开 AI 面板"
    );
    editor.read_with(cx, |editor, _| {
        let target = editor
            .ai_session
            .as_ref()
            .unwrap()
            .saved
            .target
            .as_ref()
            .unwrap();
        assert!(!target.selection.is_empty());
        assert_eq!(editor.buffer.text(), markdown);
    });
}

#[gpui::test]
fn ai_native_menu_action_does_not_read_the_window_during_its_own_dispatch(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    cx.update(crate::app_menu::init);
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原生菜单入口".into(), None));
    redraw(cx);
    cx.update(|window, cx| {
        window.activate_window();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::app_menu::dispatch_menu_action(&crate::components::OpenAiAssistant, cx);
        }));
        assert!(
            result.is_ok(),
            "原生菜单仍在窗口派发栈内时不能重入读取该窗口"
        );
    });
    cx.run_until_parked();
    redraw(cx);
    redraw(cx);
    assert!(cx.debug_bounds("ai-assistant-panel").is_some());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "原生菜单入口")
    });
}

#[gpui::test]
fn ai_selection_button_in_an_editor_group_displays_the_window_assistant(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-ai-group-ui-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("note.md");
    std::fs::write(&path, "alpha selected text").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "alpha selected text".into(), Some(path))
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.split_active_document(
            crate::editor::workspace::split::SplitDirection::Right,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    redraw(cx);
    redraw(cx);
    let group = editor.read_with(cx, |editor, _| editor.active_split_editor().unwrap());
    let block = group.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    group.update(cx, |editor, _| editor.focus_block(block.entity_id()));
    block.update(cx, |block, _| block.selected_range = 0..5);
    redraw(cx);
    redraw(cx);
    let button = cx
        .debug_bounds("toolbar-ai")
        .expect("编辑器组中的选区 AI 按钮")
        .center();
    cx.simulate_click(button, gpui::Modifiers::none());
    cx.run_until_parked();
    redraw(cx);
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(editor.ai_session.is_some(), "任务应由宿主窗口持有")
    });
    assert!(
        cx.debug_bounds("ai-assistant-panel").is_some(),
        "编辑器组宿主必须渲染 AI 面板，不能只创建状态"
    );
    let session_id = editor.read_with(cx, |editor, _| {
        editor.ai_session.as_ref().unwrap().saved.id.clone()
    });
    group.update_in(cx, |editor, window, cx| {
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx)
    });
    cx.run_until_parked();
    redraw(cx);
    redraw(cx);
    assert!(
        cx.debug_bounds("ai-assistant-panel").is_none(),
        "分组内的快捷入口应收起宿主面板"
    );
    group.update_in(cx, |editor, window, cx| {
        editor.on_open_ai_assistant(&crate::components::OpenAiAssistant, window, cx)
    });
    cx.run_until_parked();
    redraw(cx);
    redraw(cx);
    assert!(cx.debug_bounds("ai-assistant-panel").is_some());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.ai_session.as_ref().unwrap().saved.id, session_id)
    });
    std::fs::remove_dir_all(root).unwrap();
}
