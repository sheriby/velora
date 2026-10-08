use super::*;

impl WorkspaceTreeNode {
    pub(crate) fn kind_dir(&self) -> bool {
        matches!(self.kind, WorkspaceTreeKind::Directory(_))
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

/// 侧栏扫描与搜索走盘共用的跳过名单：仓库 / 构建产物目录既不进侧栏，也不进搜索
/// 与索引的文件清单。两边必须逐条一致，否则会出现「侧栏看得见、搜索搜不到」。
pub(crate) const SKIPPED_DIR_NAMES: [&str; 5] = [".git", "target", "node_modules", ".worktrees", "dist"];

fn is_skipped_dir(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| SKIPPED_DIR_NAMES.contains(&name))
}

/// 换根时先扫几层：根算第 1 层，再深一层展开时才扫（懒加载）。
///
/// 以前这里递归扫完整个树：目录没有层数上限时（打开单个文件，隐含根就是它所在
/// 目录）一次扫描要走上万次 readdir——实测系统临时根目录 26 万条目要 30 秒 CPU、
/// macOS 的 ~/Library 更多；而这一步在侧栏展开时是自动发生的。三层是取舍：常
/// 用的笔记目录整棵都在三层以内，扫完就是完整视图；超大目录也只付三层。
pub(crate) const WORKSPACE_SCAN_DEPTH: usize = 3;

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
            children.push(if depth > 1 {
                scan_workspace_dir_until(&entry_path, sort, depth - 1)?
            } else {
                WorkspaceTreeNode {
                    id: file_node_id(&entry_path),
                    label: file_label(&entry_path),
                    kind: WorkspaceTreeKind::Directory(entry_path),
                    children: Vec::new(),
                    children_loaded: false,
                }
            });
        } else if file_type.is_file() && is_markdown_file(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::MarkdownFile(entry_path),
                children: Vec::new(),
                children_loaded: true,
            });
        } else if file_type.is_file() && is_code_file(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::CodeFile(entry_path),
                children: Vec::new(),
                children_loaded: true,
            });
        } else if file_type.is_file() {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::OtherFile(entry_path),
                children: Vec::new(),
                children_loaded: true,
            });
        }
    }

    sort_workspace_children(&mut children, sort);

    Ok(WorkspaceTreeNode {
        id: file_node_id(path),
        label: file_label(path),
        kind: WorkspaceTreeKind::Directory(path.to_path_buf()),
        children,
        children_loaded: true,
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

fn sort_workspace_children(children: &mut [WorkspaceTreeNode], sort: TreeSortPreference) {
    children.sort_by(|left, right| {
        let left_dir = matches!(left.kind, WorkspaceTreeKind::Directory(_));
        let right_dir = matches!(right.kind, WorkspaceTreeKind::Directory(_));
        // Directories always group first; within a group the preference
        // decides the key (roadmap D2).
        right_dir.cmp(&left_dir).then_with(|| match sort {
            TreeSortPreference::Name => compare_labels(&left.label, &right.label),
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
                left_ext
                    .cmp(&right_ext)
                    .then_with(|| compare_labels(&left.label, &right.label))
            }
        })
    });
}

/// 忽略大小写的标签比较。`to_lowercase()` 每次比较要分配两份字符串，几万条的
/// 子目录就是几百万次分配（扫描耗时里占大头）。
fn compare_labels(left: &str, right: &str) -> std::cmp::Ordering {
    left.chars()
        .flat_map(char::to_lowercase)
        .cmp(right.chars().flat_map(char::to_lowercase))
}

/// 工作区里的所有文件（含只做文件名匹配的其它类型）：搜索 / 全部替换 / 快速切换 /
/// 索引用它，不再依赖侧栏那棵树——树只加载展开过的层，拿它当名单会让范围随展开状态
/// 漂移。文本类消费方自己过滤（`workspace_text_files` 只要 Markdown + 代码）。
///
/// 用 ripgrep 的 walker 并行走盘，但关掉 gitignore / 隐藏文件 / `.ignore` 规则：
/// 侧栏扫描不读这些文件，两边的文件集合必须逐条相同（`SKIPPED_DIR_NAMES` 是唯一的
/// 过滤来源）。结果排序后再返回，保证同一棵目录搜出同一份顺序。
///
/// 调用方负责在后台执行：~35 万文件的目录一次走盘要几秒。
pub(crate) fn collect_workspace_files_on_disk(root: &Path) -> Vec<PathBuf> {
    let files = std::sync::Mutex::new(Vec::new());
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        // 根目录自己不受跳过名单影响：工作区就叫 target 时也要能扫。
        .filter_entry(|entry| entry.depth() == 0 || !is_skipped_dir(entry.file_name()))
        .build_parallel();
    walker.run(|| {
        let files = &files;
        Box::new(move |entry| {
            let Ok(entry) = entry else {
                // 单个条目读失败（权限、扫描中删除）不影响其余名单。
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
    files
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

