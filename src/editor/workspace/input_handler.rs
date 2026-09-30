use super::*;

// Only the focused workspace search fields register this handler; document
// blocks keep their own input handlers and IME state. The query and replace
// fields share the editor's handler, routed by which focus handle is active.
impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let kind = self.active_overlay_input(window);
        let text = self.input_text(kind);
        let start = search_utf16_to_utf8(text, range.start);
        let end = search_utf16_to_utf8(text, range.end).max(start);
        *actual_range = Some(search_utf8_to_utf16(text, start)..search_utf8_to_utf16(text, end));
        Some(text[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let kind = self.active_overlay_input(window);
        let text = self.input_text(kind).to_string();
        let range = self.input_selection(kind);
        Some(UTF16Selection {
            range: search_utf8_to_utf16(&text, range.start)..search_utf8_to_utf16(&text, range.end),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let kind = self.active_overlay_input(window);
        let text = self.input_text(kind).to_string();
        self.input_marked(kind).map(|range| {
            search_utf8_to_utf16(&text, range.start)..search_utf8_to_utf16(&text, range.end)
        })
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.active_overlay_input(window);
        let was_marked = self.input_marked(kind).is_some();
        match kind {
            OverlayInputKind::Query => {
                self.workspace.search_marked_range = None;
            }
            OverlayInputKind::Replace => {
                self.workspace.replace_marked_range = None;
            }
            OverlayInputKind::QuickOpen => {
                if let Some(state) = self.quick_open.as_mut() {
                    state.marked_range = None;
                }
            }
            OverlayInputKind::CommandPalette => {
                if let Some(state) = self.command_palette.as_mut() {
                    state.marked_range = None;
                }
            }
        }
        if was_marked {
            match kind {
                OverlayInputKind::Query => self.schedule_workspace_search(cx),
                OverlayInputKind::Replace => {}
                OverlayInputKind::QuickOpen => self.refresh_quick_open_results(cx),
                OverlayInputKind::CommandPalette => {}
            }
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = self.active_overlay_input(window);
        let query = self.input_text(kind).to_string();
        let range = range
            .map(|range| {
                search_utf16_to_utf8(&query, range.start)..search_utf16_to_utf8(&query, range.end)
            })
            .or_else(|| self.input_marked(kind))
            .unwrap_or_else(|| self.input_selection(kind));
        self.replace_overlay_input_text(kind, range, text, None, false, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = self.active_overlay_input(window);
        let query = self.input_text(kind).to_string();
        let range = range
            .map(|range| {
                search_utf16_to_utf8(&query, range.start)..search_utf16_to_utf8(&query, range.end)
            })
            .or_else(|| self.input_marked(kind))
            .unwrap_or_else(|| self.input_selection(kind));
        let selected = new_selected_range.map(|range| {
            search_utf16_to_utf8(new_text, range.start)..search_utf16_to_utf8(new_text, range.end)
        });
        self.replace_overlay_input_text(kind, range, new_text, selected, true, cx);
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let kind = self.active_overlay_input(window);
        Some(self.input_text(kind).encode_utf16().count())
    }
}

/// Flattens the markdown/code files of a workspace tree in display order.
pub(crate) fn collect_workspace_files(root: &WorkspaceTreeNode) -> Vec<PathBuf> {
    let mut files = Vec::new();
    fn visit(node: &WorkspaceTreeNode, files: &mut Vec<PathBuf>) {
        match &node.kind {
            WorkspaceTreeKind::Directory(_) => {
                for child in &node.children {
                    visit(child, files);
                }
            }
            WorkspaceTreeKind::MarkdownFile(path) | WorkspaceTreeKind::CodeFile(path) => {
                files.push(path.clone());
            }
            // Other files can't be opened, so they are never replacement
            // targets for bulk replace.
            WorkspaceTreeKind::OtherFile(_) => {}
            WorkspaceTreeKind::Heading { .. } => {}
        }
    }
    visit(root, &mut files);
    files
}

/// Number of matches in a whole source string.
pub(crate) fn count_matches_in_source(source: &str, matcher: &SearchMatcher) -> usize {
    let mut count = 0;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        count += matcher.find_in_line(line).len();
    }
    count
}

/// Rewrites every match in `source` with `replacement`.
pub(crate) fn replace_in_source(source: &str, matcher: &SearchMatcher, replacement: &str) -> String {
    if matcher.is_empty() {
        return source.to_string();
    }
    let mut updated = String::with_capacity(source.len());
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let trailing_newline = raw_line.len() - line.len();
        let mut line_cursor = 0usize;
        for range in matcher.find_in_line(line) {
            if range.start < line_cursor {
                continue;
            }
            updated.push_str(&line[line_cursor..range.start]);
            updated.push_str(replacement);
            line_cursor = range.end;
        }
        updated.push_str(&line[line_cursor..]);
        if trailing_newline > 0 {
            updated.push('\n');
        }
    }
    updated
}

/// Hover tooltip text for a tree node: relative path, size, and modified time
/// for files; just the path for directories (roadmap D7).
pub(crate) fn tree_node_tooltip(node: &WorkspaceTreeNode) -> String {
    let path = match &node.kind {
        WorkspaceTreeKind::Directory(path)
        | WorkspaceTreeKind::MarkdownFile(path)
        | WorkspaceTreeKind::CodeFile(path)
        | WorkspaceTreeKind::OtherFile(path) => path,
        WorkspaceTreeKind::Heading { .. } => return node.label.clone(),
    };
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return node.label.clone(),
    };
    if node.kind_dir() {
        return node.label.clone();
    }
    let size = metadata.len();
    let size_text = if size >= 1024 * 1024 {
        format!("{:.1} MiB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KiB", size as f64 / 1024.0)
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| {
            time.duration_since(std::time::UNIX_EPOCH).ok()
        })
        .map(|elapsed| {
            let seconds = elapsed.as_secs();
            chrono_like_date_string(seconds)
        })
        .unwrap_or_default();
    format!("{} · {} · {modified}", node.label, size_text)
}

/// Minimal local-date rendering from a unix timestamp (UTC date, good enough
/// for tooltips without pulling a time-zone database).
/// 文件历史等模块复用的 UTC 日期时间格式化（D7 内建历法算法）。
pub(crate) fn chrono_like_date_string_public(seconds: u64) -> String {
    chrono_like_date_string(seconds)
}

pub(crate) fn chrono_like_date_string(seconds: u64) -> String {
    let days = seconds / 86_400;
    // Civil-from-days algorithm (Howard Hinnant) for a UTC date.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let seconds_of_day = seconds % 86_400;
    let (hour, minute) = (seconds_of_day / 3600, (seconds_of_day % 3600) / 60);
    format!("{y:04}-{m:02}-{d:02} {hour:02}:{minute:02}")
}

pub(crate) fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

pub(crate) fn file_node_id(path: &Path) -> String {
    format!("file:{}", path.to_string_lossy())
}

pub(crate) fn stable_node_hash(id: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn clamp_workspace_panel_width(width: f32, viewport_width: f32) -> f32 {
    let maximum = (viewport_width - 320.0).clamp(180.0, 600.0);
    width.clamp(180.0, maximum)
}

pub(crate) fn prune_outline_state(workspace: &mut WorkspaceState, outline: &[WorkspaceTreeNode]) {
    let mut current_ids = HashSet::new();
    collect_node_ids(outline, &mut current_ids);
    workspace
        .expanded
        .retain(|id| !is_outline_node_id(id) || current_ids.contains(id));

    if matches!(
        &workspace.selected,
        Some(WorkspaceSelection::Outline(id)) if !current_ids.contains(id)
    ) {
        workspace.selected = None;
    }
}

pub(crate) fn collect_node_ids(nodes: &[WorkspaceTreeNode], ids: &mut HashSet<String>) {
    for node in nodes {
        ids.insert(node.id.clone());
        collect_node_ids(&node.children, ids);
    }
}

pub(crate) fn is_outline_node_id(id: &str) -> bool {
    id.starts_with("outline:")
}

/// Marks every heading node at or above `max_level` expanded so the outline
/// opens down to that level (level 2 = H1/H2 expanded, showing H3 leaves).
pub(crate) fn expand_outline_to_level(
    nodes: &[WorkspaceTreeNode],
    max_level: u8,
    expanded: &mut HashSet<String>,
) {
    for node in nodes {
        if let WorkspaceTreeKind::Heading { level, .. } = &node.kind {
            if *level <= max_level {
                expanded.insert(node.id.clone());
            }
        }
        expand_outline_to_level(&node.children, max_level, expanded);
    }
}

/// 把大纲树压平为 `[TOC]` 块用的条目列表（roadmap C2），保持文档顺序。
pub(crate) fn flatten_outline_entries(nodes: &[WorkspaceTreeNode]) -> Vec<TocEntry> {
    fn visit(nodes: &[WorkspaceTreeNode], entries: &mut Vec<TocEntry>) {
        for node in nodes {
            if let WorkspaceTreeKind::Heading { line, level } = node.kind {
                entries.push(TocEntry {
                    level,
                    title: node.label.clone(),
                    line,
                });
            }
            visit(&node.children, entries);
        }
    }
    let mut entries = Vec::new();
    visit(nodes, &mut entries);
    entries
}

pub(crate) fn build_outline_tree(markdown: &str) -> Vec<WorkspaceTreeNode> {
    let mut roots = Vec::new();
    let mut stack: Vec<(u8, Vec<usize>)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for (line_index, line) in markdown.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some((marker, len)) = fence {
            if is_closing_fence(trimmed, marker, len) {
                fence = None;
            }
            continue;
        }

        if let Some(next_fence) = opening_fence(trimmed) {
            fence = Some(next_fence);
            continue;
        }

        let Some((level, title)) = BlockKind::parse_atx_heading_line(line) else {
            continue;
        };

        while stack
            .last()
            .is_some_and(|(parent_level, _)| *parent_level >= level)
        {
            stack.pop();
        }

        let node = WorkspaceTreeNode {
            id: format!("outline:{line_index}"),
            label: title,
            kind: WorkspaceTreeKind::Heading {
                line: line_index,
                level,
            },
            children: Vec::new(),
        };

        let siblings = if let Some((_, parent_path)) = stack.last() {
            children_at_path_mut(&mut roots, parent_path)
        } else {
            &mut roots
        };
        siblings.push(node);

        let mut node_path = stack
            .last()
            .map(|(_, path)| path.clone())
            .unwrap_or_default();
        node_path.push(siblings.len() - 1);
        stack.push((level, node_path));
    }

    roots
}

pub(crate) fn children_at_path_mut<'a>(
    nodes: &'a mut Vec<WorkspaceTreeNode>,
    path: &[usize],
) -> &'a mut Vec<WorkspaceTreeNode> {
    let mut current = nodes;
    for &index in path {
        current = &mut current[index].children;
    }
    current
}

pub(crate) fn opening_fence(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    (len >= 3).then_some((marker, len))
}

pub(crate) fn is_closing_fence(trimmed: &str, marker: char, len: usize) -> bool {
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    count >= len && trimmed[count..].trim().is_empty()
}
