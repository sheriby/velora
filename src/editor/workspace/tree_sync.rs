use super::*;
use crate::editor::{SourceTargetMapping, ViewMode};

impl Editor {
    /// Opens a welcome-page recent entry: folders replace the working set,
    /// files open as a tab in this window.
    /// Recomputes in-document search highlight ranges (roadmap B2): clears
    /// the previous blocks, then maps every match through the source→content
    /// mappings onto the owning block. 工作区范围（所有文件）的命中跳转过去
    /// 之后同样要看到高亮——用户报修过跳转后无高亮。
    pub(crate) fn sync_document_search_highlights(&mut self, cx: &mut Context<Self>) {
        let previous = std::mem::take(&mut self.search_highlighted_blocks);
        for entity in &previous {
            let _ = entity.update(cx, |block, _| {
                block.search_highlight_ranges.clear();
                block.search_active_range = None;
            });
        }

        let query = self.workspace.search_query.trim().to_string();
        let active = self.workspace.is_open
            && self.workspace.active_tab == WorkspaceTab::Search
            && !query.is_empty();
        if !active {
            cx.notify();
            return;
        }

        let matcher = SearchMatcher::new(&query, self.search_options());
        // 活动命中（循环跳转/点击结果选中的那个）单独标记，让用户在多个
        // 命中之间能看出当前在哪一个。
        let active_range = self.workspace.document_active_range.clone();
        let mut highlighted = Vec::new();

        match self.view_mode {
            ViewMode::Rendered => {
                // 命中先在缓冲区里按字节找（不把整篇复制出来），然后只为**有命中的
                // 那一根块**重建它自己的映射。没命中的块连换算都不需要，整篇重拼
                // source mapping 是白付的 O(文档)——查询没改、只是重算一遍高亮也要付。
                for root in self.document.root_blocks().to_vec() {
                    let Some(span) = root.read(cx).record.source_span.clone() else {
                        continue;
                    };
                    let text = self.buffer.slice(span.clone());
                    let hits: Vec<Range<usize>> = matcher
                        .find_in_line(&text)
                        .into_iter()
                        .map(|found| span.start + found.start..span.start + found.end)
                        .collect();
                    if hits.is_empty() {
                        continue;
                    }
                    let mut mappings = Vec::new();
                    let mut block_ranges = std::collections::HashMap::new();
                    self.source_mapping_builds
                        .set(self.source_mapping_builds.get() + 1);
                    self.push_root_source_mappings(&root, &mut mappings, &mut block_ranges, cx);
                    for mapping in &mappings {
                        let Some((ranges, active_local)) =
                            Self::search_ranges_for_hits(mapping, &hits, &active_range, cx)
                        else {
                            continue;
                        };
                        let entity = mapping.entity.clone();
                        entity.update(cx, |block, _| {
                            block.search_highlight_ranges = ranges;
                            block.search_active_range = active_local;
                        });
                        highlighted.push(entity);
                    }
                }
            }
            ViewMode::Source => {
                // 源码模式的块是按行切的投影，位置不挂在 `source_span` 上，仍按整篇
                // 走查算出每块的源码区间。
                let source = self.current_document_source(cx);
                let mappings = self.build_source_target_mappings(cx);
                for mapping in &mappings {
                    let Some(text) = source.get(mapping.full_source_range.clone()) else {
                        continue;
                    };
                    let hits: Vec<Range<usize>> = matcher
                        .find_in_line(text)
                        .into_iter()
                        .map(|found| {
                            mapping.full_source_range.start + found.start
                                ..mapping.full_source_range.start + found.end
                        })
                        .collect();
                    let Some((ranges, active_local)) =
                        Self::search_ranges_for_hits(mapping, &hits, &active_range, cx)
                    else {
                        continue;
                    };
                    let entity = mapping.entity.clone();
                    entity.update(cx, |block, _| {
                        block.search_highlight_ranges = ranges;
                        block.search_active_range = active_local;
                    });
                    highlighted.push(entity);
                }
            }
        }

        self.search_highlighted_blocks = highlighted;
        cx.notify();
    }

