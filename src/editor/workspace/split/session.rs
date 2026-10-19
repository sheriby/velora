use super::*;
use crate::config::EditorGroupLayout;

impl GroupNode {
    fn session_layout(&self, active: EntityId, cx: &App) -> EditorGroupLayout {
        match self {
            Self::Editor(group) => {
                let editor = group.read(cx);
                EditorGroupLayout::Editor {
                    tabs: editor
                        .workspace
                        .open_documents
                        .iter()
                        .map(|tab| tab.path.to_string_lossy().into_owned())
                        .collect(),
                    active: editor
                        .workspace
                        .active_document
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    focused: group.entity_id() == active,
                }
            }
            Self::Split {
                vertical,
                ratio,
                first,
                second,
                ..
            } => EditorGroupLayout::Split {
                vertical: *vertical,
                ratio: (*ratio * 1000.0).round() as u16,
                first: Box::new(first.session_layout(active, cx)),
                second: Box::new(second.session_layout(active, cx)),
            },
        }
    }
}

impl Editor {
    pub(crate) fn split_session_layout(&self, cx: &App) -> Option<EditorGroupLayout> {
        let workspace = self.split_workspace.as_ref()?;
        Some(workspace.layout.session_layout(workspace.active, cx))
    }

    pub(crate) fn restore_split_session(
        &mut self,
        layout: &EditorGroupLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_split_workspace(window, cx);
        let mut active = None;
        let layout = self.restore_group_node(layout, &mut active, window, cx);
        if let Some(workspace) = self.split_workspace.as_mut() {
            workspace.layout = layout;
            workspace.documents.clear();
            workspace.subscriptions.clear();
        }
        let groups = self.split_groups();
        for group in &groups {
            self.sync_shared_groups(group.entity_id(), cx);
            self.observe_editor_group(group, cx);
        }
        if let Some(active) = active.or_else(|| groups.first().map(Entity::entity_id)) {
            self.activate_split_group(active, cx);
        }
        self.remove_empty_split_groups(cx);
        self.persist_session(cx);
        cx.notify();
    }

    fn restore_group_node(
        &mut self,
        layout: &EditorGroupLayout,
        active_group: &mut Option<EntityId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> GroupNode {
        match layout {
            EditorGroupLayout::Editor {
                tabs,
                active,
                focused,
            } => {
                let group = self.new_editor_group(Vec::new(), None, window, cx);
                group.update(cx, |editor, cx| {
                    editor.split_initializing = true;
                    let order = tabs.iter().map(PathBuf::from).collect::<Vec<_>>();
                    editor.restore_untitled_tabs(&order, cx);
                    for path in tabs.iter().map(PathBuf::from).filter(|path| {
                        path.is_file()
                            || crate::editor::workspace::untitled_document_id(path).is_some()
                    }) {
                        editor.open_workspace_file(path, window, cx);
                    }
                    if let Some(path) = active.as_ref().map(PathBuf::from).filter(|path| {
                        path.is_file()
                            || crate::editor::workspace::untitled_document_id(path).is_some()
                    }) {
                        editor.open_workspace_file_in_mode(
                            path,
                            WorkspaceOpenMode::Activate,
                            window,
                            cx,
                        );
                    }
                    editor.restore_workspace_tab_order(&order, cx);
                    editor.split_initializing = false;
                    editor.split_projection_revision = editor.buffer.revision();
                });
                if *focused {
                    *active_group = Some(group.entity_id());
                }
                GroupNode::Editor(group)
            }
            EditorGroupLayout::Split {
                vertical,
                ratio,
                first,
                second,
            } => GroupNode::Split {
                id: uuid::Uuid::new_v4(),
                vertical: *vertical,
                ratio: f32::from((*ratio).clamp(150, 850)) / 1000.0,
                bounds: Rc::new(Cell::new(Bounds::default())),
                view: Rc::new(RefCell::new(None)),
                first: Box::new(self.restore_group_node(first, active_group, window, cx)),
                second: Box::new(self.restore_group_node(second, active_group, window, cx)),
            },
        }
    }
}
