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

    pub(crate) fn render_workspace_files_tree(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.workspace.root.is_none() {
            return self.render_workspace_empty_state(
                &strings.workspace_no_file_title,
                &strings.workspace_no_file_message,
                theme,
            );
        }

        if let Some(error) = self.workspace.file_error.as_ref() {
            return self.render_workspace_empty_state(
                &strings.workspace_scan_failed_title,
                error,
                theme,
            );
        }

        if self.workspace.file_tree.is_none() {
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
        }

        let rows = self.workspace_files_rows();
        let total = rows.len();
        let window = self.workspace_list_window(total);
        let visible = rows[window.clone()]
            .iter()
            .map(|row| self.render_workspace_tree_row(row, theme, editor))
            .collect();
        self.workspace_windowed_body(total, window, visible, cx)
    }
}
