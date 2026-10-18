use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _,
    button::{Button, ButtonVariants},
};

impl Editor {
    pub(crate) fn toggle_workspace_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_open {
            self.workspace.is_open = false;
            // 面板关了就不该再显示正文搜索高亮。inactive 分支会清掉全部范围。
            self.sync_document_search_highlights(cx);
            self.refocus_document_after_sidebar_collapse(window, cx);
        } else {
            self.close_menu_bar(cx);
            self.dismiss_contextual_overlays(cx);
            self.workspace.is_open = true;
            self.sync_workspace_models(cx);
            window.activate_window();
        }
        // 开关进记忆：下次启动按设置（跟随上次时）恢复（用户要求）。
        self.persist_session(cx);
        cx.notify();
    }

    /// 收起侧栏后，若焦点还留在侧栏自己的控件上（导航按钮、文件树、搜索/替换输入、改名
    /// 输入），把焦点交还正文：面板一收这些控件就不再渲染，悬空的焦点会让之后
    /// 的键盘事件静默丢失——再按 cmd/ctrl-b 也唤不出侧栏（派发回退到根节点，
    /// 挂在编辑器根部的那组监听全收不到）。
    fn refocus_document_after_sidebar_collapse(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sidebar_owns_focus = self
            .workspace
            .navigation_focus
            .as_ref()
            .is_some_and(|handle| handle.contains_focused(window, cx))
            || self
                .workspace
                .tree_focus
                .as_ref()
                .is_some_and(|handle| handle.is_focused(window))
            || self
                .workspace
                .search_focus
                .as_ref()
                .is_some_and(|handle| handle.is_focused(window))
            || self
                .workspace
                .replace_focus
                .as_ref()
                .is_some_and(|handle| handle.is_focused(window))
            || self
                .workspace
                .name_edit
                .as_ref()
                .is_some_and(|edit| edit.focus.is_focused(window));
        if !sidebar_owns_focus {
            return;
        }
        self.refocus_workspace_content(window, cx);
    }

    fn refocus_workspace_content(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(parent) = self.split_parent.clone() {
            // 分屏命令可能仍在宿主的更新栈内，先退出当前事件再更新宿主。
            window.defer(cx, move |window, cx| {
                if let Err(error) = parent.update(cx, |editor, cx| {
                    editor.refocus_workspace_document(cx);
                }) {
                    eprintln!("归还工作区正文焦点失败：{error}");
                }
                window.refresh();
            });
        } else {
            self.refocus_workspace_document(cx);
        }
    }

    fn refocus_workspace_document(&mut self, cx: &mut Context<Self>) {
        if self.refocus_visible_ai_review(cx) {
            return;
        }
        let focus = |editor: &mut Editor, cx: &mut Context<Editor>| {
            if let Some(entity_id) = editor.current_edit_target_entity_id_from_state(cx) {
                editor.focus_block(entity_id);
                cx.notify();
            }
        };
        if let Some(active) = self.active_split_editor() {
            active.update(cx, focus);
        } else {
            focus(self, cx);
        }
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
    pub(crate) fn render_activity_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let navigation_focus = self
            .workspace
            .navigation_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let strings = cx.global::<I18nManager>().strings_arc();
        let button = |id: &'static str, icon, label: &str, tab, cx: &mut Context<Self>| {
            let selected = self.workspace.is_open && self.workspace.active_tab == tab;
            Button::new(id)
                .debug_selector(move || id.to_owned())
                .ghost()
                .icon(icon)
                .selected(selected)
                .toggled(selected)
                .accessibility_label(label.to_owned())
                .tooltip(label.to_owned())
                .on_click(cx.listener(move |editor, _, window, cx| {
                    if editor.workspace.is_open && editor.workspace.active_tab == tab {
                        editor.workspace.is_open = false;
                        editor.refocus_workspace_content(window, cx);
                        editor.persist_session(cx);
                        cx.notify();
                        return;
                    }
                    editor.workspace.is_open = true;
                    editor.set_workspace_tab(tab, cx);
                    editor.persist_session(cx);
                    cx.notify();
                }))
        };
        div()
            .id("workspace-activity-rail")
            .track_focus(&navigation_focus)
            .tab_stop(false)
            .w(WORKSPACE_ACTIVITY_WIDTH)
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .pt_3()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(button(
                "activity-files",
                IconName::FileText,
                &strings.workspace_tab_files,
                WorkspaceTab::Files,
                cx,
            ))
            .child(button(
                "activity-search",
                IconName::Search,
                &strings.workspace_tab_search,
                WorkspaceTab::Search,
                cx,
            ))
            .child(button(
                "activity-outline",
                IconName::ListTree,
                &strings.workspace_tab_outline,
                WorkspaceTab::Outline,
                cx,
            ))
            .into_any_element()
    }
}
