use super::*;
use serde_json::{Value, json};

impl Editor {
    fn ai_active_editor_context(&self, root: Option<&Path>, cx: &App) -> Value {
        let source = self.buffer.text();
        let selection = self.capture_source_selection_snapshot(cx);
        let offset = source.floor_char_boundary(
            if selection.reversed {
                selection.range.start
            } else {
                selection.range.end
            }
            .min(source.len()),
        );
        let prefix = source.get(..offset).unwrap_or_default();
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count()
            + 1;
        let selected = source.get(selection.range.clone()).unwrap_or_default();
        let excerpt_start = source.floor_char_boundary(offset.saturating_sub(4096));
        let excerpt_end = source.floor_char_boundary(offset.saturating_add(4096).min(source.len()));
        json!({
            "active_tab": { "document_id": ai_document_id(self.file_path.as_deref(), root, self.recovery_id),
                "path": self.active_document_path()
                    .and_then(|path| ai_workspace_relative_path(path, root)), "modified": self.document_dirty,
                "version": super::super::persistence::file_content_version(&source) },
            "cursor": {"byte_offset": offset, "line": line, "column": column},
            "selection": {"start": selection.range.start, "end": selection.range.end,
                "reversed": selection.reversed, "text": selected.chars().take(8192).collect::<String>(),
                "truncated": selected.chars().count() > 8192},
            "near_cursor": {"start_offset": excerpt_start,
                "content": source.get(excerpt_start..excerpt_end).unwrap_or_default(),
                "truncated_before": excerpt_start > 0, "truncated_after": excerpt_end < source.len()}
        })
    }

    pub(super) fn ai_editor_context(&self, cx: &App) -> Value {
        let active_group = self.active_split_editor();
        let active = active_group.as_ref().map(|group| group.read(cx));
        let active = active.unwrap_or(self);
        let root = active
            .workspace_root_path()
            .map(PathBuf::from)
            .or_else(|| {
                active
                    .file_path
                    .as_ref()
                    .and_then(|path| path.parent().map(PathBuf::from))
            })
            .and_then(|path| path.canonicalize().ok());
        let mut context = active.ai_active_editor_context(root.as_deref(), cx);
        let groups = self.split_groups();
        let mut tabs = if groups.is_empty() {
            self.ai_open_tabs_context(root.as_deref(), 1, true)
        } else {
            groups
                .iter()
                .enumerate()
                .flat_map(|(index, group)| {
                    group.read(cx).ai_open_tabs_context(
                        root.as_deref(),
                        index + 1,
                        active_group
                            .as_ref()
                            .is_some_and(|active| active.entity_id() == group.entity_id()),
                    )
                })
                .collect::<Vec<_>>()
        };
        if let Some(active_tab) = context.get("active_tab")
            && !tabs
                .iter()
                .any(|tab| tab.get("document_id") == active_tab.get("document_id"))
        {
            tabs.push(active_tab.clone());
        }
        let excluded = context
            .get("active_tab")
            .and_then(|tab| tab.get("document_id"))
            .and_then(Value::as_str)
            .is_some_and(|id| {
                self.ai_session
                    .as_ref()
                    .is_some_and(|session| session.saved.workspace.is_excluded(id))
            });
        if let Some(fields) = context.as_object_mut() {
            if excluded {
                fields.remove("selection");
                fields.remove("near_cursor");
            }
            // 只传工作区名称，不传绝对路径，避免 AI 在工具调用中使用绝对路径
            let workspace_name = root.as_ref().and_then(|path| {
                path.file_name().and_then(|name| name.to_str()).map(|name| name.to_owned())
            });
            fields.insert("workspace_name".into(), json!(workspace_name));
            fields.insert("open_tabs".into(), json!(tabs));
        }
        context
    }
}
