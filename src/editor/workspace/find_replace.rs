use super::*;
use crate::components::UndoCaptureKind;
use crate::editor::SourceTargetMapping;

impl Editor {
    pub(crate) fn search_options(&self) -> SearchOptions {
        SearchOptions {
            match_case: self.workspace.search_match_case,
            whole_word: self.workspace.search_whole_word,
            use_regex: self.workspace.search_use_regex,
            fuzzy: self.workspace.search_fuzzy,
        }
    }

    pub(crate) fn schedule_workspace_search(&mut self, cx: &mut Context<Self>) {
        self.workspace.search_generation = self.workspace.search_generation.wrapping_add(1);
        let generation = self.workspace.search_generation;
        self.workspace.search_active_index = None;
        self.workspace.document_active_range = None;
        self.workspace.document_active_index = None;
        self.workspace.document_matches = None;
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        // 模式编译失败：把引擎交回的原始诊断显示在搜索框下方，并**停止搜索**。
        // 旧行为是静默退化成字面量继续搜——用户以为在跑正则，实际搜的是另一回事。
        self.workspace.search_error = matcher.error_message().map(str::to_string);
        if self.workspace.search_error.is_some() {
            self.workspace.search_results.clear();
            self.workspace.search_pending = false;
            self.sync_document_search_highlights(cx);
            cx.notify();
            return;
        }
        let scope = self.workspace.search_scope;
        let files = self.workspace.files_on_disk.clone();
        // 名单还在走盘（换根 / watcher 刷新之后）：这不是「没有文件」，是「还没到」。
        // 保持「进行中」并让旧结果继续显示，名单落地时会重新调度搜索；拿空名单跑
        // 一轮会把面板先清空，正是要避免的那次闪烁。
        let list_pending =
            self.workspace.files_on_disk_root.is_none() && self.workspace.root.is_some();
        if matcher.is_empty() {
            self.workspace.search_results.clear();
            self.workspace.search_pending = false;
            self.sync_document_search_highlights(cx);
            cx.notify();
            return;
        }
        if scope == WorkspaceSearchScope::Workspace && files.is_empty() {
            if list_pending {
                self.workspace.search_pending = true;
                cx.notify();
                return;
            }
            self.workspace.search_results.clear();
            self.workspace.search_pending = false;
            self.sync_document_search_highlights(cx);
            cx.notify();
            return;
        }
        // 去抖窗口里保留上一次的结果，新结果落地后再整体替换：清空会让侧栏
        // 先闪成空白再恢复（用户报修，watcher 刷新文件树等任何重新调度都会触发）。
        self.workspace.search_pending = true;
        let editor = cx.entity().downgrade();
        let background = cx.background_executor().clone();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            background.timer(Duration::from_millis(120)).await;
            let current = editor
                .update(cx, |editor, _| {
                    editor.workspace.search_generation == generation
                })
                .unwrap_or(false);
            if !current {
                return;
            }
            let results = match scope {
                WorkspaceSearchScope::Workspace => {
                    let Some(root) = editor
                        .read_with(cx, |editor, _| editor.workspace.root.clone())
                        .ok()
                        .flatten()
                    else {
                        return;
                    };
                    search_workspace_files(&root, &files, &matcher, 200, &background).await
                }
                WorkspaceSearchScope::Document => {
                    // 文档范围的扫描不再绕后台线程：实测引擎扫 10 MiB 只要 2 毫秒，
                    // 而换来的是一件更要紧的事——结果列表、高亮、跳转、全部替换
                    // 从此读同一张命中表，四处不可能算出两样结果。
                    editor
                        .update(cx, |editor, cx| {
                            let Some(hits) = editor.document_matches(cx) else {
                                return Vec::new();
                            };
                            let source = editor.current_document_source(cx);
                            let (path, label) = editor.document_search_label(cx);
                            editor.project_document_hits(&hits, &source, &path, &label, 200)
                        })
                        .unwrap_or_default()
                }
            };
            let _ = editor.update(cx, |editor, cx| {
                if editor.workspace.search_generation == generation {
                    editor.workspace.search_results = results;
                    editor.workspace.search_pending = false;
                    editor.sync_document_search_highlights(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(crate) fn goto_cursor_location(
        &mut self,
        location: CursorLocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open_in_current = self.file_path.as_deref() == location.path.as_deref();
        if !open_in_current
            && let Some(path) = location.path.clone()
            && path.is_file()
        {
            self.open_workspace_file(path, window, cx);
        }
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range: location.range,
                reversed: false,
            },
            cx,
        );
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_center_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        cx.notify();
    }

    pub(crate) fn jump_to_document_search_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        // 刚打开的大文件只同步建了首块（512 行），其余还在后台续建。落点在
        // 未物化的部分时，选区找不到归属的投影块，会被钳进首块末尾——用户
        // 报修：第一次点击命中停在 512 行，再点才对。跳转前先把续建落地；
        // 已物化的文档这里是空操作。
        self.flush_pending_materialization(cx);
        self.push_cursor_location(cx);
        // 命中在折叠标题的章节里时先展开：块被折叠过滤不挂载，既画不出高亮
        // 也滚不过去（用户报修）。
        if self.unfold_sections_covering_source_range(&range, cx) {
            self.fold_state_version = self.fold_state_version.wrapping_add(1);
        }
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range,
                reversed: false,
            },
            cx,
        );
        // 跳转目标的滚动锚点不能是表格单元格：cell 不注册进文档块树，且随
        // 表格重建而亡——锚它则滚动系统永远查不到坐标（用户报修：表格里的
        // 搜索命中点了没反应）。apply 落在 cell 上时这里校正为宿主表格块；
        // 视图切换等恢复路径不受影响（它们需要 cell 锚点继续编辑）。
        if let Some(anchor) = self.active_entity_id
            && self.document.block_entity_by_id(anchor).is_none()
            && let Some(binding) = self.table_cell_binding(anchor)
        {
            let host = binding.table_block.entity_id();
            if self.active_entity_id == Some(anchor) {
                self.active_entity_id = Some(host);
            }
            if self.pending_focus == Some(anchor) {
                self.pending_focus = Some(host);
            }
        }
        // 搜索跳转不能把焦点从查询框抢进正文：那样继续敲字会直接改写文档
        // （用户报修）。本帧块仍拿到焦点，apply_pending_scroll_into_view 靠
        // 它算滚动目标；同帧稍后 apply_pending_workspace_search_focus 把焦点
        // 交还查询框。滚动目标另走 active_entity_id（见 ensure_focused_
        // caret_visible），不依赖焦点。
        if self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Search {
            self.workspace.search_focus_pending = true;
        }
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_center_into_view = true;
        self.pending_scroll_recheck_after_layout = true;
        // 活动命中变了（跳转/循环）：重算文档内高亮，让用户看得出当前
        // 停在哪一个命中上（用户报修：来回跳毫无视觉反馈）。
        self.sync_document_search_highlights(cx);
        cx.notify();
    }

