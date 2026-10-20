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

/// search_documents 现在按精确字符串匹配，不再按空格拆分为 OR 查询。
/// 多个独立主题需要分别调用，由 AI 自行组合结果。
#[test]
fn ai_search_matches_exact_string_not_split_terms() {
    let root = fixture();
    std::fs::write(
        root.join("first.md"),
        "# 对齐\n\n地址对齐要求是 8 字节。\n",
    )
    .unwrap();
    std::fs::write(
        root.join("second.md"),
        "# 未定义行为\n\nUB 来自越界访问。\n",
    )
    .unwrap();
    std::fs::write(root.join("third.md"), "# 缓冲\n\nBuffer 容量固定。\n").unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();

    // 精确匹配 "地址对齐" 这个完整短语
    let found = workspace
        .call("search_documents", &json!({"query":"地址对齐"}))
        .unwrap();
    let paths = found["matches"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["first.md"], "应精确命中包含'地址对齐'的文档：{found}");

    // 精确匹配 "UB"
    let found = workspace
        .call("search_documents", &json!({"query":"UB"}))
        .unwrap();
    let paths = found["matches"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["second.md"], "应精确命中包含'UB'的文档：{found}");

    std::fs::remove_dir_all(root).unwrap();
}

/// 搜索是字面匹配、大小写不敏感，不再按空格拆分为多个词。
#[test]
fn ai_search_literal_match_case_insensitive() {
    let root = fixture();
    std::fs::write(
        root.join("first.md"),
        "# 对齐\n\n地址对齐要求是 8 字节。\n",
    )
    .unwrap();
    std::fs::write(
        root.join("second.md"),
        "# 未定义行为\n\nub 来自越界访问。\n",
    )
    .unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();

    // 大小写不敏感：查询 "UB" 应该匹配到 "ub"
    let found = workspace
        .call("search_documents", &json!({"query":"UB"}))
        .unwrap();
    let paths = found["matches"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["second.md"], "应大小写无关地命中：{found}");

    std::fs::remove_dir_all(root).unwrap();
}

