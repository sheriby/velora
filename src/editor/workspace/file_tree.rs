use super::*;

impl WorkspaceTreeNode {
    pub(crate) fn kind_dir(&self) -> bool {
        matches!(self.kind, WorkspaceTreeKind::Directory(_))
    }

    pub(super) fn matches_workspace_selection(
        &self,
        selection: Option<&WorkspaceSelection>,
    ) -> bool {
        match (selection, &self.kind) {
            (Some(WorkspaceSelection::Directory(selected)), WorkspaceTreeKind::Directory(path)) => {
                selected == path
            }
            (
                Some(WorkspaceSelection::File(selected)),
                WorkspaceTreeKind::MarkdownFile(path)
                | WorkspaceTreeKind::CodeFile(path)
                | WorkspaceTreeKind::OtherFile(path),
            ) => selected == path,
            (Some(WorkspaceSelection::Outline(selected)), WorkspaceTreeKind::Heading { .. }) => {
                selected == &self.id
            }
            _ => false,
        }
    }

    pub(super) fn workspace_selection(&self) -> WorkspaceSelection {
        match &self.kind {
            WorkspaceTreeKind::Directory(path) => WorkspaceSelection::Directory(path.clone()),
            WorkspaceTreeKind::Heading { .. } => WorkspaceSelection::Outline(self.id.clone()),
            _ => WorkspaceSelection::File(tree_node_path(self).to_path_buf()),
        }
    }
}

pub(crate) fn find_workspace_node<'a>(
    nodes: &'a [WorkspaceTreeNode],
    id: &str,
) -> Option<&'a WorkspaceTreeNode> {
    nodes.iter().find_map(|node| {
        if node.id == id {
            Some(node)
        } else {
            find_workspace_node(&node.children, id)
        }
    })
}

pub(crate) fn find_workspace_node_mut<'a>(
    nodes: &'a mut [WorkspaceTreeNode],
    id: &str,
) -> Option<&'a mut WorkspaceTreeNode> {
    for node in nodes.iter_mut() {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_workspace_node_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
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

/// 内部目录既不进侧栏，也不进搜索与索引的文件清单。
pub(crate) const SKIPPED_DIR_NAMES: [&str; 3] = [".git", "node_modules", ".worktrees"];

fn is_skipped_dir(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| SKIPPED_DIR_NAMES.contains(&name))
}

fn is_build_dir(name: &std::ffi::OsStr) -> bool {
    matches!(name.to_str(), Some("target" | "dist"))
}

pub(super) fn workspace_entry_node(path: PathBuf, directory: bool) -> WorkspaceTreeNode {
    let kind = if directory {
        WorkspaceTreeKind::Directory(path.clone())
    } else if is_markdown_file(&path) {
        WorkspaceTreeKind::MarkdownFile(path.clone())
    } else if is_code_file(&path) {
        WorkspaceTreeKind::CodeFile(path.clone())
    } else {
        WorkspaceTreeKind::OtherFile(path.clone())
    };
    WorkspaceTreeNode {
        id: file_node_id(&path),
        label: file_label(&path),
        kind,
        children: Vec::new(),
        children_loaded: !directory,
        children_load_failed: false,
    }
}

fn workspace_path_visible(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        relative
            .components()
            .all(|component| !is_skipped_dir(component.as_os_str()))
    })
}

pub(super) struct WorkspacePathUpdate {
    pub(super) path: PathBuf,
    pub(super) node: Option<WorkspaceTreeNode>,
    pub(super) files: Vec<PathBuf>,
}

pub(super) fn read_workspace_path_update(
    root: &Path,
    path: PathBuf,
    update_index: bool,
) -> Result<WorkspacePathUpdate> {
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).with_context(|| format!("无法读取 {}", path.display())),
    };
    // 根路径保留原来的写法；文件名取磁盘实际大小写，避免仅改大小写时留下旧条目。
    let actual_path = if metadata
        .as_ref()
        .is_some_and(|metadata| metadata.is_file() || metadata.is_dir())
    {
        let actual = crate::file_url::canonical_path(&path);
        let actual_root = crate::file_url::canonical_path(root);
        actual
            .strip_prefix(&actual_root)
            .map(|relative| root.join(relative))
            .unwrap_or(path.clone())
    } else {
        path.clone()
    };
    let node = metadata
        .as_ref()
        .filter(|metadata| metadata.is_file() || metadata.is_dir())
        .filter(|_| workspace_path_visible(root, &actual_path))
        .map(|metadata| workspace_entry_node(actual_path.clone(), metadata.is_dir()));
    let indexed = update_index
        && workspace_path_visible(root, &actual_path)
        && actual_path.strip_prefix(root).is_ok_and(|relative| {
            relative
                .components()
                .all(|component| !is_build_dir(component.as_os_str()))
        });
    let files = if indexed && metadata.as_ref().is_some_and(|metadata| metadata.is_dir()) {
        let (files, skipped) = collect_workspace_files_on_disk_reporting_skips(&actual_path);
        if skipped > 0 {
            anyhow::bail!("更新 {} 时有 {skipped} 个条目无法读取", path.display());
        }
        files
    } else if indexed && metadata.as_ref().is_some_and(|metadata| metadata.is_file()) {
        vec![actual_path]
    } else {
        Vec::new()
    };
    Ok(WorkspacePathUpdate { path, node, files })
}

