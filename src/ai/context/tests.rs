use super::explicit_references;

#[test]
fn ai_context_mentions_support_quoted_paths_and_do_not_capture_email_addresses() {
    let references = explicit_references(
        "参考 @notes/资料.md 和 @\"项目 文档/会议记录\"，联系 person@example.com",
    )
    .unwrap();
    assert_eq!(references, vec!["notes/资料.md", "项目 文档/会议记录"]);
    assert!(
        explicit_references("邮件发给 person@example.com")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn ai_context_templates_expand_values_once_and_keep_unknown_variables() {
    assert_eq!(
        super::expand_instruction(
            "校对 {{selection}}；来自 {{document}}，资料 {{references}}，{{unknown}}",
            "{{document}}",
            "笔记.md",
            &["参考.md".into()]
        ),
        "校对 {{document}}；来自 笔记.md，资料 参考.md，{{unknown}}"
    );
    assert_eq!(
        super::explicit_references("参考@文档.md 和 me@example.com").unwrap(),
        vec!["文档.md"]
    );
}

#[test]
fn ai_prompt_cache_keeps_workspace_prefix_stable_across_instructions_tab_order_and_cursor_changes()
{
    let first = serde_json::json!({
        "workspace_path":"/notes", "active_tab":{"document_id":"a.md", "version":1},
        "open_tabs":[{"document_id":"b.md", "path":"/notes/b.md", "modified":true},
            {"document_id":"a.md", "path":"/notes/a.md", "active":true}],
        "cursor":{"byte_offset":1}, "selection":{"text":"甲"}, "near_cursor":{"content":"第一处"}
    });
    let mut second = first.clone();
    second["open_tabs"].as_array_mut().unwrap().reverse();
    second["open_tabs"][1]["modified"] = false.into();
    second["cursor"]["byte_offset"] = 20.into();
    second["selection"]["text"] = "乙".into();
    second["near_cursor"]["content"] = "另一处".into();
    let left = super::task_prompt("解释这一处", &first, None, &[], true, None);
    let right = super::task_prompt("润色另一处", &second, None, &[], true, None);
    let left_prefix = left.split("<editor_context>").next().unwrap();
    let right_prefix = right.split("<editor_context>").next().unwrap();
    assert_eq!(
        left_prefix, right_prefix,
        "光标、指令和标签展示顺序不能破坏固定前缀"
    );
    assert!(left_prefix.contains("/notes/a.md") && left_prefix.contains("/notes/b.md"));
    assert!(left.ends_with("解释这一处"), "本次指令放在易变信息之后");
}

#[test]
fn ai_prompt_cache_sends_the_frozen_target_only_at_the_start_of_a_task() {
    let target = crate::ai::session::TargetSnapshot {
        document_id: "a.md".into(),
        path: None,
        range: 0..9,
        identity: 1,
        selection: "最初的选区原文".into(),
        version: 1,
    };
    let context = serde_json::json!({"workspace_path":"/notes", "open_tabs":[],
        "active_tab":{"document_id":"a.md"}, "cursor":{"byte_offset":12}});
    let first = super::task_prompt("润色", &context, Some(&target), &[], true, None);
    let next = super::task_prompt("语气自然一些", &context, Some(&target), &[], false, None);
    assert!(first.contains("最初的选区原文"));
    assert!(
        !next.contains("最初的选区原文"),
        "续聊通过历史保留原始目标，不重复注入旧选区"
    );
    assert!(next.contains("byte_offset") && next.ends_with("语气自然一些"));
}

#[test]
fn ai_prompt_cache_quick_requirements_precede_the_variable_document_and_selection() {
    let mut context = serde_json::json!({"workspace_path":"/first", "open_tabs":[],
        "selection":{"text":"独立选区甲"}, "cursor":{"byte_offset":4}});
    let first = super::task_prompt(
        "润色",
        &context,
        None,
        &[],
        true,
        Some(crate::ai::instructions::default_instruction(
            crate::ai::AiCommand::Polish,
            "en-US",
        )),
    );
    context["workspace_path"] = "/second".into();
    context["selection"]["text"] = "独立选区乙".into();
    let second = super::task_prompt(
        "润色",
        &context,
        None,
        &[],
        true,
        Some(crate::ai::instructions::default_instruction(
            crate::ai::AiCommand::Polish,
            "en-US",
        )),
    );
    let stable =
        crate::ai::instructions::default_instruction(crate::ai::AiCommand::Polish, "en-US");
    assert!(
        first.starts_with(stable) && second.starts_with(stable),
        "同一快捷指令的固定任务要求必须先于变化的文档资料"
    );
    assert!(!first.contains("独立选区乙") && !second.contains("独立选区甲"));
}