    pub(crate) fn open_document_find(&mut self, cx: &mut Context<Self>) {
        // Document search scans blocks, so a partially imported huge document
        // must finish importing first (roadmap G8).
        self.flush_pending_materialization(cx);
        self.workspace.is_open = true;
        self.workspace.active_tab = WorkspaceTab::Search;
        self.workspace.search_scope = WorkspaceSearchScope::Document;
        self.workspace.search_selected_range = 0..self.workspace.search_query.len();
        self.workspace.search_marked_range = None;
        self.workspace.search_focus_pending = true;
        self.schedule_workspace_search(cx);
        cx.notify();
    }

    /// Opens the workspace file named `target` (with `.md` appended when
    /// missing); creates it at the workspace root when no match exists
    /// (roadmap C3).
    pub(crate) fn open_wikilink(
        &mut self,
        target: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let needle = target.to_lowercase();
        let found = self.workspace_text_files().into_iter().find(|path| {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_lowercase());
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase());
            stem.as_deref() == Some(needle.as_str())
                || name.as_deref() == Some(needle.as_str())
        });
        if let Some(path) = found {
            // 链接目标按预览标签打开（与搜索结果同口径）：连着点几个链接不堆标签栏。
            self.open_workspace_file_in_mode(path, WorkspaceOpenMode::Preview, window, cx);
            return;
        }
        // Create `<target>.md` at the workspace root.
        let path = self
            .workspace
            .root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            .join(format!("{target}.md"));
        if !path.exists() {
            if let Err(error) = fs::write(&path, format!("# {target}\n")) {
                self.workspace.file_error = Some(error.to_string());
                cx.notify();
                return;
            }
            self.refresh_workspace_tree(cx);
        }
        self.open_workspace_file_in_mode(path, WorkspaceOpenMode::Preview, window, cx);
    }

    /// `#tag` 点击：打开搜索面板并以工作区范围列出同类（roadmap C4）。
    pub(crate) fn open_tag_search(&mut self, query: String, cx: &mut Context<Self>) {
        self.workspace.is_open = true;
        self.workspace.active_tab = WorkspaceTab::Search;
        self.workspace.search_scope = WorkspaceSearchScope::Workspace;
        self.workspace.search_query = query;
        self.workspace.search_selected_range = 0..self.workspace.search_query.len();
        self.workspace.search_marked_range = None;
        self.workspace.search_active_index = None;
        self.workspace.document_active_range = None;
        self.schedule_workspace_search(cx);
        cx.notify();
    }

    pub(crate) fn on_find_in_document(
        &mut self,
        _: &crate::components::FindInDocument,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_document_find(cx);
    }

    pub(crate) fn on_find_next_match(
        &mut self,
        _: &crate::components::FindNextMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.advance_search_match(false, window, cx);
    }

    pub(crate) fn on_find_previous_match(
        &mut self,
        _: &crate::components::FindPreviousMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.advance_search_match(true, window, cx);
    }

    pub(crate) fn refresh_document_find_after_edit(&mut self, cx: &mut Context<Self>) {
        if self.workspace.is_open
            && self.workspace.active_tab == WorkspaceTab::Search
            && self.workspace.search_scope == WorkspaceSearchScope::Document
            && !self.workspace.search_query.trim().is_empty()
        {
            self.schedule_workspace_search(cx);
        }
    }

    pub(crate) fn apply_pending_workspace_search_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.search_focus_pending {
            let focus = self
                .workspace
                .search_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            window.focus(&focus);
            self.workspace.search_focus_pending = false;
        }
    }

    /// Enter/「下一个/上一个」按钮的统一入口：工作区范围在结果列表里
    /// 循环（此前是空操作，来回跳毫无反应——用户报修），文档范围在当前
    /// 文档内循环。
    pub(crate) fn advance_search_match(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.search_scope == WorkspaceSearchScope::Workspace {
            self.cycle_workspace_search_hit(reverse, window, cx);
        } else {
            self.find_next_document_match(reverse, cx);
        }
    }

    /// 工作区范围：按 search_active_index 在结果列表里循环点击。
    fn cycle_workspace_search_hit(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.workspace.search_results.len();
        if count == 0 {
            return;
        }
        let next = match self.workspace.search_active_index {
            Some(index) => {
                let step = if reverse { -1isize } else { 1isize };
                (index as isize + step).rem_euclid(count as isize) as usize
            }
            None => {
                if reverse {
                    count - 1
                } else {
                    0
                }
            }
        };
        self.open_search_hit(next, window, cx);
    }

    pub(crate) fn find_next_document_match(&mut self, reverse: bool, cx: &mut Context<Self>) {
        // 命中表按缓冲区版本缓存，内容一改就重算，所以这里的区间永远是按当前
        // 文本算的——拿搜索结果落地那一刻的快照算，用户中间打过的字会让它落到
        // 别的位置上（这是被替换掉的旧实现里真实存在过的错法）。
        let count = self
            .document_matches(cx)
            .as_ref()
            .map(|hits| hits.len())
            .unwrap_or(0);
        if count == 0 {
            self.workspace.document_active_index = None;
            self.workspace.document_active_range = None;
            self.sync_document_search_highlights(cx);
            return;
        }
        // 导航是取索引：O(1)，且与文档大小无关。旧实现每次按键都要重扫整篇文档
        // 并把所有区间收集进一个 Vec 再线性找下一个（10 MiB 中文查询实测 61 毫秒
        // 一次按键，无后续命中要回绕时等于扫两遍）。
        let Some(index) = self.advance_document_match_index(reverse) else {
            return;
        };
        let Some(range) = self.document_match_at(index) else {
            return;
        };
        // 侧栏那 200 行的选中态按区间找；命中超出上限时它就是 None，
        // 这不是错——document_find_navigates_beyond_sidebar_result_limit 钉的正是
        // 「列表之外也要能继续跳」。
        self.workspace.search_active_index = self
            .workspace
            .search_results
            .iter()
            .position(|hit| hit.source_range.as_ref() == Some(&range));
        self.workspace.document_active_range = Some(range.clone());
        self.jump_to_document_search_range(range, cx);
    }

    /// Replaces the currently active document match (selected via a jump).
    pub(crate) fn replace_active_document_match(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(range) = self.workspace.document_active_range.clone() else {
            return false;
        };
        let replacement = self.workspace.replace_query.clone();
        self.apply_selection_snapshot_in_current_mode(
            &UndoSelectionSnapshot {
                range,
                reversed: false,
            },
            cx,
        );
        self.replace_selected_block_text(&replacement, window, cx)
    }

    /// Replaces every current match in the open document, last-first so the
    /// earlier byte offsets stay valid while editing.
    pub(crate) fn replace_all_document_matches(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        // 命中来自那张表——和结果列表、高亮、跳转同一份数据，
        // 「替换计数」与「列表说有几个」因此不可能对不上。
        let replacement = self.workspace.replace_query.clone();
        let table = self.document_matches(cx);
        let source = self.current_document_source(cx);
        let mut hits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        for hit in table.iter().flat_map(|hits| hits.iter()) {
            let matched = match source.get(hit.range.clone()) {
                Some(text) => text.to_string(),
                // 命中表是按这一版缓冲区文本算的，取不到只可能是越界的零宽命中；
                // 宁可少换一处，也不要换到错的位置上。
                None => continue,
            };
            hits.push((hit.range.clone(), matched));
        }
        if hits.is_empty() {
            self.workspace.document_active_range = None;
            return 0;
        }

        // 整批共用一次映射构建（旧实现每个命中付两次整篇重建），先把每条命中
        // 换算成「块 + 可见区间」；换算不过去的一律留给缓冲区字节写回。
        let mappings = self.build_source_target_mappings(cx);
        let mut plan: Vec<Option<(Entity<crate::components::Block>, std::ops::Range<usize>)>> =
            Vec::with_capacity(hits.len());
        for (range, matched) in &hits {
            plan.push(Self::block_target_for_hit(&mappings, range, matched, cx));
        }

        // 从后往前处理：改一条只会影响它**之后**的偏移，剩下的都在它之前，
        // 所以偏移一直有效。跨块命中走缓冲区写回，那条会重建整篇投影——块实体
        // 从此不能再拿旧的换算结果，剩下的必须现算，否则写进一块已脱离文档的
        // 实体上就是白写（不报错、也不替换）。
        let mut replaced = 0usize;
        let mut fresh_mappings: Option<Vec<SourceTargetMapping>> = None;
        let mut buffer_capture_started = false;
        for index in (0..hits.len()).rev() {
            let (range, matched) = (&hits[index].0, &hits[index].1);
            let target = match fresh_mappings.as_ref() {
                Some(mappings) => {
                    Self::block_target_for_hit(mappings, range, matched, cx)
                }
                None => plan[index].clone(),
            };
            if let Some((entity, visible)) = target {
                entity.update(cx, |block, cx| {
                    let utf16 = block.range_to_utf16(&visible);
                    use gpui::EntityInputHandler;
                    block.replace_text_in_range(Some(utf16), &replacement, window, cx);
                });
                replaced += 1;
                continue;
            }
            // 跨块命中（以及映射换算落不准的）直接按缓冲区字节区间替换：
            // 这一段字节就是搜到的原文，位置由构造保证，不存在换算漂移。
            if !buffer_capture_started {
                // 整批的缓冲区写回共用一次撤销捕获（与替换单条跨块命中同一档），
                // 撤销时这一批一起退回，不会退一半留下半替的文档。
                self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                buffer_capture_started = true;
            }
            self.write_back_cross_block_source_edit(range.clone(), &replacement, cx);
            replaced += 1;
            fresh_mappings = Some(self.build_source_target_mappings(cx));
        }
        if buffer_capture_started {
            // 这批缓冲区写回收进同一次撤销捕获：不 finalize 的话撤销栈上什么都不会
            // 留下，「全部替换」按一次撤销就退不回去（与粘贴、表格编辑同一套路子：
            // 谁开捕获谁收尾）。
            self.finalize_pending_undo_capture(cx);
        }
        self.workspace.document_active_range = None;
        replaced
    }

    /// 把一条命中换算成「哪一块的哪一段可见区间」；这块装不下它，或换算回来的
    /// 文本对不上搜到的原文，就交回 `None`（调用方退回缓冲区字节写回）。
    ///
    /// 核对不能省：映射按规范前缀记账，非规范前缀的块（`>引用`）上会漂——
    /// 宁可少换一处，也绝不替换到错误的位置、绝不许替换计数虚报。
    fn block_target_for_hit(
        mappings: &[SourceTargetMapping],
        range: &std::ops::Range<usize>,
        matched: &str,
        cx: &App,
    ) -> Option<(Entity<crate::components::Block>, std::ops::Range<usize>)> {
        let mapping = mappings.iter().find(|mapping| {
            mapping.full_source_range.start <= range.start
                && range.end <= mapping.full_source_range.end
        })?;
        let local_start = range.start - mapping.full_source_range.start;
        let local_end = range.end - mapping.full_source_range.start;
        let max_content = mapping.source_to_content.len().saturating_sub(1);
        let content_start = mapping.source_to_content[local_start.min(max_content)];
        let content_end = mapping.source_to_content[local_end.min(max_content)];
        let visible = mapping
            .entity
            .read(cx)
            .markdown_range_to_current_range(content_start..content_end);
        let block_text = mapping.entity.read(cx).display_text().to_string();
        if visible.end > block_text.len() || block_text[visible.clone()] != *matched {
            return None;
        }
        Some((mapping.entity.clone(), visible))
    }

    /// Runs `replace_text_in_range` on the block that currently holds the
    /// selection (set by `apply_selection_snapshot_in_current_mode`). The
    /// target block may be outside the visible window, so it is resolved
    /// through the full-tree location map instead of the visible snapshot.
    pub(crate) fn replace_selected_block_text(
        &mut self,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(entity_id) = self.active_entity_id else {
            return false;
        };
        let Some(block) = self.document.block_entity_at_location(entity_id, cx) else {
            return false;
        };
        block.update(cx, |block, cx| {
            block.replace_text_in_range(None, replacement, window, cx);
        });
        true
    }

    /// Replaces matches across every file in the workspace tree. Open tabs get
    /// their cached markdown rewritten (and marked dirty); the active document
    /// is replaced live through the block editor so undo still works.
    pub(crate) fn replace_all_workspace_matches(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> usize {
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let replacement = self.workspace.replace_query.clone();
        let active_path = self.file_path.clone();

        let files = self.text_files_on_disk();
        let mut total = 0usize;
        for path in files {
            if Some(&path) == active_path.as_ref() {
                // The active document goes through the live editor path so the
                // change is undoable and stays in sync with the block model.
                total += self.replace_all_document_matches(window, cx);
                continue;
            }
            let open_tab_index = self
                .workspace
                .open_documents
                .iter()
                .position(|tab| tab.path == path);
            let source = match open_tab_index {
                Some(index) if self.workspace.open_documents[index].dirty => {
                    self.workspace.open_documents[index].markdown.clone()
                }
                _ => match crate::editor::encoding::read_document_string(&path) {
                    Ok(source) => source,
                    Err(_) => continue,
                },
            };
            let updated = replace_in_source(&source, &matcher, &replacement);
            if updated == source {
                continue;
            }
            total += count_matches_in_source(&source, &matcher);
            if fs::write(&path, &updated).is_ok() {
                if let Some(index) = open_tab_index {
                    let tab = &mut self.workspace.open_documents[index];
                    tab.markdown = updated;
                    tab.dirty = true;
                    tab.file_version = crate::editor::persistence::file_content_version(&tab.markdown);
                }
            } else {
                self.workspace.file_error = Some(format!("无法写入 {}", path.display()));
            }
        }
        if total > 0 {
            self.refresh_workspace_tree(cx);
        }
        total
    }
}