/// 匹配换到 ripgrep 引擎后，词里的正则元字符仍必须按字面处理：
/// `a.b` 只命中字面的 `a.b`，不能把 `axb` 也搜出来。
#[test]
fn ai_search_treats_query_terms_as_literals_not_patterns() {
    let root = fixture();
    std::fs::write(root.join("first.md"), "# 字面\n\n版本 a.b 已发布。\n").unwrap();
    std::fs::write(root.join("second.md"), "# 正则\n\n版本 axb 未发布。\n").unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();

    let found = workspace
        .call("search_documents", &json!({"query":"a.b"}))
        .unwrap();
    let paths = found["matches"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"first.md"), "应命中字面的 a.b：{found}");
    assert!(!paths.contains(&"second.md"), "不能按正则命中 axb：{found}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_search_rejects_whitespace_only_queries() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    assert!(
        workspace
            .call("search_documents", &json!({"query":" \t\u{3000} "}))
            .is_err(),
        "只有空白的 query 拆不出任何词，应拒绝"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// 未保存的活文档只能按缓冲区内容命中；不能因为磁盘上有同名旧文件
/// 就走并行磁盘快路径把旧正文搜出来。
#[test]
fn ai_search_prefers_live_buffer_content_over_disk() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    workspace.live_documents.insert("first.md".into());
    workspace.refresh.callback = Some(std::sync::Arc::new(|id| {
        if id == "first.md" {
            Ok(Some(DocumentSnapshot {
                id: "first.md".into(),
                source: "# 第一篇\n\n未保存草稿里的独有词。\n".into(),
            }))
        } else {
            Ok(None)
        }
    }));

    let live = workspace
        .call("search_documents", &json!({"query":"独有词"}))
        .unwrap();
    assert!(
        live["matches"].to_string().contains("first.md"),
        "活文档应按未保存缓冲区命中：{live}"
    );
    let disk = workspace
        .call("search_documents", &json!({"query":"磁盘原文"}))
        .unwrap();
    assert!(
        disk["matches"].as_array().unwrap().is_empty(),
        "活文档不能回落到磁盘旧正文：{disk}"
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
        source: source.clone().into(),
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
                source: source.lock().unwrap().clone().into(),
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
    assert_eq!(&*workspace.changes.proposals["note.md"].proposed, "重新生成");
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

#[test]
fn ai_reading_tasks_reject_all_proposal_tools_and_leave_content_unchanged() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    workspace.read_only = true;
    assert!(
        workspace
            .call("read_document", &json!({"path":"first.md"}))
            .is_ok()
    );
    for (name, arguments) in [
        (
            "propose_edit",
            json!({"path":"first.md","before":"磁盘原文。","after":"不应写入"}),
        ),
        (
            "propose_document",
            json!({"path":"answer.md","content":"不应创建"}),
        ),
        ("reset_proposal", json!({"path":"first.md"})),
    ] {
        assert!(
            workspace.call(name, &arguments).is_err(),
            "阅读任务不能调用 {name}"
        );
        assert!(workspace.changes.proposals.is_empty());
    }
    assert!(!root.join("answer.md").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("first.md")).unwrap(),
        "# 第一篇\n\n磁盘原文。\n"
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn scoped_fixture() -> (PathBuf, AiWorkspace, std::ops::Range<usize>) {
    let root = fixture();
    let source = "前文保持不变。\n\n选中的框架🙂\n\n后文保持不变。\n";
    std::fs::write(root.join("first.md"), source).unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    let start = source.find("选中的框架🙂").unwrap();
    let range = start..start + "选中的框架🙂".len();
    workspace.selection_scope = Some(super::SelectionScope {
        document_id: "first.md".into(),
        range: range.clone(),
        version: crate::editor::persistence::file_content_version(source),
    });
    (root, workspace, range)
}

#[test]
fn ai_scoped_edit_cannot_touch_other_text_or_other_documents() {
    let (root, mut workspace, range) = scoped_fixture();
    workspace
        .call(
            "propose_edit",
            &json!({"path":"first.md","before":"选中的框架🙂","after":"已经扩写的框架🙂"}),
        )
        .unwrap();
    let approved = workspace.changes.proposals["first.md"].proposed.clone();
    for (tool, args) in [
        (
            "propose_edit",
            json!({"path":"first.md","before":"前文保持不变。","after":"越界修改"}),
        ),
        (
            "propose_edit",
            json!({"path":"first.md","before":"后文保持不变。","after":"越界修改"}),
        ),
        (
            "propose_edit",
            json!({"path":"second.md","before":"另一篇笔记。","after":"越界修改"}),
        ),
        (
            "propose_document",
            json!({"path":"new.md","content":"不允许新建"}),
        ),
    ] {
        assert!(
            workspace.call(tool, &args).is_err(),
            "只能修改原选区：{tool}"
        );
        assert_eq!(workspace.changes.proposals["first.md"].proposed.as_ref(), approved.as_ref());
        assert_eq!(workspace.changes.proposals.len(), 1);
    }
    let original = std::fs::read_to_string(root.join("first.md")).unwrap();
    assert_eq!(original.get(range).unwrap(), "选中的框架🙂");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_scoped_edit_appends_at_the_selection_end_and_disambiguates_equal_text_outside_it() {
    let (root, mut workspace, _) = scoped_fixture();
    workspace
        .call(
            "propose_edit",
            &json!({"path":"first.md","before":"","after":"，补充一条说明"}),
        )
        .unwrap();
    assert!(
        workspace.changes.proposals["first.md"]
            .proposed
            .contains("选中的框架🙂，补充一条说明\n\n后文")
    );
    let source = "同一句\n\n同一句\n\n结尾";
    std::fs::write(root.join("second.md"), source).unwrap();
    workspace.changes = Default::default();
    let start = source.rfind("同一句").unwrap();
    workspace.selection_scope = Some(super::SelectionScope {
        document_id: "second.md".into(),
        range: start..start + "同一句".len(),
        version: crate::editor::persistence::file_content_version(source),
    });
    workspace
        .call(
            "propose_edit",
            &json!({"path":"second.md","before":"同一句","after":"仅修改选中的一句"}),
        )
        .unwrap();
    assert_eq!(
        &*workspace.changes.proposals["second.md"].proposed,
        "同一句\n\n仅修改选中的一句\n\n结尾"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_insert_at_cursor_is_a_pending_edit_and_repeated_inserts_keep_the_document_unchanged() {
    let source = "前文🙂｜后文";
    let offset = source.find('｜').unwrap();
    let mut workspace = AiWorkspace::new(
        None,
        vec![DocumentSnapshot {
            id: "note.md".into(),
            source: source.into(),
        }],
    )
    .unwrap();
    workspace.cursor_target = Some(super::CursorTarget {
        document_id: "note.md".into(),
        offset,
        version: crate::editor::persistence::file_content_version(source),
    });
    let result = workspace.call("propose_insert_at_cursor", &json!({"content":"插入一"}));
    assert!(result.is_ok(), "应在捕获的光标处生成插入提议：{result:?}");
    assert_eq!(
        &*workspace.changes.proposals["note.md"].proposed,
        "前文🙂插入一｜后文"
    );
    workspace
        .call("propose_insert_at_cursor", &json!({"content":"插入二"}))
        .unwrap();
    assert_eq!(
        &*workspace.changes.proposals["note.md"].proposed,
        "前文🙂插入一插入二｜后文"
    );
    assert_eq!(&*workspace.documents["note.md"].source, source);
    workspace.read_only = true;
    assert!(
        workspace
            .call(
                "propose_insert_at_cursor",
                &json!({"content":"阅读结果不应插入"})
            )
            .is_err()
    );
}

#[test]
fn ai_outline_skips_front_matter_and_quoted_headings_like_the_editor_outline() {
    let mut workspace = AiWorkspace::new(
        None,
        vec![DocumentSnapshot {
            id: "note.md".into(),
            source: "---\ntitle: 元数据\n---\n\n# 正文标题\n\n> ## 引用块里的行\n"
                .into(),
        }],
    )
    .unwrap();
    let outline = workspace.call("get_outline", &json!({"path":"note.md"})).unwrap();
    let headings = outline["headings"].as_array().unwrap();
    assert_eq!(
        headings.len(),
        1,
        "front matter 的伪 setext 标题与引用块内标题不应进入大纲：{outline}"
    );
    assert_eq!(headings[0]["title"].as_str().unwrap(), "正文标题");
    assert_eq!(headings[0]["line"].as_u64().unwrap(), 5);
}

#[test]
fn ai_search_reading_pending_proposals_registers_dependencies() {
    let mut workspace = AiWorkspace::new(
        None,
        vec![
            DocumentSnapshot {
                id: "a.md".into(),
                source: "甲的旧文".into(),
            },
            DocumentSnapshot {
                id: "b.md".into(),
                source: "乙的正文".into(),
            },
        ],
    )
    .unwrap();
    workspace
        .call(
            "propose_edit",
            &json!({"path":"a.md","before":"甲的旧文","after":"甲的新句"}),
        )
        .unwrap();
    let found = workspace.call("search_documents", &json!({"query":"甲的新句"}));
    assert!(found.is_ok(), "{found:?}");
    workspace
        .call(
            "propose_edit",
            &json!({"path":"b.md","before":"乙的正文","after":"乙的改写"}),
        )
        .unwrap();
    workspace
        .call("reset_proposal", &json!({"path":"a.md"}))
        .unwrap();
    assert!(
        workspace.changes.invalid.contains("b.md"),
        "依据待审阅内容搜索后提出的修改，应在前置提议重置时一并失效"
    );
}

#[test]
fn ai_snapshot_rereads_disk_after_a_live_document_closes() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    let from_disk = workspace
        .call("read_document", &json!({"path":"first.md"}))
        .unwrap();
    assert!(from_disk["content"].as_str().unwrap().contains("磁盘原文"));
    let live = std::sync::Arc::new(std::sync::Mutex::new(Some(DocumentSnapshot {
        id: "first.md".into(),
        source: "# 第一篇\n\n未保存草稿。\n".into(),
    })));
    workspace.live_documents.insert("first.md".into());
    let bridge = live.clone();
    workspace.refresh.callback = Some(std::sync::Arc::new(move |id| {
        if id == "first.md" {
            Ok(bridge.lock().unwrap().clone())
        } else {
            Ok(None)
        }
    }));
    let from_live = workspace
        .call("read_document", &json!({"path":"first.md"}))
        .unwrap();
    assert!(from_live["content"].as_str().unwrap().contains("未保存草稿"));
    *live.lock().unwrap() = None;
    let after_close = workspace
        .call("read_document", &json!({"path":"first.md"}))
        .unwrap();
    assert_eq!(
        after_close["content"].as_str().unwrap(),
        from_disk["content"].as_str().unwrap(),
        "标签关闭后应回读磁盘，而不是返回已丢弃的草稿"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn ai_workspace_listing_tolerates_unreadable_entries() {
    let root = fixture();
    std::fs::write(root.join("open.md"), "# 开着的\n").unwrap();
    std::fs::create_dir_all(root.join("locked")).unwrap();
    std::fs::write(root.join("locked/inner.md"), "# 锁住的\n").unwrap();
    let mut permissions = std::fs::metadata(root.join("locked")).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o000);
    std::fs::set_permissions(root.join("locked"), permissions).unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    let listed = workspace.call("list_documents", &json!({}));
    assert!(
        listed.is_ok(),
        "单个不可读条目不应让整个名单失败：{listed:?}"
    );
    let documents = listed.unwrap()["documents"].as_array().unwrap().clone();
    assert!(documents.iter().any(|file| file.as_str() == Some("open.md")));
    let mut permissions = std::fs::metadata(root.join("locked")).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(root.join("locked"), permissions).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn ai_new_document_targets_must_not_be_dangling_symlinks() {
    let root = fixture();
    std::os::unix::fs::symlink(root.join("不存在.md"), root.join("evil.md")).unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    let result = workspace.call(
        "propose_document",
        &json!({"path":"evil.md","content":"# 覆盖链接"}),
    );
    assert!(
        result.is_err(),
        "悬空符号链接不应当作可新建的文档目标"
    );
    std::fs::remove_file(root.join("evil.md")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_propose_within_reports_overlapping_occurrences() {
    let mut workspace = AiWorkspace::new(
        None,
        vec![DocumentSnapshot {
            id: "n.md".into(),
            source: "aaa".into(),
        }],
    )
    .unwrap();
    let result = workspace.call("propose_edit", &json!({"path":"n.md","before":"aa","after":"bb"}));
    let error = result.expect_err("重叠出现应按多处匹配处理");
    assert!(
        error.to_string().contains("多处匹配"),
        "应报告多处匹配：{error}"
    );
}

fn found_documents(workspace: &mut AiWorkspace, arguments: serde_json::Value) -> Vec<String> {
    let found = workspace
        .call("find_documents", &arguments)
        .expect("find_documents 应成功");
    let documents = found["documents"].as_array().expect("应返回 documents 数组");
    assert_eq!(
        found["total"].as_u64(),
        Some(documents.len() as u64),
        "total 必须与实际返回的条数一致：{found}"
    );
    documents
        .iter()
        .map(|file| file.as_str().unwrap().to_owned())
        .collect()
}

/// find_documents 的递归写法是 `**`；返回的必须是工作区相对路径，
/// 非文本文档（图片）根本不进名单。
#[test]
fn ai_find_documents_matches_glob_patterns_and_returns_relative_paths() {
    let root = fixture();
    std::fs::create_dir_all(root.join("docs/nested")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# 指南\n").unwrap();
    std::fs::write(root.join("docs/nested/deep.md"), "# 更深\n").unwrap();
    std::fs::write(root.join("notes.txt"), "杂记\n").unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("cover.png"), b"\x89PNG\r\n").unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();

    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"**/*.md"})),
        vec!["docs/guide.md", "docs/nested/deep.md", "first.md", "second.md"]
    );
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"**/*.txt"})),
        vec!["notes.txt"]
    );
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"**/*.rs"})),
        vec!["main.rs"]
    );
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"docs/nested/**"})),
        vec!["docs/nested/deep.md"]
    );
    let everything = found_documents(&mut workspace, json!({"pattern":"**"}));
    assert!(everything.contains(&"notes.txt".to_owned()));
    assert!(
        !everything.iter().any(|path| path.ends_with(".png")),
        "图片不能出现在文本文档名单里：{everything:?}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// 模式大小写不敏感：`.MD` 与 `.md` 找到的必须是同一批文档。
#[test]
fn ai_find_documents_ignores_pattern_case() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"**/*.MD"})),
        found_documents(&mut workspace, json!({"pattern":"**/*.md"})),
    );
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"**/SECOND.md"})),
        vec!["second.md"]
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// folder 是模式的匹配起点：名单先按 `folder/` 截取，模式再对截出来的
/// 相对路径求值；同前缀的兄弟目录（docs-archive）不能被卷进来。
#[test]
fn ai_find_documents_matches_relative_to_the_given_folder() {
    let root = fixture();
    std::fs::create_dir_all(root.join("docs/nested")).unwrap();
    std::fs::create_dir_all(root.join("docs-archive")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# 指南\n").unwrap();
    std::fs::write(root.join("docs/nested/deep.md"), "# 更深\n").unwrap();
    std::fs::write(root.join("docs-archive/old.md"), "# 归档\n").unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();

    let expected = vec!["docs/guide.md".to_owned(), "docs/nested/deep.md".to_owned()];
    for folder in ["docs", "docs/"] {
        assert_eq!(
            found_documents(&mut workspace, json!({"pattern":"**/*.md","folder":folder})),
            expected,
            "folder={folder} 只能在 docs 下匹配"
        );
    }
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"nested/*.md","folder":"docs"})),
        vec!["docs/nested/deep.md"]
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// find_documents 与 list_documents 共用同一份名单：排除规则、资料范围与
/// 本任务尚未落盘的草稿都要按同一口径生效。
#[test]
fn ai_find_documents_respects_exclusions_scope_and_pending_drafts() {
    let root = fixture();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# 指南\n").unwrap();
    std::fs::write(root.join("private.md"), "# 私密\n").unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();

    workspace.excluded_paths.push("private.md".into());
    let listed = found_documents(&mut workspace, json!({"pattern":"**/*.md"}));
    assert!(
        !listed.contains(&"private.md".to_owned()),
        "被排除的文档不能出现在查找结果里：{listed:?}"
    );

    workspace
        .call(
            "propose_document",
            &json!({"path":"draft.md","content":"# 草稿\n"}),
        )
        .unwrap();
    assert!(
        found_documents(&mut workspace, json!({"pattern":"**/*.md"})).contains(&"draft.md".to_owned()),
        "待审阅的新文档必须能被查回"
    );

    workspace.set_scope(&["docs".into()], "docs").unwrap();
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"**/*.md"})),
        vec!["docs/guide.md", "draft.md"],
        "限定资料后只剩该目录，但本任务草稿不受限定"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_find_documents_reports_bad_patterns_and_works_without_a_workspace() {
    let root = fixture();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();
    let missing = workspace
        .call("find_documents", &json!({}))
        .expect_err("pattern 是必填参数");
    assert!(
        missing.to_string().contains("pattern"),
        "应报出缺少的参数名：{missing}"
    );
    let invalid = workspace
        .call("find_documents", &json!({"pattern":"[a"}))
        .expect_err("未闭合的字符类不是合法 glob");
    assert!(
        invalid.to_string().contains("glob"),
        "应说明是 glob 模式无效：{invalid}"
    );
    std::fs::remove_dir_all(root).unwrap();

    let mut without_root = AiWorkspace::new(None, vec![]).unwrap();
    assert!(
        found_documents(&mut without_root, json!({"pattern":"**/*.md"})).is_empty(),
        "没有工作区根目录时只能返回空名单，不能失败"
    );
}

