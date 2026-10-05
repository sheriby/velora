use super::*;
use crate::editor::source_mapping::{clip_hit_to_span, hit_overlaps};
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

        // 命中读那张文档命中表：与结果列表、跳转、全部替换同一份数据。
        // 旧实现是在这里再扫一遍（逐根块取切片喂引擎），于是「列表说有这个命中」
        // 和「高亮画在哪儿」是两次独立计算。
        let all_hits: Vec<Range<usize>> = self
            .document_matches(cx)
            .iter()
            .flat_map(|hits| hits.iter())
            .map(|hit| hit.range.clone())
            .collect();
        // 活动命中（循环跳转/点击结果选中的那个）单独标记，让用户在多个
        // 命中之间能看出当前在哪一个。
        let active_range = self.workspace.document_active_range.clone();
        let mut highlighted = Vec::new();

        match self.view_mode {
            ViewMode::Rendered => {
                // 只为**与本块有交集的那根块**重建它自己的映射：没沾到命中的块连换算
                // 都不需要，整篇重拼 source mapping 是白付的 O(文档)。
                // 判据是「有交集」而不是「被包住」——跨块命中要落到它盖住的每一根块上。
                for root in self.document.root_blocks().to_vec() {
                    let Some(span) = self.document.source_span_of(root.entity_id()) else {
                        continue;
                    };
                    let hits: Vec<Range<usize>> = all_hits
                        .iter()
                        .filter(|range| hit_overlaps(range, &span))
                        .cloned()
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
                let mappings = self.build_source_target_mappings(cx);
                for mapping in &mappings {
                    let hits: Vec<Range<usize>> = all_hits
                        .iter()
                        .filter(|range| hit_overlaps(range, &mapping.full_source_range))
                        .cloned()
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
    ///
    /// 跨块命中在这里**按块裁段**：一块只拿到属于它那截，另一端落在别的块上的部分
    /// 由那一根块负责。旧写法是「命中必须整条被这块包住」，于是跨块命中一块都不画。
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
            let Some(clipped) = clip_hit_to_span(hit, block_start, block_end) else {
                continue;
            };
            let local_start = clipped.start - block_start;
            let local_end = clipped.end - block_start;
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
        let active_local = active_range
            .as_ref()
            .and_then(|active| clip_hit_to_span(active, block_start, block_end))
            .and_then(|clipped| {
                let local = |offset: usize| {
                    let index = offset - block_start;
                    mapping.source_to_content[index.min(mapping.source_to_content.len() - 1)]
                };
                let content = local(clipped.start)..local(clipped.end);
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

    /// 折叠过滤 + `[TOC]` 条目落账，返回**活下来的可见下标**（升序）。
    ///
    /// 块的类型、层级、是不是 `[TOC]`、手上有没有目录条目，全部读可见列表那一趟
    /// 记下的元数据（`DocumentTree::row_spacing_at`），所以这两遍扫不再逐块读实体：
    /// 10 MiB 一次按键的行计划重建里，实体读取从「每个可见块两次」降到
    /// 「每个标题一次」（chevron 的 `foldable` 与折叠状态 `folded` 住在块上）。
    /// 元数据是文本形状，靠「文本变了就重跑同步」保新鲜——这条前提由
    /// `typing_a_toc_marker_fills_its_entries_on_that_frame` 钉住。
    pub(crate) fn apply_heading_fold_filter(&mut self, cx: &mut Context<Self>) -> Vec<u32> {
        let all = self.document.visible_blocks().len();
        for index in 0..all {
            let meta = self.document.row_spacing_at(index);
            if let Some(level) = meta.heading_level {
                let has_section = index + 1 < all
                    && match self.document.row_spacing_at(index + 1).heading_level {
                        Some(next_level) => next_level > level,
                        None => true,
                    };
                self.document.visible_blocks()[index]
                    .entity
                    .update(cx, |block, _cx| block.foldable = has_section);
                continue;
            }
            if !meta.is_toc && !meta.had_toc {
                continue;
            }
            self.count_row_plan_block_read();
            let entity = self.document.visible_blocks()[index].entity.clone();
            if meta.is_toc {
                // `[TOC]` 就是大纲的一个读者：它出现的那一帧要把清单算出来（同步
                // 自己会先比一次「有没有什么变了」，没变时这里几乎不花钱），
                // 否则刚打完 `[TOC]` 的那一帧会拿到上一份、甚至空的条目。
                self.sync_workspace_outline(cx);
                let entries = self.workspace.toc_entries.clone();
                entity.update(cx, |block, _cx| block.toc_entries = entries);
            } else {
                entity.update(cx, |block, _cx| block.toc_entries.clear());
            }
            // 条目刚被写上或清掉，快照里那条 `had_toc` 跟着更新（下一趟过滤
            // 才知道这条还需不需要动）。
            self.document.refresh_row_spacing_for(entity.entity_id(), cx);
        }
        let mut filtered = Vec::with_capacity(all);
        let mut hide_below_level: Option<u8> = None;
        for index in 0..all {
            let meta = self.document.row_spacing_at(index);
            match meta.heading_level {
                Some(level) => {
                    if let Some(hide) = hide_below_level
                        && level <= hide
                    {
                        hide_below_level = None;
                    }
                    self.count_row_plan_block_read();
                    if self.document.visible_blocks()[index]
                        .entity
                        .read(cx)
                        .folded
                    {
                        hide_below_level = Some(level);
                    }
                    filtered.push(index as u32);
                }
                None => {
                    if hide_below_level.is_none() {
                        filtered.push(index as u32);
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
        // 两端各查一次：跨块命中的尾段可能落在另一章节里，而那章节正好是折叠的——
        // 只看起点就会「跳过去了，但尾巴还折着」。零宽命中没有「尾字节」，只查起点。
        let mut tail_offsets = vec![range.start];
        if !range.is_empty() {
            tail_offsets.push(range.end - 1);
        }
        let mut covering: Vec<(u8, EntityId)> = Vec::new();
        for offset in tail_offsets {
            let Some(entity_id) = self.block_id_at_source_offset(offset, cx) else {
                continue;
            };
            for item in self.folded_headings_above(entity_id, cx) {
                if !covering.iter().any(|(_, id)| *id == item.1) {
                    covering.push(item);
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

    /// 按折叠栈算出「盖住这一块」的那些折叠标题，从文档序头部扫到这块的位置。
    /// 返回 `(标题层级, 标题块 id)`，由外层到内层。
    fn folded_headings_above(&self, entity_id: EntityId, cx: &App) -> Vec<(u8, EntityId)> {
        let Some(target_index) = self.document.visible_index_for_entity_id(entity_id) else {
            return Vec::new();
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
        covering
    }

    /// 记一笔「这段字节里的区间接缝被重新分过」。写回层按区间拆块/合块时调它：
    /// 块自己的字节可以一个字没动，但它现在指着的是一段不同的字节，下一次大纲同步
    /// 必须把落在里面的块重扫，不然就会留着上一世的标题（差分测试抓到过：拆块把
    /// `# 甲一` 拆成 `# ` 与 `甲一`，按缓存拼出来的大纲还认得那个标题）。
    pub(crate) fn note_outline_dirty_region(&mut self, range: Range<usize>) {
        if range.start >= range.end {
            return;
        }
        self.workspace.outline_dirty = Some(match self.workspace.outline_dirty.take() {
            Some(seen) => seen.start.min(range.start)..seen.end.max(range.end),
            None => range,
        });
    }

    /// 这次编辑有没有碰到这根块的字节。没碰到的块，内容一个字节都没变，
    /// 上次算好的大纲摘要照用（至多平移行号）。
    fn region_touches(region: &Range<usize>, span: &Range<usize>) -> bool {
        span.start < region.end && region.start < span.end
    }

    /// 文档大纲：侧栏「大纲」页签与正文 `[TOC]` 的那份标题树。
    ///
    /// 「有没有必要重算」这个判断必须便宜：以前是拿整篇文本比较（10 MiB 一次大
    /// 搬运）再把整篇按行重扫一遍（实测一次按键 105ms / 58.5 万行）。现在改问缓冲区
    /// 「哪些字节被改过」，只重扫改动落到的那几根块——一根块的行数是局部量，与文档
    /// 多大无关。
    pub(crate) fn sync_workspace_outline(&mut self, _cx: &mut Context<Self>) {
        // 懒导入还没接完：文档本身还不完整，这一段一帧都不必动（旧实现靠「整篇
        // 文本没变」短路，于是一边续建一边留着半份大纲，`[TOC]` 直到第一次编辑
        // 才补全）。续建结束时根块数与上次同步对不上，那时一次建全。
        // 注意顺序：这里必须在取走脏区间之前返回，否则导入期间用户改的那一段字节
        // 会被当成「已经处理过」。
        if self.document.pending_tail().is_some() || self.document.pending_source().is_some() {
            return;
        }
        let dirty = match (
            self.buffer.take_dirty_region(),
            self.workspace.outline_dirty.take(),
        ) {
            (Some(edits), Some(repartitioned)) => Some(
                edits.start.min(repartitioned.start)..edits.end.max(repartitioned.end),
            ),
            (edits, repartitioned) => edits.or(repartitioned),
        };
        let roots = self.document.root_blocks().to_vec();
        if dirty.is_none()
            && !self.workspace.outline_stale
            && roots.len() == self.workspace.outline_root_count
        {
            return;
        }
        self.workspace.outline_root_count = roots.len();
        self.workspace.outline_stale = false;

        let started = std::time::Instant::now();
        // 每根块的第一行在整篇的第几行：一次批量问缓冲区。逐根块问是
        // O(根块数 × 文本块数)——10 MiB 那份实测把一次按键拖到 6.3 秒。
        let spans: Vec<Option<Range<usize>>> = roots
            .iter()
            .map(|root| self.document.source_span_of(root.entity_id()))
            .collect();
        let mut order = (0..roots.len())
            .filter_map(|index| spans[index].as_ref().map(|span| (span.start, index)))
            .collect::<Vec<_>>();
        order.sort_unstable();
        let answers = self.buffer.lines_and_line_starts(
            &order.iter().map(|(start, _)| *start).collect::<Vec<_>>(),
        );
        let total = self.buffer.byte_len();
        let mut first_lines: Vec<Option<usize>> = vec![None; roots.len()];
        for ((start, index), (line, line_start)) in order.iter().zip(answers) {
            // 段内相对行号要能对上整篇行号，所以起点必须正落在行首；区间还得在缓冲区内。
            if *start == line_start
                && spans[*index]
                    .as_ref()
                    .is_some_and(|span| span.end <= total)
            {
                first_lines[*index] = Some(line);
            }
        }

        let mut segments: HashMap<(EntityId, OutlineFence), OutlineSegment> =
            HashMap::with_capacity(roots.len());
        let mut rescanned_lines = 0usize;
        let mut changed = false;
        // 有一根块带不出可用的区间（懒导入还没接上的尾段）：只能整篇重扫。
        let mut rescan_everything = false;
        // 围栏的状态要跨块传下去。源码视图按行切片，一根 ``` 能正好落在两片接缝上，
        // 于是后一片是从围栏**里面**开始的；以前这里认不出来，只有「这片结尾还在
        // 围栏里 → 整篇重扫」一条退回，所以一份 10 MiB 的代码文档里跨接缝的围栏把整条
        // 增量路作废了（实测每帧一次大纲同步 142ms、重扫 585499 行 = 全文；见闸门
        // `one_mib_code_document_with_a_fence_on_the_seam_scans_one_chunk` 的成对数字）。
        let mut fence: OutlineFence = None;
        // 按文档顺序记下每块用的是哪份摘要（键含围栏状态），拼树时照这个顺序取。
        let mut walk: Vec<(EntityId, OutlineFence)> = Vec::with_capacity(roots.len());
        for (index, root) in roots.iter().enumerate() {
            let id = root.entity_id();
            let (Some(span), Some(first_line)) = (spans[index].clone(), first_lines[index]) else {
                rescan_everything = true;
                break;
            };
            let touched = dirty
                .as_ref()
                .is_some_and(|dirty| Self::region_touches(dirty, &span));
            // 缓存键是「这块 + 走进这块时的围栏状态」：同样的字节，围栏外扫出来一片
            // 标题，围栏里一个都没有，两份摘要不能混用。
            let key = (id, fence);
            walk.push(key);
            let cached = self.workspace.outline_segments.remove(&key);
            match cached {
                // 这块的字节一个都没动、段的字节数也没变：摘要照用，至多把行号平移
                // 到现在的位置。字节数变了说明接缝被重新分过（拆块、合块），那段
                // 字节已经不是这块的了。
                Some(cached) if !touched && cached.byte_len == span.end - span.start => {
                    if cached.first_line == first_line {
                        fence = cached.fence_out;
                        segments.insert(key, cached);
                    } else {
                        let cached = cached.shifted_to(first_line);
                        fence = cached.fence_out;
                        changed = true;
                        segments.insert(key, cached);
                    }
                }
                // 改动落在这一块里（或者它是刚换上去的新块、又或者上面的编辑把围栏
                // 状态推到它身上来了）：只重扫它自己那几行。
                cached => {
                    let segment =
                        outline_segment(first_line, &self.buffer.slice(span), fence);
                    rescanned_lines += segment.lines;
                    changed |= cached.is_none_or(|cached| !cached.same_content_as(&segment));
                    fence = segment.fence_out;
                    segments.insert(key, segment);
                }
            }
        }
        // 整篇重扫这条退回路径不缓存任何东西：下一帧重新按块算。
        self.workspace.outline_segments = if rescan_everything {
            HashMap::new()
        } else {
            segments
        };

        if rescan_everything {
            // 增量这条路对这个文档不成立（有块带不出可用的区间）：退回整篇扫一遍，
            // 缓存留空，下一帧重新按块算。
            self.outline_full_rescans
                .set(self.outline_full_rescans.get() + 1);
            let source = self.buffer.text();
            rescanned_lines = self.buffer.line_count();
            self.install_outline(build_outline_tree(&source));
            self.outline_rebuilds.set(self.outline_rebuilds.get() + 1);
        } else if changed {
            let mut headings: Vec<OutlineHeading> = Vec::new();
            for key in &walk {
                if let Some(segment) = self.workspace.outline_segments.get(key) {
                    headings.extend(segment.headings.iter().cloned());
                }
            }
            // 只有标题文字变了（层级与行号一个没动）就原地换标签：整树重拼在
            // 10 MiB 文档上是一次按键 130ms，而「在标题里打字」正是每键都来的
            // 那种改动。层级或行号动了（加标题、上方多/少了一行）才重拼。
            if !self.relabel_outline_in_place(&headings) {
                self.install_outline(nest_outline_headings(&headings));
                self.outline_rebuilds.set(self.outline_rebuilds.get() + 1);
            }
        }
        self.outline_lines_scanned
            .set(self.outline_lines_scanned.get() + rescanned_lines as u64);
        self.outline_nanos.set(
            self.outline_nanos.get()
                + started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
        );
    }

    /// 标题集合「同层级、同行号，只有文字变了」时原地改标签，返回是否改成功。
    /// 树的节点顺序与标题清单的顺序一致（`nest_outline_headings` 保序），所以
    /// 两边并排走一遍就能核对；不一致（增删标题、行号平移）返回 false，由调用方
    /// 退回整树重拼。原地改标签不动物件 id、`expanded`、选中态——行号没动，
    /// 这些状态本来就还指着同一批节点。
    fn relabel_outline_in_place(&mut self, headings: &[OutlineHeading]) -> bool {
        fn walk(
            nodes: &mut [WorkspaceTreeNode],
            headings: &[OutlineHeading],
            cursor: &mut usize,
        ) -> bool {
            for node in nodes.iter_mut() {
                if let WorkspaceTreeKind::Heading { line, level } = node.kind {
                    let Some(heading) = headings.get(*cursor) else {
                        return false;
                    };
                    if heading.level != level || heading.line != line {
                        return false;
                    }
                    if node.label != heading.label {
                        node.label = heading.label.clone();
                    }
                    *cursor += 1;
                }
                if !walk(&mut node.children, headings, cursor) {
                    return false;
                }
            }
            true
        }

        let mut cursor = 0usize;
        let mut tree = std::mem::take(&mut self.workspace.outline_tree);
        let same = walk(&mut tree, headings, &mut cursor) && cursor == headings.len();
        self.workspace.outline_tree = tree;
        if !same {
            return false;
        }
        for (entry, heading) in self.workspace.toc_entries.iter_mut().zip(headings) {
            if entry.title != heading.label {
                entry.title = heading.label.clone();
            }
            entry.line = heading.line;
            entry.level = heading.level;
        }
        // `toc_state_version` 是给行计划的折叠过滤下的命令：下一帧把新条目推给
        // 正文里的 `[TOC]` 块。没有 `[TOC]` 块时推进它只是白白下架整张行计划
        // （10 MiB 一次重排 ~42ms，而在标题里打字是每键都来的），所以只在真的有
        // 读者时才推。`is_toc` 自己的翻面走 `refresh_row_spacing_for` 的版本。
        if self.document.has_toc_reader() {
            self.toc_state_version = self.toc_state_version.wrapping_add(1);
        }
        true
    }

    fn install_outline(&mut self, outline: Vec<WorkspaceTreeNode>) {
        prune_outline_state(&mut self.workspace, &outline);
        // Expand headings down to H3 by default so the outline is usable
        // without clicking through every level; users can still collapse.
        expand_outline_to_level(&outline, 2, &mut self.workspace.expanded);
        self.workspace.toc_entries = flatten_outline_entries(&outline);
        self.toc_state_version = self.toc_state_version.wrapping_add(1);
        self.workspace.outline_tree = outline;
    }
}