fn workspace_file_range(files: &[PathBuf], path: &Path) -> Range<usize> {
    let start = files.partition_point(|candidate| candidate.as_path() < path);
    let end = start
        + files
            .iter()
            .skip(start)
            .take_while(|candidate| candidate.starts_with(path))
            .count();
    start..end
}

fn apply_workspace_file_update(files: &mut Vec<PathBuf>, update: &WorkspacePathUpdate) {
    let range = workspace_file_range(files, &update.path);
    if files.get(range.clone()) != Some(update.files.as_slice()) {
        drop(files.splice(range, update.files.iter().cloned()));
        if update
            .files
            .iter()
            .any(|path| !path.starts_with(&update.path))
        {
            // 不区分大小写的文件系统上，旧名字仍可能读到改名后的文件。
            files.sort();
            files.dedup();
        }
    }
}

pub(super) fn apply_workspace_file_updates(
    files: &mut Vec<PathBuf>,
    updates: &[WorkspacePathUpdate],
) {
    let mut ranges: Vec<_> = updates
        .iter()
        .map(|update| workspace_file_range(files, &update.path))
        .collect();
    if ranges
        .iter()
        .zip(updates)
        .all(|(range, update)| files.get(range.clone()) == Some(update.files.as_slice()))
    {
        return;
    }
    let mut additions = Vec::new();
    for update in updates {
        apply_workspace_file_update(&mut additions, update);
    }
    ranges.sort_unstable_by_key(|range| range.start);
    let mut ranges = ranges
        .into_iter()
        .filter(|range| !range.is_empty())
        .peekable();
    let mut position = 0;
    files.retain(|_| {
        while ranges.peek().is_some_and(|range| range.end <= position) {
            drop(ranges.next());
        }
        let retain = ranges.peek().is_none_or(|range| position < range.start);
        position += 1;
        retain
    });
    // 一批变动只移动一次旧名单，避免逐个插入把大工作区变成 N×M 次搬运。
    let mut existing = std::mem::take(files).into_iter().peekable();
    files.reserve(existing.len() + additions.len());
    let mut additions = additions.into_iter().peekable();
    while let (Some(before), Some(after)) = (existing.peek(), additions.peek()) {
        match before.cmp(after) {
            std::cmp::Ordering::Less => files.extend(existing.next()),
            std::cmp::Ordering::Equal => {
                drop(existing.next());
                files.extend(additions.next());
            }
            std::cmp::Ordering::Greater => files.extend(additions.next()),
        }
    }
    files.extend(existing);
    files.extend(additions);
}

/// 首次只读根目录；即使只有三层，临时目录也可能包含数十万条目。
pub(crate) const WORKSPACE_SCAN_DEPTH: usize = 1;

/// 扫到默认层数：更深的目录留 `children_loaded: false`，展开时由
/// `load_workspace_dir_level` 补扫。
pub(crate) fn scan_workspace_dir(path: &Path, sort: TreeSortPreference) -> Result<WorkspaceTreeNode> {
    scan_workspace_dir_until(path, sort, WORKSPACE_SCAN_DEPTH)
}

/// 只扫一层：展开目录时用它补扫下一层。
pub(crate) fn scan_workspace_dir_level(
    path: &Path,
    sort: TreeSortPreference,
) -> Result<WorkspaceTreeNode> {
    scan_workspace_dir_until(path, sort, 1)
}

fn scan_workspace_dir_until(
    path: &Path,
    sort: TreeSortPreference,
    depth: usize,
) -> Result<WorkspaceTreeNode> {
    let mut children = Vec::new();
    for entry in
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?
    {
        let entry = entry?;
        let entry_path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if is_skipped_dir(&entry.file_name()) {
                continue;
            }
            // 构建产物可能有数十万项：目录可见，但只有明确展开后才读下一层。
            children.push(if depth > 1 && !is_build_dir(&entry.file_name()) {
                scan_workspace_dir_until(&entry_path, sort, depth - 1)?
            } else {
                workspace_entry_node(entry_path, true)
            });
        } else if file_type.is_file() {
            children.push(workspace_entry_node(entry_path, false));
        }
    }

    sort_workspace_children(&mut children, sort);

    Ok(WorkspaceTreeNode {
        id: file_node_id(path),
        label: file_label(path),
        kind: WorkspaceTreeKind::Directory(path.to_path_buf()),
        children,
        children_loaded: true,
        children_load_failed: false,
    })
}

