use super::*;
use crate::ai::DocumentSnapshot;

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
    });
    let path = root.join("session.json");
    let saved = session.save_at(&path);
    assert!(saved.is_ok(), "{saved:?}");
    let restored = SavedSession::load_at(&path, Some(&root)).unwrap();
    assert_eq!(restored.prompt_profile, session.prompt_profile, "历史恢复后沿用原指令快照");
    assert!(!restored.running);
    assert!(restored.interrupted);
    assert_eq!(restored.messages[0].text, "请修订");
    assert_eq!(
        restored.workspace.changes.proposals["note.md"].proposed,
        "建议"
    );
    assert!(SavedSession::load_at(&path, Some(&root.join("different"))).is_err());
    std::fs::write(&path, "{\"format\":999}").unwrap();
    assert!(SavedSession::load_at(&path, Some(&root)).is_err());
    assert!(path.exists(), "不能删除无法解析的原记录");
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
    });
    newer.save_at(&path).unwrap();
    older.save_at(&path).unwrap();
    let restored = SavedSession::load_at(&path, None).unwrap();
    assert_eq!(restored.sequence, 2);
    assert_eq!(restored.messages[0].text, "最新完成的结果");
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
