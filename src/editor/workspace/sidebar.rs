use super::*;

impl Editor {
    pub(crate) fn toggle_workspace_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_open {
            self.workspace.is_open = false;
            // 面板关了就不该再显示正文搜索高亮（用户报修：残留高亮没有面板
            // 可以解释，也没有别的路径清它）。inactive 分支会清掉全部范围。
            self.sync_document_search_highlights(cx);
        } else {
            self.close_menu_bar(cx);
            self.dismiss_contextual_overlays(cx);
            self.workspace.is_open = true;
            self.sync_workspace_models(cx);
            window.activate_window();
        }
        cx.notify();
    }

    pub(crate) fn on_toggle_workspace_action(
        &mut self,
        _: &crate::components::ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_workspace_drawer(window, cx);
    }
    pub(crate) fn code_tab_active(&self) -> bool {
        self.code_document
    }

    pub(crate) fn workspace_breadcrumb(&self) -> String {
        self.file_path
            .as_ref()
            .or(self.recovery_source_path.as_ref())
            .and_then(|path| path.file_name())
            .or_else(|| {
                self.workspace
                    .root
                    .as_ref()
                    .and_then(|path| path.file_name())
            })
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub(crate) fn active_code_line_count(&self, cx: &App) -> Option<usize> {
        self.code_tab_active().then(|| {
            // P4a：按修订缓存；逐块统计换行片段数，禁止整篇重新序列化
            // （此前每帧 raw_source_text 一次，10 MiB 文档是每帧的灾难）。
            let revision = self.document_revision;
            if let Some((cached_revision, lines)) = self.code_line_count_cache.get()
                && cached_revision == revision
            {
                return lines;
            }
            let mut pieces = 0usize;
            for visible in self.document.visible_blocks() {
                pieces += visible.entity.read(cx).display_text().split('\n').count();
            }
            if let Some(tail) = self.document.pending_tail() {
                pieces += tail.lines.len() - tail.next_line;
            }
            if let Some(tail) = self.document.pending_source() {
                pieces += tail.source.split('\n').count().saturating_sub(1);
            }
            let lines = pieces.max(1);
            self.code_line_count_cache.set(Some((revision, lines)));
            lines
        })
    }

    pub(crate) fn current_workspace_panel_width(&self, viewport_width: f32, cx: &App) -> f32 {
        let width = self
            .workspace
            .panel_width
            .unwrap_or_else(|| crate::config::EditorSettings::workspace_sidebar_width(cx) as f32);
        clamp_workspace_panel_width(width, viewport_width)
    }

    pub(crate) fn start_workspace_resize(&mut self, pointer_x: f32, width: f32, cx: &mut Context<Self>) {
        self.workspace.resize_drag = Some(WorkspaceResizeDrag {
            start_x: pointer_x,
            start_width: width,
        });
        cx.notify();
    }

    pub(crate) fn on_workspace_resize_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.workspace.resize_drag else {
            return;
        };
        let viewport = f32::from(window.viewport_size().width);
        let width = clamp_workspace_panel_width(
            drag.start_width + f32::from(event.position.x) - drag.start_x,
            viewport,
        );
        self.workspace.panel_width = Some(width);
        cx.notify();
    }

    pub(crate) fn on_workspace_resize_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.resize_drag.take().is_some() {
            if let Some(width) = self.workspace.panel_width {
                crate::config::EditorSettings::set_workspace_sidebar_width(
                    cx,
                    width.round() as u16,
                );
            }
            cx.notify();
        }
    }
    pub(crate) fn render_activity_rail(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();
        let button = |id: &'static str,
                      icon_path: &'static str,
                      label: &'static str,
                      selected: bool,
                      tab: WorkspaceTab,
                      editor: WeakEntity<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .w(px(34.0))
                .h(px(34.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.0))
                .bg(if selected {
                    c.selection
                } else {
                    c.dialog_secondary_button_bg
                })
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(
                    svg()
                        .path(icon_path)
                        .size(px(18.0))
                        .text_color(if selected {
                            c.dialog_primary_button_bg
                        } else {
                            c.dialog_muted
                        }),
                )
                .tooltip(move |_, cx| {
                    cx.new(|_| WorkspaceTooltip {
                        label: label.into(),
                    })
                    .into()
                })
                .on_click(move |_, _, cx| {
                    let _ = editor.update(cx, |editor, cx| {
                        // 三个按钮一致：已经开在这一页时再点一次就收起侧边栏
                        // （之前只有文件和搜索会收，大纲那个参数写的是 false）。
                        if editor.workspace.is_open && editor.workspace.active_tab == tab {
                            editor.workspace.is_open = false;
                            cx.notify();
                            return;
                        }
                        editor.workspace.is_open = true;
                        editor.set_workspace_tab(tab, cx);
                        cx.notify();
                    });
                })
        };
        div()
            .id("workspace-activity-rail")
            .w(px(50.0))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.0))
            .pt(px(12.0))
            .bg(c.dialog_secondary_button_bg)
            .border_r(px(1.0))
            .border_color(c.dialog_border)
            .child(button(
                "activity-files",
                ACTIVITY_FILES_ICON,
                "文件",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Files,
                WorkspaceTab::Files,
                editor.clone(),
            ))
            .child(button(
                "activity-search",
                ACTIVITY_SEARCH_ICON,
                "搜索",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Search,
                WorkspaceTab::Search,
                editor.clone(),
            ))
            .child(button(
                "activity-outline",
                ACTIVITY_OUTLINE_ICON,
                "大纲",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Outline,
                WorkspaceTab::Outline,
                editor.clone(),
            ))
            .child(button(
                "activity-backlinks",
                ACTIVITY_BACKLINKS_ICON,
                "反链",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Backlinks,
                WorkspaceTab::Backlinks,
                editor.clone(),
            ))
            .child(button(
                "activity-tags",
                ACTIVITY_TAGS_ICON,
                "标签",
                self.workspace.is_open && self.workspace.active_tab == WorkspaceTab::Tags,
                WorkspaceTab::Tags,
                editor,
            ))
            .into_any_element()
    }
}