/// 工具描述的口径是「`*` 不含 `/`，递归写 `**`」：单层 `*` 不能越过目录，
/// 越界会把「找 docs 下的文件名」变成全库扫描。
#[test]
fn ai_find_documents_keeps_single_star_within_one_directory() {
    let root = fixture();
    std::fs::create_dir_all(root.join("docs/nested")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "# 指南\n").unwrap();
    std::fs::write(root.join("docs/nested/deep.md"), "# 更深\n").unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();

    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"*.md"})),
        vec!["first.md", "second.md"],
        "单层 * 只能匹配工作区根下的这一层"
    );
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"docs/*.md"})),
        vec!["docs/guide.md"],
        "docs/*.md 不能越过一层拿到 docs/nested/deep.md"
    );
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"docs/**/*.md"})),
        vec!["docs/guide.md", "docs/nested/deep.md"],
        "** 跨越零层或多层目录"
    );
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"**/deep.md"})),
        vec!["docs/nested/deep.md"]
    );
    assert_eq!(
        found_documents(&mut workspace, json!({"pattern":"**/*.md"})),
        vec!["docs/guide.md", "docs/nested/deep.md", "first.md", "second.md"],
        "**/ 允许匹配零层目录，根下的文档也要能查回"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// 精确匹配的另一半：带空格的整串短语必须能命中同一行的原文，
/// 而不是因为被拆开而命中只含其中一个词的文档。
#[test]
fn ai_search_matches_a_spaced_phrase_only_where_it_applies_as_a_whole() {
    let root = fixture();
    std::fs::write(root.join("first.md"), "# 大纲\n\nget outline 的用法。\n").unwrap();
    std::fs::write(root.join("second.md"), "# 别的\n\n只提到 get 而已。\n").unwrap();
    let mut workspace = AiWorkspace::new(Some(root.clone()), vec![]).unwrap();

    let found = workspace
        .call("search_documents", &json!({"query":"get outline"}))
        .unwrap();
    let paths = found["matches"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["first.md"], "整串短语只应命中原文里有它的行：{found}");
    std::fs::remove_dir_all(root).unwrap();
}

