use super::*;

impl Editor {

    /// 单击 = 预览打开，双击 = 固定打开（用户需求）；键盘触发的点击按固定处理。
    pub(crate) fn tree_click_open_mode(event: &ClickEvent) -> WorkspaceOpenMode {
        match event {
            ClickEvent::Mouse(mouse) if mouse.up.click_count >= 2 => WorkspaceOpenMode::Pinned,
            ClickEvent::Mouse(_) => WorkspaceOpenMode::Preview,
            ClickEvent::Keyboard(_) => WorkspaceOpenMode::Pinned,
        }
    }

    /// 文件树过滤：非空查询时显示匹配文件的扁平列表（roadmap D8）。
    pub(crate) fn render_tree_filter_row(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let query = self.workspace.tree_filter.clone();
        let focus = self
            .workspace
            .tree_filter_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let focus_for_click = focus.clone();
        let editor = cx.entity().downgrade();
        // 聚焦即给出高亮描边并清掉占位符：否则看起来像不可点击的静态文字
        // （用户报修）。
        let focused = focus.is_focused(window);
        div()
            .id("workspace-tree-filter")
            .relative()
            .track_focus(&focus)
            .w_full()
            .h(px(26.0))
            .px(px(8.0))
            .mr(px(6.0))
            .flex()
            .items_center()
            .rounded(px(5.0))
            .border_1()
            .border_color(if focused {
                c.dialog_primary_button_bg
            } else {
                c.dialog_border
            })
            .bg(c.editor_background)
            .text_size(px(11.0))
            .text_color(if query.is_empty() && !focused {
                c.dialog_muted
            } else {
                c.text_default
            })
            .cursor(CursorStyle::IBeam)
            .child(if query.is_empty() {
                if focused {
                    String::new()
                } else {
                    strings.tree_filter_placeholder.clone()
                }
            } else {
                query.clone()
            })
            .on_click({
                let focus = focus_for_click.clone();
                move |_event, window, _cx| window.focus(&focus)
            })
            .on_key_down({
                let editor = editor.clone();
                move |event: &KeyDownEvent, _window, cx| {
                    let _ = editor.update(cx, |editor, cx| {
                        editor.on_tree_filter_key_down(event, cx);
                    });
                }
            })
            .into_any_element()
    }

    /// 过滤框按键：字母/数字追加、退格删除、Esc 清空（roadmap D8）。
    pub(crate) fn on_tree_filter_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.clone();
        match key.as_str() {
            "escape" => {
                self.workspace.tree_filter.clear();
                self.workspace.tree_filter_focus = None;
            }
            "backspace" => {
                self.workspace.tree_filter.pop();
            }
            "shift" | "control" | "alt" | "meta" | "capslock" | "tab" | "enter" => return,
            _ => {
                if key.len() == 1 && key.chars().all(|ch| ch.is_ascii_graphic()) {
                    self.workspace.tree_filter.push_str(&key.to_lowercase());
                } else {
                    return;
                }
            }
        }
        cx.notify();
    }

    /// 树头部：排序按钮 + 过滤输入。
    pub(crate) fn render_tree_filter_and_sort_header(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let filter_row = self.render_tree_filter_row(theme, strings, window, cx);
        let order = match crate::config::EditorSettings::tree_sort(cx) {
            TreeSortPreference::Name => strings.tree_sort_name.clone(),
            TreeSortPreference::ModifiedTime => strings.tree_sort_mtime.clone(),
            TreeSortPreference::Type => strings.tree_sort_type.clone(),
        };
        div()
            .id("workspace-tree-header")
            .w_full()
            .px(px(4.0))
            .py(px(3.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(filter_row)
            .child(
                div()
                    .id("workspace-tree-sort")
                    .px(px(6.0))
                    .h(px(26.0))
                    .flex()
                    .items_center()
                    .rounded(px(5.0))
                    .text_size(px(11.0))
                    .text_color(c.dialog_muted)
                    .cursor_pointer()
                    .hover(|this| {
                        this.text_color(c.text_default)
                            .bg(c.dialog_secondary_button_hover)
                    })
                    .child(format!(
                        "{} · {} ↻",
                        strings.tree_sort_prefix, order
                    ))
                    .on_click(cx.listener(Self::on_cycle_tree_sort)),
            )
            .into_any_element()
    }
    pub(crate) fn render_workspace_files_tree(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.root.is_none() {
            return self.render_workspace_empty_state(
                &strings.workspace_no_file_title,
                &strings.workspace_no_file_message,
                theme,
            );
        }

        // 过滤激活：只显示文件名匹配的文件（roadmap D8）。
        let filter = self.workspace.tree_filter.trim().to_lowercase();
        if !filter.is_empty() {
            let mut rows: Vec<AnyElement> = Vec::new();
            let all_files = self.workspace_text_files();
            let editor = editor.clone();
            let mut shown = 0usize;
            for path in all_files {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !name.contains(&filter) {
                    continue;
                }
                shown += 1;
                if shown > 50 {
                    break;
                }
                let label = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let directory = path
                    .parent()
                    .map(|parent| parent.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let row_editor = editor.clone();
                let row_path = path.clone();
                rows.push(
                    div()
                        .id(("tree-filter-hit", shown))
                        .w_full()
                        .px(px(10.0))
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .hover(|this| this.bg(theme.colors.dialog_secondary_button_hover))
                        .child(
                            div()
                                .max_w(px(180.0))
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(theme.colors.text_default)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(10.5))
                                .text_color(theme.colors.dialog_muted)
                                .child(directory),
                        )
                        .on_click(move |event, window, cx| {
                            let _ = row_editor.update(cx, |editor, cx| {
                                let mode = Self::tree_click_open_mode(&event);
                                editor.open_workspace_file_in_mode(
                                    row_path.clone(),
                                    mode,
                                    window,
                                    cx,
                                );
                            });
                            let _ = event;
                        })
                        .into_any_element(),
                );
            }
            if rows.is_empty() {
                return self.render_workspace_empty_state(
                    "",
                    &strings.workspace_no_search_results,
                    theme,
                );
            }
            return div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .children(rows)
                .into_any_element();
        }

        if let Some(error) = self.workspace.file_error.as_ref() {
            return self.render_workspace_empty_state(
                &strings.workspace_scan_failed_title,
                error,
                theme,
            );
        }

        let Some(root) = self.workspace.file_tree.as_ref() else {
            // 后台扫描进行中（roadmap D9）：与搜索面板一致显示处理中占位，
            // 而不是误报「空文件夹」。
            if self.workspace.tree_scan_root.is_some()
                && self.workspace.tree_scan_root.as_ref() == self.workspace.root.as_ref()
            {
                return div()
                    .p(px(12.0))
                    .text_size(px(14.0))
                    .text_color(theme.colors.dialog_muted)
                    .child("…")
                    .into_any_element();
            }
            return self.render_workspace_empty_state("", &strings.workspace_empty_files, theme);
        };

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(std::slice::from_ref(root), 0, theme, editor))
            .into_any_element()
    }
}
