use super::*;
use crate::ai::DocumentSnapshot;

#[test]
fn ai_session_fails_in_flight_tools_when_a_run_stops() {
    let workspace = AiWorkspace::new(None, Vec::new()).unwrap();
    let mut session = SavedSession::new(workspace);
    session.tool_calls = vec![
        ToolCallRecord {
            message_index: 0,
            id: "running".into(),
            name: "read_document".into(),
            arguments: String::new(),
            status: ToolCallStatus::Running,
            result: String::new(),
        },
        ToolCallRecord {
            message_index: 0,
            id: "done".into(),
            name: "get_outline".into(),
            arguments: String::new(),
            status: ToolCallStatus::Succeeded,
            result: String::new(),
        },
    ];
    session.fail_in_flight_tools();
    assert_eq!(session.tool_calls[0].status, ToolCallStatus::Failed);
    assert_eq!(session.tool_calls[1].status, ToolCallStatus::Succeeded);
}

#[test]
fn ai_session_restores_pending_changes_and_marks_running_tasks_interrupted() {
    let root = std::env::temp_dir().join(format!("velora-ai-session-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let document = DocumentSnapshot {
        id: "note.md".into(),
        source: "原文".into(),
    };
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![document.clone()]).unwrap();
    workspace.changes.propose(document, "原文", "建议").unwrap();
    let mut session = SavedSession::new(workspace);
    session.prompt_profile = Some(crate::ai::instructions::PromptProfile::capture(
        crate::ai::AiCommand::Polish, "en-US", &crate::ai::settings::AiSettings::default(), "",
    ));
    session.running = true;
    session.messages.push(ChatMessage {
        role: "user".into(),
        text: "请修订".into(),
        thinking: "".into()
    });
    let path = root.join("session.json");
    let saved = session.save_at(&path);
    assert!(saved.is_ok(), "{saved:?}");
    let restored = SavedSession::load_at(&path, Some(&root)).unwrap();
    assert_eq!(restored.prompt_profile, session.prompt_profile, "历史恢复后沿用原指令快照");
    assert!(!restored.running);
    assert!(restored.interrupted);
    assert_eq!(&*restored.messages[0].text, "请修订");
    assert_eq!(
        &*restored.workspace.changes.proposals["note.md"].proposed,
        "建议"
    );
    assert!(SavedSession::load_at(&path, Some(&root.join("different"))).is_err());
    std::fs::write(&path, "{\"format\":999}").unwrap();
    assert!(SavedSession::load_at(&path, Some(&root)).is_err());
    assert!(path.exists(), "不能删除无法解析的原记录");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn tool_call_trace_rolls_back_with_its_failed_round_and_never_restores_as_running() {
    let mut session = SavedSession::new(AiWorkspace::new(None, vec![]).unwrap());
    session.messages.push(ChatMessage {
        role: "user".into(),
        text: "第一问".into(),
        thinking: "".into()
    });
    session.messages.push(ChatMessage {
        role: "assistant".into(),
        text: "第一答".into(),
        thinking: "".into()
    });
    session.tool_calls.push(ToolCallRecord {
        message_index: 1,
        id: "call-a".into(),
        name: "read_document".into(),
        arguments: "{\"path\":\"a.md\"}".into(),
        status: ToolCallStatus::Succeeded,
        result: "a".into(),
    });
    session.messages.push(ChatMessage {
        role: "user".into(),
        text: "第二问".into(),
        thinking: "".into()
    });
    session.messages.push(ChatMessage {
        role: "assistant".into(),
        text: String::new().into(),
        thinking: "".into()
    });
    session.tool_calls.push(ToolCallRecord {
        message_index: 3,
        id: "call-b".into(),
        name: "search_documents".into(),
        arguments: "{\"query\":\"UB\"}".into(),
        status: ToolCallStatus::Running,
        result: String::new(),
    });

    session.rollback_failed_conversation_round();
    assert_eq!(session.messages.len(), 2, "失败轮的消息应被回滚");
    assert_eq!(session.tool_calls.len(), 1, "失败轮的工具轨迹也要回滚");
    assert_eq!(session.tool_calls[0].id, "call-a");

    let root = std::env::temp_dir().join(format!("velora-ai-trace-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("session.json");
    let mut restored_source = session.clone();
    restored_source.tool_calls[0].status = ToolCallStatus::Running;
    restored_source.save_at(&path).unwrap();
    let restored = SavedSession::load_at(&path, None).unwrap();
    assert_eq!(
        restored.tool_calls[0].status,
        ToolCallStatus::Failed,
        "上次进程中断留下的 Running 记录不能恢复后永远转圈"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_session_older_background_checkpoints_cannot_replace_a_newer_record() {
    let root = std::env::temp_dir().join(format!("velora-ai-order-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("session.json");
    let mut older = SavedSession::new(AiWorkspace::new(None, vec![]).unwrap());
    older.sequence = 1;
    older.running = true;
    let mut newer = older.clone();
    newer.sequence = 2;
    newer.running = false;
    newer.messages.push(ChatMessage {
        role: "assistant".into(),
        text: "最新完成的结果".into(),
        thinking: "".into()
    });
    newer.save_at(&path).unwrap();
    older.save_at(&path).unwrap();
    let restored = SavedSession::load_at(&path, None).unwrap();
    assert_eq!(restored.sequence, 2);
    assert_eq!(&*restored.messages[0].text, "最新完成的结果");
    assert!(!restored.interrupted);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_session_deleted_history_cannot_be_recreated_by_a_late_background_write() {
    let root = std::env::temp_dir().join(format!("velora-ai-delete-{}", uuid::Uuid::new_v4()));
    let path = root.join("session.json");
    let saved = SavedSession::new(AiWorkspace::new(None, vec![]).unwrap());
    saved.save_at(&path).unwrap();
    SavedSession::delete_at(&path).unwrap();
    assert!(!path.exists());
    saved.save_at(&path).unwrap();
    assert!(!path.exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_session_persists_task_kind_and_recognizes_legacy_quick_command_permissions() {
    let directory = std::env::temp_dir().join(format!("velora-ai-kind-{}", uuid::Uuid::new_v4()));
    let path = directory.join("session.json");
    let mut saved = SavedSession::new(AiWorkspace::new(None, vec![]).unwrap());
    saved.kind = SessionKind::QuickCommand;
    saved.save_at(&path).unwrap();
    assert_eq!(
        SavedSession::load_at(&path, None).unwrap().kind,
        SessionKind::QuickCommand
    );
    saved.kind = SessionKind::Conversation;
    saved.sequence += 1;
    saved.save_at(&path).unwrap();
    assert_eq!(
        SavedSession::load_at(&path, None).unwrap().kind,
        SessionKind::Conversation
    );
    let mut legacy = serde_json::to_value(&saved).unwrap();
    legacy.as_object_mut().unwrap().remove("kind");
    legacy["workspace"]["read_only"] = true.into();
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(
        SavedSession::load_at(&path, None).unwrap().kind,
        SessionKind::QuickCommand
    );
    std::fs::remove_dir_all(directory).unwrap();
}
