use super::*;

impl Editor {
    pub(crate) fn toggle_workspace_node(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.workspace.expanded.remove(id) {
            self.workspace.expanded.insert(id.to_string());
        }
        cx.notify();
    }

    /// Creates a `name copy.ext` / `name copy 2.ext` duplicate of the selected
    /// file beside it (roadmap D6).
    pub(crate) fn duplicate_selected_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.selected_workspace_path() else {
            return;
        };
        if source.is_dir() {
            return;
        }
        let Ok(contents) = fs::read(&source) else {
            return;
        };
        let parent = source.parent().unwrap_or(Path::new(""));
        let candidate = unique_workspace_copy_path(parent, &source);
        if let Err(error) = fs::write(&candidate, contents) {
            self.workspace.file_error = Some(error.to_string());
            cx.notify();
            return;
        }
        self.refresh_workspace_tree(cx);
        cx.notify();
        let _ = window;
    }

    /// 是否仍是空白欢迎态（未打开文件、未编辑、无标签）；用于启动窗口让位
    /// 给 Finder/`open` 的文件事件（roadmap G5）。
    pub(crate) fn is_pristine_startup_window(&self) -> bool {
        self.show_welcome
            && self.file_path.is_none()
            && !self.document_dirty
            && self.workspace_open_document_paths().is_empty()
    }

    /// 已打开文档的路径集合（崩溃恢复合并用，roadmap E10）。
    pub(crate) fn workspace_open_document_paths(&self) -> Vec<PathBuf> {
        self.workspace
            .open_documents
            .iter()
            .map(|tab| tab.path.clone())
            .collect()
    }

    /// 崩溃恢复合并（roadmap E10）：把恢复快照的未保存内容并入已打开的会话标签，
    /// 避免同一文件既出现在会话标签又弹出恢复窗口。快照 id 转移给该标签，保存后
    /// 由既有清理逻辑删除。返回 true 表示已合并。
    pub(crate) fn merge_recovery_snapshot(
        &mut self,
        path: &Path,
        markdown: &str,
        recovery_id: uuid::Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self
            .workspace
            .open_documents
            .iter()
            .any(|tab| tab.path == path)
        {
            return false;
        }
        if self.file_path.as_deref() != Some(path) {
            self.open_workspace_file(path.to_path_buf(), window, cx);
        }
        if self.file_path.as_deref() != Some(path) {
            return false;
        }
        self.replace_document_from_markdown(markdown.to_string(), Some(path.to_path_buf()), cx);
        self.recovery_id = recovery_id;
        self.mark_dirty(cx);
        if let Some(tab) = self
            .workspace
            .open_documents
            .iter_mut()
            .find(|tab| tab.path == path)
        {
            tab.markdown = markdown.to_string();
            tab.dirty = true;
            tab.recovery_id = recovery_id;
        }
        cx.notify();
        true
    }

    /// 测试用：读取会话标签的 (dirty, recovery_id, markdown)。
    #[cfg(test)]
    pub(crate) fn workspace_tab_state_for_test(
        &self,
        path: &Path,
    ) -> Option<(bool, uuid::Uuid, String)> {
        self.workspace
            .open_documents
            .iter()
            .find(|tab| tab.path == path)
            .map(|tab| (tab.dirty, tab.recovery_id, tab.markdown.clone()))
    }

    /// 测试用：按路径设置树选中项（等价于点击该节点）。
    #[cfg(test)]
    pub(crate) fn select_workspace_path_for_test(
        &mut self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.workspace.selected = Some(if path.is_dir() {
            WorkspaceSelection::Directory(path)
        } else {
            WorkspaceSelection::File(path)
        });
        cx.notify();
    }

    /// 树右键「复制」：记录源文件并写入系统剪贴板（roadmap D6）。
    pub(crate) fn copy_selected_workspace_file(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.selected_workspace_path() else {
            return;
        };
        if source.is_dir() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            source.to_string_lossy().into_owned(),
        ));
        self.tree_clipboard = Some(source);
        cx.notify();
    }

    /// 树右键「粘贴」：把已复制的文件或剪贴板图片落到目标目录（roadmap D6）。
    pub(crate) fn paste_into_workspace_tree(&mut self, cx: &mut Context<Self>) {
        let Some(target_dir) = self.workspace_paste_target_dir() else {
            return;
        };
        if let Err(error) = fs::create_dir_all(&target_dir) {
            self.workspace.file_error = Some(error.to_string());
            cx.notify();
            return;
        }

        if let Some(source) = self.tree_clipboard.clone().filter(|path| path.is_file()) {
            match fs::read(&source) {
                Ok(contents) => {
                    let candidate = unique_workspace_copy_path(&target_dir, &source);
                    if let Err(error) = fs::write(&candidate, contents) {
                        self.workspace.file_error = Some(error.to_string());
                    } else {
                        self.refresh_workspace_tree(cx);
                    }
                    cx.notify();
                    return;
                }
                Err(error) => {
                    self.workspace.file_error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        }

        let image = cx.read_from_clipboard().and_then(|item| {
            item.entries().iter().find_map(|entry| match entry {
                gpui::ClipboardEntry::Image(image) => Some(image.clone()),
                gpui::ClipboardEntry::String(_) => None,
            })
        });
        let Some(image) = image else {
            self.workspace.file_error = Some(
                cx.global::<crate::i18n::I18nManager>()
                    .strings()
                    .workspace_paste_empty
                    .clone(),
            );
            cx.notify();
            return;
        };

        let file_name = format!(
            "{}-{}.{}",
            crate::config::today_local_date(),
            pasted_image_bytes_hash(&image.bytes),
            clipboard_image_extension(image.format)
        );
        let candidate = unique_workspace_file_name(&target_dir, &file_name);
        if let Err(error) = fs::write(&candidate, &image.bytes) {
            self.workspace.file_error = Some(error.to_string());
        } else {
            self.refresh_workspace_tree(cx);
        }
        cx.notify();
    }

    /// 粘贴目标目录：选中目录用其本身，选中文件用其父目录，否则工作区根。
    pub(crate) fn workspace_paste_target_dir(&self) -> Option<PathBuf> {
        match self.workspace.selected.as_ref() {
            Some(WorkspaceSelection::Directory(path)) => Some(path.clone()),
            Some(WorkspaceSelection::File(path)) => {
                if path.is_dir() {
                    Some(path.clone())
                } else {
                    path.parent().map(Path::to_path_buf)
                }
            }
            Some(WorkspaceSelection::WorkspaceRoot(path)) => Some(path.clone()),
            _ => self.workspace.root.clone(),
        }
    }

    /// Double-clicking an outline heading enters rename mode: the caret jumps
    /// to the heading with its title text selected, so typing replaces it
    /// directly in the document (roadmap C6).
    pub(crate) fn rename_outline_heading(&mut self, line: usize, cx: &mut Context<Self>) {
        let range = self.buffer.line_range(line);
        let line_text = self.buffer.slice(range.clone());
        let marker_len = line_text.chars().take_while(|ch| *ch == '#').count();
        let after_marker = &line_text[marker_len..];
        let spaces = after_marker.len() - after_marker.trim_start().len();
        let title_start = range.start + marker_len + spaces;
        if title_start < range.end && self.buffer.is_char_boundary(title_start) {
            self.jump_to_document_search_range(title_start..range.end, cx);
        }
    }

    /// Clicking an outline heading jumps to that heading and expands it so its
    /// children become visible.
    pub(crate) fn open_outline_node(&mut self, id: String, line: usize, cx: &mut Context<Self>) {
        self.workspace.selected = Some(WorkspaceSelection::Outline(id.clone()));
        self.workspace.expanded.insert(id);
        // 折叠的标题被点击时先展开，使章节内容可见（roadmap C7）。
        if let Some(heading) = self.heading_block_at_source_line(line, cx) {
            heading.update(cx, |block, _cx| {
                if block.folded {
                    block.folded = false;
                    self.fold_state_version = self.fold_state_version.wrapping_add(1);
                }
            });
        }
        let range = self.buffer.line_range(line);
        if !range.is_empty() {
            self.jump_to_document_search_range(range, cx);
        } else {
            cx.notify();
        }
    }

    /// Finds the heading block whose source line equals `line`.
    pub(crate) fn heading_block_at_source_line(
        &self,
        line: usize,
        cx: &App,
    ) -> Option<Entity<crate::editor::Block>> {
        let (_, ranges) = self.build_source_target_mappings_with_block_ranges(cx);
        let line_start = self.buffer.line_range(line).start;
        ranges
            .iter()
            .find(|(_, range)| range.contains(&line_start) || range.start == line_start)
            .map(|(entity_id, _)| *entity_id)
            .and_then(|entity_id| self.document.block_entity_at_location(entity_id, cx))
    }

    /// Expands the file tree to the given path so it is visible (roadmap D1).
    pub(crate) fn reveal_path_in_tree(&mut self, path: &Path) {
        let Some(root) = self.workspace.root.as_ref() else {
            return;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            return;
        };
        // Directory node ids are `file:{path}` (see file_node_id); expand every
        // ancestor of the target.
        let mut ancestor = root.clone();
        for component in relative.components().take(relative.components().count().saturating_sub(1)) {
            if matches!(component, std::path::Component::Normal(_)) {
                ancestor.push(component.as_os_str());
                self.workspace
                    .expanded
                    .insert(format!("file:{}", ancestor.to_string_lossy()));
            }
        }
    }
}
