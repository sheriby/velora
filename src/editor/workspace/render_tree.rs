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
        let window = self.workspace_list_window(rows.len());
        self.panel_rows_rendered.set(window.len() as u64);
        self.panel_first_row_rendered.set(window.start as u64);
        let needs_fill = rows.len() > PANEL_WINDOW_THRESHOLD_ROWS
            && f32::from(self.workspace.tree_scroll_handle.bounds().size.height) <= 0.0;
        let element = {
            let mut elements: Vec<AnyElement> = Vec::with_capacity(window.len() + 2);
            // 上下各垫一段等高空白：滚动条的长度与位置仍按整棵树算，
            // 中间只挂视口里那一窗真行（与大纲面板同一手法）。
            if window.start > 0 {
                elements.push(
                    div()
                        .h(px(window.start as f32 * WORKSPACE_NODE_HEIGHT))
                        .flex_shrink_0()
                        .into_any_element(),
                );
            }
            for row in &rows[window.clone()] {
                elements.push(self.render_workspace_tree_row(row, theme, editor));
            }
            let below = rows.len() - window.end;
            if below > 0 {
                elements.push(
                    div()
                        .h(px(below as f32 * WORKSPACE_NODE_HEIGHT))
                        .flex_shrink_0()
                        .into_any_element(),
                );
            }
            div()
                .w_full()
                .flex()
                .flex_col()
                .children(elements)
                .into_any_element()
        };
        // 首帧还没量过滚动视口：先铺一小段，立刻排下一帧补齐（帧数封顶，量到尺寸即停）。
        if needs_fill && self.panel_fill_frames < PANEL_FILL_MAX_FRAMES {
            self.panel_fill_frames += 1;
            self.schedule_followup_frame(cx);
        } else if !needs_fill {
            self.panel_fill_frames = 0;
        }
        element
    }
}
