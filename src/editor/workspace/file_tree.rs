use super::*;

impl WorkspaceTreeNode {
    pub(crate) fn kind_dir(&self) -> bool {
        matches!(self.kind, WorkspaceTreeKind::Directory(_))
    }
}

pub(crate) fn tree_node_path(node: &WorkspaceTreeNode) -> &Path {
    match &node.kind {
        WorkspaceTreeKind::Directory(path)
        | WorkspaceTreeKind::MarkdownFile(path)
        | WorkspaceTreeKind::CodeFile(path)
        | WorkspaceTreeKind::OtherFile(path) => path,
        WorkspaceTreeKind::Heading { .. } => Path::new(""),
    }
}

pub(crate) fn scan_workspace_dir(path: &Path, sort: TreeSortPreference) -> Result<WorkspaceTreeNode> {
    let mut children = Vec::new();
    for entry in
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?
    {
        let entry = entry?;
        let entry_path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if matches!(
                name.to_str(),
                Some(".git" | "target" | "node_modules" | ".worktrees" | "dist")
            ) {
                continue;
            }
            children.push(scan_workspace_dir(&entry_path, sort)?);
        } else if file_type.is_file() && is_markdown_file(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::MarkdownFile(entry_path),
                children: Vec::new(),
            });
        } else if file_type.is_file() && is_code_file(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::CodeFile(entry_path),
                children: Vec::new(),
            });
        } else if file_type.is_file() {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::OtherFile(entry_path),
                children: Vec::new(),
            });
        }
    }

    children.sort_by(|left, right| {
        let left_dir = matches!(left.kind, WorkspaceTreeKind::Directory(_));
        let right_dir = matches!(right.kind, WorkspaceTreeKind::Directory(_));
        // Directories always group first; within a group the preference
        // decides the key (roadmap D2).
        right_dir.cmp(&left_dir).then_with(|| match sort {
            TreeSortPreference::Name => left
                .label
                .to_lowercase()
                .cmp(&right.label.to_lowercase()),
            TreeSortPreference::ModifiedTime => {
                let left_time = fs::metadata(tree_node_path(left))
                    .ok()
                    .and_then(|meta| meta.modified().ok());
                let right_time = fs::metadata(tree_node_path(right))
                    .ok()
                    .and_then(|meta| meta.modified().ok());
                // Newest first; missing metadata sorts last.
                right_time.cmp(&left_time)
            }
            TreeSortPreference::Type => {
                let left_ext = tree_node_path(left)
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let right_ext = tree_node_path(right)
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                left_ext.cmp(&right_ext).then_with(|| {
                    left.label.to_lowercase().cmp(&right.label.to_lowercase())
                })
            }
        })
    });

    Ok(WorkspaceTreeNode {
        id: file_node_id(path),
        label: file_label(path),
        kind: WorkspaceTreeKind::Directory(path.to_path_buf()),
        children,
    })
}

pub(crate) fn search_utf16_to_utf8(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, ch) in text.char_indices() {
        if utf16 >= offset || utf16 + ch.len_utf16() > offset {
            return byte;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}

pub(crate) fn search_utf8_to_utf16(text: &str, offset: usize) -> usize {
    let mut byte = offset.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    text[..byte].encode_utf16().count()
}