    /// 把绝对的命中字节区间换算成这一块上的显示区间；没有落在这块里的命中就 `None`。
    fn search_ranges_for_hits(
        mapping: &SourceTargetMapping,
        hits: &[Range<usize>],
        active_range: &Option<Range<usize>>,
        cx: &App,
    ) -> Option<(Vec<Range<usize>>, Option<Range<usize>>)> {
        let block_start = mapping.full_source_range.start;
        let block_end = mapping.full_source_range.end;
        let mut ranges = Vec::new();
        for hit in hits {
            if hit.start < block_start || hit.end > block_end {
                continue;
            }
            let local_start = hit.start - block_start;
            let local_end = hit.end - block_start;
            if local_end >= mapping.source_to_content.len() {
                continue;
            }
            let content_start = mapping.source_to_content[local_start];
            let content_end = mapping.source_to_content[local_end];
            if content_end <= content_start {
                continue;
            }
            let range = mapping
                .entity
                .read(cx)
                .markdown_range_to_current_range(content_start..content_end);
            if !range.is_empty() {
                ranges.push(range);
            }
        }
        if ranges.is_empty() {
            return None;
        }
        let active_local = active_range.as_ref().and_then(|active| {
            if block_start > active.start || active.end > block_end {
                return None;
            }
            let local = |offset: usize| {
                let index = offset - block_start;
                mapping.source_to_content[index.min(mapping.source_to_content.len() - 1)]
            };
            let content = local(active.start)..local(active.end);
            let converted = mapping
                .entity
                .read(cx)
                .markdown_range_to_current_range(content);
            (!converted.is_empty()).then_some(converted)
        });
        Some((ranges, active_local))
    }

    /// Cycles the file tree sort order (roadmap D2) and rescans.
    pub(crate) fn on_cycle_tree_sort(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = match crate::config::EditorSettings::tree_sort(cx) {
            TreeSortPreference::Name => TreeSortPreference::ModifiedTime,
            TreeSortPreference::ModifiedTime => TreeSortPreference::Type,
            TreeSortPreference::Type => TreeSortPreference::Name,
        };
        crate::config::EditorSettings::set_tree_sort(cx, next);
        self.refresh_workspace_tree(cx);
    }

    /// 标题折叠（roadmap C7）：折叠标题之后的块隐藏，直到同级或更高
    /// 级标题出现。顺带刷新每个标题的 `foldable`（其后方是否有章节内容），
    /// 供标题行内的折叠 chevron 决定是否显示。
    /// 跳到源码行并选中该行（roadmap C2 的 `[TOC]` 条目点击）。
    pub(crate) fn jump_to_source_line(&mut self, line: usize, cx: &mut Context<Self>) {
        // 行号换算只有一条路：缓冲区里的第 line 行（0 基）。它说的就是文件。
        let range = self.buffer.line_range(line);
        if !range.is_empty() {
            self.jump_to_document_search_range(range, cx);
        }
    }

