use anyhow::Result;

pub(crate) fn explicit_references(prompt: &str) -> Result<Vec<String>> {
    let expression = regex::Regex::new(r#"@(?:"([^"]+)"|([^\s，。；,;]+))"#)?;
    let mut references = Vec::new();
    for capture in expression.captures_iter(prompt) {
        if capture.get(0).is_some_and(|whole| {
            prompt
                .get(..whole.start())
                .and_then(|prefix| prefix.chars().next_back())
                .is_some_and(|previous| {
                    previous.is_ascii_alphanumeric() || matches!(previous, '_' | '.' | '+' | '-')
                })
        }) {
            continue;
        }
        if let Some(path) = capture.get(1).or_else(|| capture.get(2)) {
            let path = path.as_str().trim_end_matches('/').to_owned();
            if !path.is_empty() && !references.contains(&path) {
                references.push(path);
            }
        }
    }
    Ok(references)
}

#[cfg(test)]
mod tests;

pub(crate) fn expand_instruction(
    template: &str,
    selection: &str,
    document: &str,
    references: &[String],
) -> String {
    let mut result = String::new();
    let mut remaining = template;
    while let Some(start) = remaining.find("{{") {
        result.push_str(remaining.get(..start).unwrap_or_default());
        let tail = remaining.get(start..).unwrap_or_default();
        let Some(end) = tail.find("}}") else {
            result.push_str(tail);
            return result;
        };
        let variable = tail.get(2..end).unwrap_or_default().trim();
        match variable {
            "selection" => result.push_str(selection),
            "document" => result.push_str(document),
            "references" => result.push_str(&references.join(", ")),
            _ => result.push_str(tail.get(..end + 2).unwrap_or_default()),
        }
        remaining = tail.get(end + 2..).unwrap_or_default();
    }
    result.push_str(remaining);
    result
}

pub(crate) fn task_prompt(
    task: &str,
    editor_context: &serde_json::Value,
    target: Option<&super::session::TargetSnapshot>,
    references: &[String],
    initial: bool,
    quick_instruction: Option<&str>,
) -> String {
    use serde_json::json;
    let mut prompt = String::new();
    if initial {
        if let Some(instruction) = quick_instruction {
            prompt.push_str(instruction);
            prompt.push_str("\n\n");
        }
    }
    if initial {
        let mut tabs = std::collections::BTreeMap::new();
        if let Some(open_tabs) = editor_context
            .get("open_tabs")
            .and_then(serde_json::Value::as_array)
        {
            for tab in open_tabs {
                if let Some(id) = tab.get("document_id").and_then(serde_json::Value::as_str) {
                    tabs.insert(id, json!({"document_id":id, "path":tab.get("path")}));
                }
            }
        }
        let references = references.iter().collect::<std::collections::BTreeSet<_>>();
        prompt.push_str(&format!(
            "<workspace_context>\n{}\n</workspace_context>\n",
            json!({
                "workspace_name": editor_context.get("workspace_name"),
                "open_tabs": tabs.values().collect::<Vec<_>>(), "references": references,
            })
        ));
        if let Some(target) = target {
            prompt.push_str(&format!(
                "<task_origin>\n{}\n</task_origin>\n",
                json!({
                    "document_id":target.document_id, "version":target.version,
                    "range":{"start":target.range.start, "end":target.range.end},
                    "truncated":target.selection.chars().count() > 8192,
                })
            ));
            if !target.selection.is_empty() {
                prompt.push_str("<original_selection>\n");
                prompt.extend(target.selection.chars().take(8192));
                prompt.push_str("\n</original_selection>\n");
            }
            if target.selection.chars().count() > 8192 {
                prompt.push_str("<selection_truncated read_full_range_with=\"read_document\" />\n");
            }
        }
    }
    // 原始任务范围只写入首轮消息；续聊追加状态与要求，不能改写已发送的历史。
    prompt.push_str(&format!(
        "<editor_context>\n{editor_context}\n</editor_context>\n"
    ));
    if !initial && !references.is_empty() {
        let references = references.iter().collect::<std::collections::BTreeSet<_>>();
        prompt.push_str(&format!("<references>{}</references>\n", json!(references)));
    }
    prompt.push_str("<user_request>\n");
    prompt.push_str(task);
    prompt.push_str("\n</user_request>");
    prompt
}