/// 递归扫完整棵树的接口：留给需要完整结构的测试夹具与离线工具，侧栏路径不用它。
#[cfg(test)]
pub(crate) fn scan_workspace_dir_recursive(
    path: &Path,
    sort: TreeSortPreference,
) -> Result<WorkspaceTreeNode> {
    scan_workspace_dir_until(path, sort, usize::MAX)
}

pub(super) fn sort_workspace_children(children: &mut [WorkspaceTreeNode], sort: TreeSortPreference) {
    // 键每个节点只算一次（sort_by_cached_key）：按修改时间排序时比较器里
    // 现开两次 fs::metadata，约 2·n·log n 次系统调用（Windows 上 Defender
    // 还会放大）；标签的小写比较也从每次比较两份分配降到每节点一份。
    // Directories always group first; within a group the preference decides
    // the key (roadmap D2). 排序键里的变体只在同一种偏好下互比。
    children.sort_by_cached_key(|node| {
        let is_file = !matches!(node.kind, WorkspaceTreeKind::Directory(_));
        let key = match sort {
            TreeSortPreference::Name => TreeSortKey::Label(node.label.to_lowercase()),
            TreeSortPreference::ModifiedTime => {
                // Newest first; missing metadata sorts last（Reverse(None) 最大）。
                let modified = fs::metadata(tree_node_path(node))
                    .ok()
                    .and_then(|meta| meta.modified().ok());
                TreeSortKey::Time(std::cmp::Reverse(modified))
            }
            TreeSortPreference::Type => TreeSortKey::Type((
                tree_node_path(node)
                    .extension()
                    .map(|extension| extension.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                node.label.to_lowercase(),
            )),
        };
        (is_file, key)
    });
}

/// 排序键：同一次排序里只会出现同一种偏好的变体。
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum TreeSortKey {
    Label(String),
    Time(std::cmp::Reverse<Option<std::time::SystemTime>>),
    Type((String, String)),
}


/// 工作区里的所有文件（含只做文件名匹配的其它类型）：搜索 / 全部替换 / 快速切换 /
/// 索引用它，不再依赖侧栏那棵树——树只加载展开过的层，拿它当名单会让范围随展开状态
/// 漂移。文本类消费方自己过滤（`workspace_text_files` 只要 Markdown + 代码）。
///
/// 用 ripgrep 的 walker 并行走盘，但关掉 gitignore / 隐藏文件 / `.ignore` 规则：
/// 内部目录共用 `SKIPPED_DIR_NAMES`；构建目录虽在侧栏可展开，仍不进入全局名单，
/// 避免每次工作区刷新都遍历大量产物。结果排序后再返回，保证同一目录的顺序稳定。
///
/// 调用方负责在后台执行：~35 万文件的目录一次走盘要几秒。
///
/// 只要名单、不看跳过计数的调用方（现在只有测试夹具）用这个；产品路径走
/// [`collect_workspace_files_on_disk_reporting_skips`]，好把读不了的条目报给界面。
#[cfg(test)]
pub(crate) fn collect_workspace_files_on_disk(root: &Path) -> Vec<PathBuf> {
    collect_workspace_files_on_disk_reporting_skips(root).0
}

/// 同 [`collect_workspace_files_on_disk`]，另外报出走盘时**读不了、被跳过**的条目数。
///
/// 名单是搜索 / 全部替换 / 快速切换 / 索引的唯一输入：跳过的条目必须能让调用方
/// 看见（侧栏那条 `file_error`），否则用户只会以为「搜不到 = 没有」——需要忽略的
/// 地方不许静默吞错（AGENTS §1）。
pub(crate) fn collect_workspace_files_on_disk_reporting_skips(
    root: &Path,
) -> (Vec<PathBuf>, usize) {
    let files = std::sync::Mutex::new(Vec::new());
    let skipped = std::sync::atomic::AtomicUsize::new(0);
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        // 根目录自己不受跳过名单影响：工作区就叫 target 时也要能扫。
        .filter_entry(|entry| {
            entry.depth() == 0
                || (!is_skipped_dir(entry.file_name()) && !is_build_dir(entry.file_name()))
        })
        .build_parallel();
    walker.run(|| {
        let files = &files;
        let skipped = &skipped;
        Box::new(move |entry| {
            let Ok(entry) = entry else {
                // 单个条目读失败（权限、扫描中删除）不影响其余名单，但要计数上报。
                skipped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return ignore::WalkState::Continue;
            };
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
                if let Ok(mut files) = files.lock() {
                    files.push(entry.path().to_path_buf());
                }
            }
            ignore::WalkState::Continue
        })
    });
    let mut files = files
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    files.sort();
    (files, skipped.load(std::sync::atomic::Ordering::Relaxed))
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

