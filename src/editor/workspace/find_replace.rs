use super::*;

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
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let scope = self.workspace.search_scope;
        let tree = self.workspace.file_tree.clone();
        if matcher.is_empty() || (scope == WorkspaceSearchScope::Workspace && tree.is_none()) {
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
                    let Some(tree) = tree else { return };
                    search_workspace_files(&tree, &matcher, 200, &background).await
                }
                WorkspaceSearchScope::Document => {
                    let Ok((source, path, label)) = editor.update(cx, |editor, cx| {
                        let source = editor.current_document_source(cx);
                        let path = editor.file_path.clone().unwrap_or_default();
                        let label = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| {
                                cx.global::<I18nManager>()
                                    .strings()
                                    .workspace_current_document_label
                                    .clone()
                            });
                        (source, path, label)
                    }) else {
                        return;
                    };
                    background
                        .spawn(async move {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                search_document_source(&source, &matcher, &path, &label, 200)
                            }))
                            .unwrap_or_default()
                        })
                        .await
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
        if std::env::var("VELORA_SEARCH_JUMP_DEBUG").as_deref() == Ok("1") {
            eprintln!("[SEARCHJUMP] jump range={range:?}");
        }
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
        if std::env::var("VELORA_SEARCH_JUMP_DEBUG").as_deref() == Ok("1") {
            eprintln!(
                "[SEARCHJUMP] jump flags set, active={:?}",
                self.active_entity_id
            );
        }
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
            self.open_workspace_file(path, window, cx);
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
        self.open_workspace_file(path, window, cx);
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
        // 命中区间按当前文本算：这个区间要拿去跳转、拿去替换，拿搜索结果落地那一刻
        // 的快照算，用户中间打过的字会让它落到别的位置上。
        let source = self.current_document_source(cx);
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let from = self
            .workspace
            .document_active_range
            .as_ref()
            .map(|range| if reverse { range.start } else { range.end })
            .unwrap_or(if reverse { source.len() } else { 0 });
        let Some(range) =
            find_document_match_from(&source, &matcher, from, reverse).or_else(|| {
                find_document_match_from(
                    &source,
                    &matcher,
                    if reverse { source.len() } else { 0 },
                    reverse,
                )
            })
        else {
            return;
        };
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
        let source = self.current_document_source(cx);
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let replacement = self.workspace.replace_query.clone();
        let mut ranges = Vec::new();
        let mut absolute = 0usize;
        for raw_line in source.split_inclusive('\n') {
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            for found in matcher.find_in_line(line) {
                ranges.push(absolute + found.start..absolute + found.end);
            }
            absolute += raw_line.len();
        }
        let mut replaced = 0usize;
        for range in ranges.iter().rev() {
            self.apply_selection_snapshot_in_current_mode(
                &UndoSelectionSnapshot {
                    range: range.clone(),
                    reversed: false,
                },
                cx,
            );
            if self.replace_selected_block_text(&replacement, window, cx) {
                replaced += 1;
            }
        }
        self.workspace.document_active_range = None;
        replaced
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
        let Some(tree) = self.workspace.file_tree.clone() else {
            return 0;
        };
        let matcher = SearchMatcher::new(self.workspace.search_query.trim(), self.search_options());
        let replacement = self.workspace.replace_query.clone();
        let active_path = self.file_path.clone();

        let files = collect_workspace_files(&tree);
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