    pub(crate) fn apply_heading_fold_filter(
        &self,
        all: Vec<crate::editor::tree::VisibleBlock>,
        cx: &mut Context<Self>,
    ) -> Vec<crate::editor::tree::VisibleBlock> {
        for (index, visible) in all.iter().enumerate() {
            // P4b 后这里只在行计划重建时运行，但仍是 O(文档)：先做最廉价
            // 的类型判断，[TOC] 全文 trim 比较只可能命中 Paragraph。
            let kind = visible.entity.read(cx).kind();
            if !matches!(kind, BlockKind::Paragraph) {
                let level = match kind {
                    BlockKind::Heading { level } => level,
                    _ => continue,
                };
                let has_section = all.get(index + 1).is_some_and(|next| {
                    match next.entity.read(cx).kind() {
                        BlockKind::Heading { level: next_level } => next_level > level,
                        _ => true,
                    }
                });
                visible
                    .entity
                    .update(cx, |block, _cx| block.foldable = has_section);
                continue;
            }
            let (is_toc, had_toc) = {
                let block = visible.entity.read(cx);
                (
                    block.display_text().trim().eq_ignore_ascii_case("[toc]"),
                    !block.toc_entries.is_empty(),
                )
            };
            if is_toc {
                let entries = self.workspace.toc_entries.clone();
                visible
                    .entity
                    .update(cx, |block, _cx| block.toc_entries = entries);
            } else if had_toc {
                visible
                    .entity
                    .update(cx, |block, _cx| block.toc_entries.clear());
            }
            let level = match kind {
                BlockKind::Heading { level } => level,
                _ => continue,
            };
            let has_section = all.get(index + 1).is_some_and(|next| {
                match next.entity.read(cx).kind() {
                    BlockKind::Heading { level: next_level } => next_level > level,
                    _ => true,
                }
            });
            visible
                .entity
                .update(cx, |block, _cx| block.foldable = has_section);
        }
        let mut filtered = Vec::with_capacity(all.len());
        let mut hide_below_level: Option<u8> = None;
        for visible in all {
            let block = visible.entity.read(cx);
            match block.kind() {
                BlockKind::Heading { level } => {
                    if let Some(hide) = hide_below_level
                        && level <= hide
                    {
                        hide_below_level = None;
                    }
                    if block.folded {
                        hide_below_level = Some(level);
                    }
                    filtered.push(visible);
                }
                _ => {
                    if hide_below_level.is_none() {
                        filtered.push(visible);
                    }
                }
            }
        }
        filtered
    }
    /// Outline-follows-scroll (roadmap C5): while the Outline tab is visible,
    /// select the deepest heading at or above the topmost visible block.
    /// Cheap per frame: block bounds are cached by the previous layout, a block's
    /// source position is its own `source_span`, and the line number is the
    /// buffer's—no document-wide work left on this path.
    pub(crate) fn sync_outline_follow_scroll(
        &mut self,
        viewport_top: Pixels,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.is_open || self.workspace.active_tab != WorkspaceTab::Outline {
            return;
        }
        let offset = f32::from(self.scroll_handle.offset().y);
        if !self.last_outline_follow_offset.is_nan()
            && (offset - self.last_outline_follow_offset).abs() < 2.0
        {
            return;
        }
        self.last_outline_follow_offset = offset;

        // First block whose body extends below the viewport top band.
        let cutoff = viewport_top + px(48.0);
        let mut target_source_start: Option<usize> = None;
        for visible in self.document.visible_blocks().to_vec() {
            let Some(bounds) = visible.entity.read(cx).last_bounds else {
                continue;
            };
            if bounds.bottom() > cutoff {
                // 这一块在缓冲区里从哪个字节开始，问它自己的区间；行号问缓冲区。
                target_source_start = self
                    .block_source_range(visible.entity.entity_id(), cx)
                    .map(|range| range.start);
                break;
            }
        }
        let Some(source_start) = target_source_start else {
            return;
        };
        let line = self.buffer.line_of(source_start);

        // Preorder walk visits headings in ascending source line order, so the
        // last heading at or above the target is the deepest enclosing one.
        let mut current: Option<String> = None;
        {
            let tree = &self.workspace.outline_tree;
            fn visit(nodes: &[WorkspaceTreeNode], line: usize, current: &mut Option<String>) {
                for node in nodes {
                    if let WorkspaceTreeKind::Heading {
                        line: heading_line, ..
                    } = &node.kind
                    {
                        if *heading_line <= line {
                            *current = Some(node.id.clone());
                        }
                    }
                    visit(&node.children, line, current);
                }
            }
            visit(tree, line, &mut current);
        }
        if let Some(id) = current
            && self.workspace.selected != Some(WorkspaceSelection::Outline(id.clone()))
        {
            self.workspace.selected = Some(WorkspaceSelection::Outline(id));
            cx.notify();
        }
    }

    /// Persists the open-tab set for session restore (roadmap A4). The JSON
    pub(crate) fn sync_workspace_file_tree(&mut self, cx: &mut Context<Self>) {
        self.sync_workspace_file_tree_inner(false, cx);
    }

