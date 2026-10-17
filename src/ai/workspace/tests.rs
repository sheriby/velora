use super::AiWorkspace;
use crate::ai::DocumentSnapshot;
use serde_json::json;
use std::path::PathBuf;

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("velora-ai-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("first.md"), "# 第一篇\n\n磁盘原文。\n").unwrap();
    std::fs::write(root.join("second.md"), "# 第二篇\n\n另一篇笔记。\n").unwrap();
    root
}

#[test]
fn ai_tools_read_unsaved_content_and_revise_their_pending_changes() {
    let root = fixture();
    let snapshot = DocumentSnapshot {
        id: "first.md".into(),
        source: "# 第一篇\n\n尚未保存的内容。\n".into(),
    };
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![snapshot]).unwrap();
    let read = workspace.call("read_document", &json!({"path":"first.md"}));
    assert!(read.is_ok(), "{read:?}");
    assert!(
        read.unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("尚未保存")
    );
    assert!(
        workspace
            .call(
                "propose_edit",
                &json!({"path":"first.md","before":"尚未保存的内容。","after":"AI 建议。"})
            )
            .is_ok()
    );
    assert!(
        workspace
            .call("read_document", &json!({"path":"first.md"}))
            .unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("AI 建议。")
    );
    assert!(
        workspace
            .call(
                "propose_edit",
                &json!({"path":"first.md","before":"AI 建议。","after":"再次校对。"})
            )
            .is_ok()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("first.md")).unwrap(),
        "# 第一篇\n\n磁盘原文。\n"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_tools_search_multiple_notes_and_propose_new_documents_without_writing_files() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    let found = workspace.call("search_documents", &json!({"query":"笔记"}));
    assert!(found.is_ok(), "{found:?}");
    assert!(found.unwrap().to_string().contains("second.md"));
    assert!(
        workspace
            .call(
                "propose_document",
                &json!({"path":"weekly.md","content":"# 周报\n\n综合结果。"})
            )
            .is_ok()
    );
    assert!(!root.join("weekly.md").exists());
    assert!(
        workspace
            .call("read_document", &json!({"path":"weekly.md"}))
            .unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("综合结果")
    );
    assert!(
        workspace
            .call(
                "propose_document",
                &json!({"path":"first.md","content":"覆盖"})
            )
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_tools_reject_paths_outside_the_workspace_and_do_not_expose_generic_tools() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    assert!(
        workspace
            .call("read_document", &json!({"path":"first.md"}))
            .is_ok()
    );
    for path in [
        "../outside.md",
        "/etc/passwd",
        "C:\\outside.md",
        "folder/../../outside.md",
    ] {
        assert!(
            workspace
                .call("read_document", &json!({"path":path}))
                .is_err()
        );
        assert!(
            workspace
                .call("propose_document", &json!({"path":path,"content":"错误"}))
                .is_err()
        );
    }
    for tool in ["bash", "write_file", "mcp", "delete_document"] {
        assert!(workspace.call(tool, &json!({})).is_err());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.parent().unwrap(), root.join("escape")).unwrap();
        assert!(
            workspace
                .call(
                    "propose_document",
                    &json!({"path":"escape/outside.md","content":"错误"})
                )
                .is_err()
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_tools_page_long_lines_without_skipping_text_and_reject_file_parent_paths() {
    let root = fixture();
    let source = "中文🙂".repeat(20_000);
    let document = DocumentSnapshot {
        id: "long.md".into(),
        source: source.clone(),
    };
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![document]).unwrap();
    let mut offset = 0;
    let mut restored = String::new();
    loop {
        let chunk = workspace
            .call(
                "read_document",
                &json!({"path":"long.md","start_offset":offset}),
            )
            .unwrap();
        restored.push_str(chunk["content"].as_str().unwrap());
        let next = chunk["next_offset"].as_u64();
        assert!(next.is_some(), "长行必须返回可继续读取的字节位置");
        if !chunk["truncated"].as_bool().unwrap() {
            break;
        }
        assert!(next.unwrap() > offset);
        offset = next.unwrap();
    }
    assert_eq!(restored, source);
    assert!(
        workspace
            .call(
                "propose_document",
                &json!({"path":"first.md/child.md","content":"不能把文件当成目录"})
            )
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_tools_refresh_live_buffers_and_distinguish_stale_proposals() {
    use std::sync::{Arc, Mutex};
    let source = Arc::new(Mutex::new("新的未保存内容".to_owned()));
    let mut workspace = AiWorkspace::new(
        None,
        vec![DocumentSnapshot {
            id: "note.md".into(),
            source: "旧内容".into(),
        }],
    )
    .unwrap();
    workspace.refresh.callback = Some(Arc::new({
        let source = source.clone();
        move |id| {
            Ok(Some(DocumentSnapshot {
                id: id.into(),
                source: source.lock().unwrap().clone(),
            }))
        }
    }));
    assert_eq!(
        workspace
            .call("read_document", &json!({"path":"note.md"}))
            .unwrap()["content"],
        "新的未保存内容"
    );
    workspace
        .call(
            "propose_edit",
            &json!({"path":"note.md","before":"新的未保存内容","after":"建议"}),
        )
        .unwrap();
    *source.lock().unwrap() = "用户继续编辑".into();
    let latest = workspace
        .call("read_document", &json!({"path":"note.md"}))
        .unwrap();
    assert_eq!(latest["content"], "用户继续编辑");
    assert_eq!(latest["pending_stale"], true);
    workspace
        .call("reset_proposal", &json!({"path":"note.md"}))
        .unwrap();
    workspace
        .call(
            "propose_edit",
            &json!({"path":"note.md","before":"用户继续编辑","after":"重新生成"}),
        )
        .unwrap();
    assert_eq!(workspace.changes.proposals["note.md"].proposed, "重新生成");
}

#[test]
fn ai_context_scope_and_exclusions_apply_to_reads_and_search_results() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    workspace
        .set_scope(&["first.md".into()], "first.md")
        .unwrap();
    assert!(
        workspace
            .call("read_document", &json!({"path":"first.md"}))
            .is_ok()
    );
    assert!(
        workspace
            .call("read_document", &json!({"path":"second.md"}))
            .is_err()
    );
    assert!(
        !workspace
            .call("list_documents", &json!({}))
            .unwrap()
            .to_string()
            .contains("second.md")
    );
    assert!(
        workspace
            .set_scope(&["../outside".into()], "first.md")
            .is_err()
    );
    workspace.set_scope(&[], "first.md").unwrap();
    workspace.excluded_paths.push("second.md".into());
    assert!(
        workspace
            .call("read_document", &json!({"path":"second.md"}))
            .is_err()
    );
    assert!(
        !workspace
            .call("list_documents", &json!({}))
            .unwrap()
            .to_string()
            .contains("second.md")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_review_groups_pending_dependencies_and_invalidates_them_after_rejection() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    workspace
        .call(
            "propose_edit",
            &json!({"path":"first.md","before":"","after":"\n## 新章节"}),
        )
        .unwrap();
    workspace
        .call("read_document", &json!({"path":"first.md"}))
        .unwrap();
    workspace
        .call(
            "propose_edit",
            &json!({"path":"second.md","before":"","after":"\n[first](first.md#新章节)"}),
        )
        .unwrap();
    assert!(
        workspace
            .changes
            .validate_review(&["second.md".into()], false)
            .is_err(),
        "不能单独接受依赖尚未接受标题的链接"
    );
    assert!(
        workspace
            .changes
            .validate_review(&["first.md".into(), "second.md".into()], false)
            .is_ok()
    );
    workspace
        .call("reset_proposal", &json!({"path":"first.md"}))
        .unwrap();
    let read = workspace
        .call("read_document", &json!({"path":"second.md"}))
        .unwrap();
    assert_eq!(read["pending_stale"], true);
    assert!(
        workspace
            .call(
                "propose_edit",
                &json!({"path":"second.md","before":"","after":"more"})
            )
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_sources_keep_read_positions_and_the_version_that_was_read() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    workspace
        .call(
            "read_document",
            &json!({"path":"first.md","start_line":2,"limit":1}),
        )
        .unwrap();
    let encoded = serde_json::to_value(&workspace).unwrap();
    let records = encoded
        .get("source_records")
        .and_then(serde_json::Value::as_array);
    assert!(
        records.is_some_and(|records| !records.is_empty()),
        "来源必须保存具体位置，不能只有文件名"
    );
    let record = &records.unwrap()[0];
    assert_eq!(record["path"], "first.md");
    assert_eq!(record["line"], 2);
    assert!(record["version"].as_u64().is_some());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_source_locations_recover_unique_text_after_edits_and_report_ambiguous_content() {
    let record = super::SourceRecord {
        path: "note.md".into(),
        line: 1,
        range: 0..6,
        version: 0,
        excerpt: "原文".into(),
    };
    assert_eq!(record.locate("新增\n原文").unwrap(), 7..13);
    assert!(record.locate("原文 原文").is_err());
    assert!(record.locate("已删去").is_err());
}

#[test]
fn ai_search_does_not_quote_stale_pending_content_and_creation_respects_exclusions() {
    let mut workspace = AiWorkspace::new(
        None,
        vec![DocumentSnapshot {
            id: "note.md".into(),
            source: "旧文字".into(),
        }],
    )
    .unwrap();
    workspace
        .call(
            "propose_edit",
            &json!({"path":"note.md","before":"旧文字","after":"已经过时的建议"}),
        )
        .unwrap();
    workspace.documents.get_mut("note.md").unwrap().source = "最新原文".into();
    let result = workspace
        .call("search_documents", &json!({"query":"已经过时"}))
        .unwrap();
    assert!(result["matches"].as_array().unwrap().is_empty());
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    workspace.excluded_paths.push("private.md".into());
    assert!(
        workspace
            .call(
                "propose_document",
                &json!({"path":"private.md","content":"不能绕过排除规则"})
            )
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_discarding_proposals_removes_new_drafts_and_allows_the_same_task_to_start_again() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    workspace
        .call(
            "propose_document",
            &json!({"path":"draft.md","content":"第一稿"}),
        )
        .unwrap();
    workspace.changes.invalid.insert("first.md".into());
    workspace.discard_proposals();
    assert!(!workspace.documents.contains_key("draft.md"));
    assert!(!workspace.new_documents.contains("draft.md"));
    assert!(workspace.changes.invalid.is_empty());
    assert!(
        workspace
            .call(
                "propose_document",
                &json!({"path":"draft.md","content":"重新生成"})
            )
            .is_ok()
    );
    std::fs::remove_dir_all(root).unwrap();
}
