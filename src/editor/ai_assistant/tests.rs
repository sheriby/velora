use super::{AiCommand, Editor, PanelMode, RunControl};
use gpui::{TestAppContext, VisualTestContext};

fn initialize(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

#[gpui::test]
fn ai_writing_entry_is_a_compact_command_bar_without_a_conversation(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor.ai_session.as_mut().unwrap().mode = PanelMode::Task;
    });
    draw(cx);
    draw(cx);
    let command = cx.debug_bounds("ai-command-bar").expect("入口应是指令栏");
    assert!(
        f32::from(command.size.height) <= 200.0,
        "入口不承担结果阅读"
    );
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
    let surface = cx
        .debug_bounds("ai-review-workspace")
        .expect("修改应在正文区域审阅");
    let viewport = cx.update(|window, _| window.viewport_size());
    assert!(surface.size.width > viewport.width * 0.65);
    assert!(surface.size.height > viewport.height * 0.65);
    assert!(cx.debug_bounds("ai-command-bar").is_none());
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
fn ai_multiple_document_review_has_file_navigation_outside_the_instruction_bar(
    cx: &mut TestAppContext,
) {
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
    let document = cx.debug_bounds("ai-review-document").unwrap();
    assert!(files.right() <= document.left());
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
        editor.ai_session.as_mut().unwrap().mode = PanelMode::Task;
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
            .proposed
            .clone()
    });
    let (start, end) = draft.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().unwrap().read(cx);
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
    draft.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), proposed));
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
            .insert(id, crate::ai::Proposal::new(baseline, "新文".into()));
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
            crate::ai::Proposal::new(baseline, "修改后的内容".into()),
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
fn ai_opening_another_selection_keeps_existing_unreviewed_changes(cx: &mut TestAppContext) {
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
            crate::ai::Proposal::new(baseline, "待审阅修改".into()),
        );
        editor.select_document(cx);
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_ref().unwrap();
        assert_eq!(
            session.saved.id, task_id,
            "新选区不能悄悄替换尚未处理的任务"
        );
        assert!(session.saved.workspace.changes.proposals.contains_key(&id));
    });
}

#[gpui::test]
fn ai_opening_a_real_document_returns_to_editing_and_keeps_pending_changes(
    cx: &mut TestAppContext,
) {
    initialize(cx);
    let directory =
        std::env::temp_dir().join(format!("velora-ai-review-tabs-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let first = directory.join("first.md");
    let second = directory.join("second.md");
    std::fs::write(&first, "原文").unwrap();
    std::fs::write(&second, "另外一篇").unwrap();
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文".into(), Some(first)));
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
            .insert(id, crate::ai::Proposal::new(baseline, "待审阅内容".into()));
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("ai-review-workspace").is_some());
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(second.clone(), window, cx)
    });
    draw(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("ai-review-workspace").is_none(),
        "切换文件不能仍被审阅页面遮挡"
    );
    assert!(cx.debug_bounds("ai-return").is_some());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "另外一篇");
        assert_eq!(
            editor
                .ai_session
                .as_ref()
                .unwrap()
                .saved
                .workspace
                .changes
                .proposals
                .len(),
            1
        );
    });
    std::fs::remove_dir_all(directory).unwrap();
}

#[gpui::test]
fn ai_reference_picker_keeps_multiple_selections_visible_until_done(cx: &mut TestAppContext) {
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
    let first = cx.debug_bounds("ai-context-choice-0").unwrap().center();
    cx.simulate_click(first, gpui::Modifiers::none());
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.ai_session.as_ref().unwrap().mode == PanelMode::Context,
            "选择一篇资料后不应立即关闭选择器"
        )
    });
    let second = cx.debug_bounds("ai-context-choice-1").unwrap().center();
    cx.simulate_click(second, gpui::Modifiers::none());
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.ai_session.as_ref().unwrap().saved.references,
            ["alpha.md", "beta.md"]
        )
    });
    let first = cx.debug_bounds("ai-context-choice-0").unwrap().center();
    cx.simulate_click(first, gpui::Modifiers::none());
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
    assert!(cx.debug_bounds("ai-command-bar").is_some());
}

#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
fn ai_instruction_to_native_tool_to_document_review_completes_through_the_ui(
    cx: &mut TestAppContext,
) {
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
        for tool in [true, false] {
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
                json!({"role":"assistant","content":"已准备新文档。"})
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
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
    });
    draw(cx);
    cx.simulate_input("新建一篇笔记，等待我审阅");
    cx.simulate_keystrokes("enter");
    for _ in 0..10 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(50));
        draw(cx);
    }
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
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
            crate::ai::Proposal::new(baseline, "修订后的文字".into()),
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
            crate::ai::Proposal::new(baseline, "改写后的文字".into()),
        );
        editor.refresh_ai_preview(cx);
    });
    draw(cx);
    draw(cx);
    editor.update_in(cx, |editor, window, _| {
        window.focus(&editor.ai_session.as_ref().unwrap().review_focus)
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
fn ai_stopping_a_response_keeps_the_received_text_readable_in_the_document_area(
    cx: &mut TestAppContext,
) {
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
        cx.debug_bounds("ai-result-document").is_some(),
        "停止后仍然可以阅读已收到的内容"
    );
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "原文");
        assert!(editor.ai_session.as_ref().unwrap().saved.interrupted);
    });
}

#[gpui::test]
fn editing_connection_inputs_cancels_the_old_probe_and_removes_its_status(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    let control = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.control = RunControl::default();
        session.activity = "old connection status".into();
        session
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.focus(window, cx));
        session.control.clone()
    });
    draw(cx);
    draw(cx);
    cx.simulate_input("changed-model");
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.ai_session.as_ref().unwrap().activity.is_empty(),
            "修改连接后不能继续显示旧连接的测试状态"
        )
    });
    assert!(control.cancel.is_cancelled());
}

#[gpui::test]
fn editing_connection_inputs_does_not_cancel_a_running_document_task(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    let control = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.control = RunControl::default();
        session.saved.running = true;
        session.generation = 20;
        session.activity = "read_document".into();
        session
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.focus(window, cx));
        session.control.clone()
    });
    draw(cx);
    draw(cx);
    cx.simulate_input("changed-model");
    draw(cx);
    assert!(!control.cancel.is_cancelled());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.ai_session.as_ref().unwrap().generation, 20)
    });
}

#[gpui::test]
fn a_background_connection_save_keeps_newer_input_in_the_form(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.set_value("saved-model", cx));
        editor.save_ai_endpoint(cx);
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.set_value("newer-unsaved-model", cx));
        editor.invalidate_ai_connection_test(cx);
    });
    cx.run_until_parked();
    draw(cx);
    editor.read_with(cx, |editor, cx| {
        let session = editor.ai_session.as_ref().unwrap();
        assert!(
            session.draft.is_some(),
            "异步保存完成不能丢弃随后输入的内容"
        );
        assert!(session.mode == PanelMode::Settings);
        assert_eq!(
            session.draft.as_ref().unwrap().model.read(cx).value(),
            "newer-unsaved-model"
        );
        assert_eq!(session.settings.endpoints[0].model, "saved-model");
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