    /// 扫描工作区目录并更新文件树。扫描在后台线程执行（roadmap D9），
    /// 超大目录不再阻塞首帧；结果按代数校验，过期扫描直接丢弃。
    pub(crate) fn sync_workspace_file_tree_inner(&mut self, force: bool, cx: &mut Context<Self>) {
        let next_root = self
            .workspace
            .root
            .clone()
            .or_else(|| self.workspace_root_for_current_file());
        // 该根已有结果（树或错误）或扫描在途：不重复发起扫描，
        // 否则渲染期每帧都会重启扫描（roadmap D9）。
        if !force
            && self.workspace.root == next_root
            && self.workspace.tree_scan_root == next_root
        {
            self.follow_active_document_in_workspace_tree();
            return;
        }

        self.workspace.root = next_root.clone();
        self.clear_workspace_file_error();

        let Some(root) = next_root else {
            self.workspace.file_tree = None;
            self.workspace.selected = None;
            self.workspace.tree_scan_root = None;
            return;
        };

        // Validate the root path
        if root.as_os_str().is_empty() {
            self.workspace.file_error = Some("Invalid workspace path: empty path".to_string());
            self.workspace.file_tree = None;
            self.workspace.selected = None;
            self.workspace.tree_scan_root = None;
            return;
        }

        let tree_sort = crate::config::EditorSettings::tree_sort(cx);
        self.workspace.tree_scan_root = Some(root.clone());
        self.workspace.tree_scan_generation = self.workspace.tree_scan_generation.wrapping_add(1);
        let generation = self.workspace.tree_scan_generation;
        let editor = cx.entity().downgrade();
        let scan_root = root.clone();
        let scan = cx.background_spawn(async move { scan_workspace_dir(&scan_root, tree_sort) });
        // Dropping the previous task cancels a scan that is no longer relevant.
        self.workspace.tree_scan_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let result = scan.await;
                editor
                    .update(cx, |editor, cx| {
                        if editor.workspace.tree_scan_generation != generation {
                            return;
                        }
                        match result {
                            Ok(tree) => {
                                editor.workspace.expanded.insert(tree.id.clone());
                                editor.workspace.file_tree = Some(tree);
                                editor.follow_active_document_in_workspace_tree();
                                // 扫描期间发起的工作区搜索此时才有文件列表可用。
                                if editor.workspace.active_tab == WorkspaceTab::Search
                                    && !editor.workspace.search_query.is_empty()
                                {
                                    editor.schedule_workspace_search(cx);
                                }
                                // 反链/标签索引：换根后第一次树落地时全量重建，
                                // 之后由 watcher 单文件增量维持。
                                if let Some(root) = editor.workspace.root.clone() {
                                    let files = editor.workspace_text_files();
                                    editor
                                        .workspace_link_index
                                        .ensure_built_for_root(&root, files, cx);
                                }
                            }
                            Err(err) => {
                                editor.workspace.file_error = Some(err.to_string());
                            }
                        }
                        cx.notify();
                    })
                    .ok();
            },
        ));
    }

    /// 侧栏树选中项跟随活动文件——但只在用户没在树上做出别的选择时。
    /// 右键目录/工作区根之后，菜单动作（新建、粘贴、重命名、删除…）都按
    /// 点击时的选择取目标，所以每帧的「跟随活动文件」不能把目录选中顶掉
    /// （用户报修：右键 drafts 新建文件落到了活动文件旁边）。
    pub(crate) fn follow_active_document_in_workspace_tree(&mut self) {
        if !matches!(
            self.workspace.selected,
            None | Some(WorkspaceSelection::File(_))
        ) {
            return;
        }
        self.workspace.selected = self
            .file_path
            .as_ref()
            .map(|path| WorkspaceSelection::File(path.clone()));
    }

    /// 命中所在的块被折叠标题盖住时，把所有盖住它的折叠标题展开。
    /// 折叠逻辑见 `apply_heading_fold_filter`：标题折叠后隐藏后续块，直到
    /// 同级或更高级标题；这里按同一顺序维护一个折叠栈，栈里的标题就是
    /// 盖住目标块的那些。返回是否真的展开了。
    pub(crate) fn unfold_sections_covering_source_range(
        &mut self,
        range: &Range<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(entity_id) = self.block_id_at_source_offset(range.start, cx) else {
            return false;
        };
        let Some(target_index) = self.document.visible_index_for_entity_id(entity_id) else {
            return false;
        };
        let visible = self.document.visible_blocks().to_vec();
        let mut covering: Vec<(u8, EntityId)> = Vec::new();
        for visible_block in visible.iter().take(target_index) {
            let block = visible_block.entity.read(cx);
            if let BlockKind::Heading { level } = block.kind() {
                while covering
                    .last()
                    .is_some_and(|(hide_below, _)| level <= *hide_below)
                {
                    covering.pop();
                }
                if block.folded {
                    covering.push((level, visible_block.entity.entity_id()));
                }
            }
        }
        let mut unfolded = false;
        for (_, heading_id) in covering {
            let Some(heading) = self.document.block_entity_by_id(heading_id) else {
                continue;
            };
            heading.update(cx, |block, cx| {
                if block.folded {
                    block.folded = false;
                    unfolded = true;
                    cx.notify();
                }
            });
        }
        unfolded
    }

    pub(crate) fn sync_workspace_outline(&mut self, _cx: &mut Context<Self>) {
        // 这条短路每帧都会走（侧栏一开着就渲染），所以比较必须零拷贝：
        // `buffer.text()` 每次都要复制整篇，10 MiB 文档上就是一帧一次大搬运。
        let unchanged = self
            .workspace
            .outline_source
            .as_deref()
            .is_some_and(|source| self.buffer.matches_text(source));
        if unchanged {
            return;
        }

        let source = self.buffer.text();
        let outline = build_outline_tree(&source);
        prune_outline_state(&mut self.workspace, &outline);
        // Expand headings down to H3 by default so the outline is usable
        // without clicking through every level; users can still collapse.
        expand_outline_to_level(&outline, 2, &mut self.workspace.expanded);
        self.workspace.toc_entries = flatten_outline_entries(&outline);
        self.toc_state_version = self.toc_state_version.wrapping_add(1);
        self.workspace.outline_tree = outline;
        self.workspace.outline_source = Some(source);
    }
}
