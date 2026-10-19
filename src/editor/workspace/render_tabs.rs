use super::*;
use gpui::prelude::FluentBuilder;
use gpui_kit::base::{Tab as DocumentTab, Tabs as DocumentTabs};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _,
    button::{Button, ButtonVariants},
    tooltip::Tooltip,
};

impl Editor {
    pub(crate) fn render_document_tabs(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tabs = self.render_document_tab_items(theme, None, cx);
        if tabs.is_empty() {
            self.workspace.tabs_revealed = None;
            return None;
        }
        let group_id = cx.entity_id();
        if let Some((index, tab)) = self
            .workspace
            .open_documents
            .iter()
            .enumerate()
            .find(|(_, tab)| Some(&tab.path) == self.workspace.active_document.as_ref())
        {
            self.reveal_document_tab(group_id, tab.path.clone(), index, window, cx);
        }
        Some(
            DocumentTabs::new("document-tabs")
                .debug_selector(move || format!("document-tabs-{group_id:?}"))
                .h_full()
                .min_w(px(0.0))
                .flex()
                .track_scroll(&self.workspace.tabs_scroll_handle)
                .overflow_x_scroll()
                .children(tabs.into_iter().map(|(_, tab)| tab))
                .into_any_element(),
        )
    }

    pub(crate) fn render_document_tab_items(
        &mut self,
        theme: &Theme,
        group_accent: Option<Hsla>,
        cx: &mut Context<Self>,
    ) -> Vec<(u64, AnyElement)> {
        self.ensure_current_document_tab(cx);
        let editor = cx.entity().downgrade();
        let c = &theme.colors;
        let tabs_focus = self
            .workspace
            .tabs_focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        let strings = cx.global::<I18nManager>().strings_arc();
        let tabs = self
            .workspace
            .open_documents
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let path = tab.path.clone();
                let path_hash = stable_node_hash(&path.to_string_lossy());
                let click_path = path.clone();
                let middle_click_path = path.clone();
                let drag_from_path = path.clone();
                let close_path = path.clone();
                let context_path = path.clone();
                let tooltip = path.to_string_lossy().into_owned();
                let active = self.workspace.active_document.as_ref() == Some(&tab.path);
                let dirty = if active {
                    self.document_dirty
                } else {
                    tab.dirty
                };
                let title = tab
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| tab.path.to_string_lossy().into_owned());
                let tab_editor = editor.clone();
                let context_editor = editor.clone();
                // Markdown glyph only for real markdown files; code files and
                // extension-less dotfiles render as source, binaries show as
                // placeholders — neither is markdown.
                let tab_shows_code_icon = untitled_document_id(&path).is_none()
                    && (is_code_file(&path)
                        || path.extension().is_none()
                        || self.unsupported_preview_path.as_ref() == Some(&path));
                let tab_is_image = crate::components::Block::has_supported_image_extension(&path);
                let close_label = format!("{} — {title}", strings.menu_close_tab);
                let close_button = Button::new(("document-tab-close", path_hash))
                    .debug_selector(move || format!("document-tab-close-{index}"))
                    .ghost()
                    .xsmall()
                    .tab_stop(active)
                    .accessibility_label(close_label.clone())
                    .tooltip(close_label)
                    .icon(
                        Icon::empty()
                            .path(TAB_CLOSE_ICON)
                            .text_color(c.dialog_muted),
                    )
                    .on_click({
                        let close_editor = editor.clone();
                        move |_, window, cx| {
                            cx.stop_propagation();
                            if let Err(error) = close_editor.update(cx, |editor, cx| {
                                editor.close_workspace_document(&close_path, window, cx);
                            }) {
                                eprintln!("关闭标签失败：{error}");
                            }
                        }
                    });
                let tab = DocumentTab::new(("document-tab", path_hash))
                    .selected(active && self.split_active)
                    .accessibility_label(title.clone())
                    .set_position(index + 1, self.workspace.open_documents.len())
                    .debug_selector(move || format!("document-tab-{index}"))
                    .group("doc-tab")
                    .h_full()
                    .min_w(rems(7.5))
                    .max_w(rems(13.75))
                    .px_2p5()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .relative()
                    .when(group_accent.is_none(), |this| {
                        this.border_r(px(1.0)).border_color(c.dialog_border)
                    })
                    .bg(if active {
                        c.editor_background
                    } else {
                        cx.theme().transparent
                    })
                    .hover(|this| {
                        this.bg(if active {
                            c.editor_background
                        } else {
                            c.dialog_secondary_button_hover
                        })
                    })
                    .text_xs()
                    .focus_visible(|style| {
                        style.bg(cx.theme().selection).border_color(cx.theme().ring)
                    })
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .when(active, |tab| {
                        tab.track_focus(&tabs_focus)
                            .on_key_down(cx.listener(Self::on_document_tab_key_down))
                    })
                    .text_color(if active {
                        c.dialog_body
                    } else {
                        c.dialog_muted
                    })
                    .children(group_accent.map(|accent| {
                        let group_id = cx.entity_id();
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(2.0))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .debug_selector(move || {
                                        format!("tab-group-left-border-{group_id:?}-{index}")
                                    })
                                    .w_full()
                                    .h(px(18.0))
                                    .rounded(px(1.0))
                                    .bg(accent),
                            )
                    }))
                    .children(active.then(|| {
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(2.0))
                            .bg(if self.split_active {
                                c.dialog_primary_button_bg
                            } else {
                                c.dialog_border
                            })
                    }))
                    .children([false, true].into_iter().map(|after| {
                        let drop_target_path = path.clone();
                        let drag_hover_path = path.clone();
                        let drop_editor = editor.clone();
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .w(relative(0.5))
                            .when(after, |this| this.right_0())
                            .when(!after, |this| this.left_0())
                            .drag_over::<EditorFileDrag>(move |style, drag, _window, cx| {
                                if drag.from_path == drag_hover_path {
                                    return style;
                                }
                                let theme = cx.global::<ThemeManager>().current_arc();
                                let style = if after {
                                    style.border_r(px(2.0))
                                } else {
                                    style.border_l(px(2.0))
                                };
                                style
                                    .border_color(theme.colors.dialog_primary_button_bg)
                                    .bg(theme.colors.dialog_secondary_button_hover)
                            })
                            .on_drop(move |drag: &EditorFileDrag, window, cx| {
                                if let Err(error) = drop_editor.update(cx, |editor, cx| {
                                    editor.receive_editor_file_drop(
                                        drag,
                                        Some((drop_target_path.clone(), after)),
                                        None,
                                        window,
                                        cx,
                                    );
                                }) {
                                    eprintln!("移动标签失败：{error}");
                                }
                            })
                    }))
                    .child(if tab_is_image {
                        svg()
                            .path(GENERIC_FILE_ICON)
                            .size(px(12.0))
                            .text_color(c.dialog_muted)
                            .into_any_element()
                    } else {
                        div()
                            .w(px(11.0))
                            .text_center()
                            .text_size(px(theme.typography.ui_text_size(9.0)))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if tab_shows_code_icon {
                                c.dialog_muted
                            } else {
                                c.dialog_primary_button_bg
                            })
                            .child(if tab_shows_code_icon { "⌘" } else { "M" })
                            .into_any_element()
                    })
                    .child({
                        // 预览标签用斜体区分（用户需求：单击预览/双击固定）。
                        let mut title_el = div().flex_1().min_w(px(0.0)).truncate();
                        if tab.preview {
                            title_el = title_el.italic();
                        }
                        title_el
                            .when(untitled_document_id(&path).is_some(), |title| {
                                title.debug_selector(|| "untitled-tab".into())
                            })
                            .child(title)
                    })
                    .children(dirty.then(|| {
                        // 未保存的圆点：每个脏标签都点，包括当前这一篇（用户需求：
                        // 像 VS Code 那样在标签上标出「这篇改过还没落盘」）。此前刻意
                        // 跳过活动标签，于是正在编辑的那篇反而没有标记。
                        div()
                            .w(px(7.0))
                            .h(px(7.0))
                            .flex_shrink_0()
                            .rounded(px(4.0))
                            .bg(c.dialog_primary_button_bg)
                            .debug_selector(|| format!("document-tab-dirty-{index}"))
                    }))
                    .child(close_button)
                    .on_click(move |_event, window, cx| {
                        if let Err(error) = tab_editor.update(cx, |editor, cx| {
                            // 点标签栏只是激活：不把预览标签升级为固定（用户需求
                            // 的预览语义：只有双击树节点或产生修改才固定）。
                            editor.open_workspace_file_in_mode(
                                click_path.clone(),
                                WorkspaceOpenMode::Activate,
                                window,
                                cx,
                            );
                        }) {
                            eprintln!("激活标签失败：{error}");
                        }
                    })
                    .on_drag(
                        EditorFileDrag {
                            from_path: drag_from_path.clone(),
                            source: Some(editor.clone()),
                        },
                        move |drag, _offset, _window, cx| {
                            let label = drag
                                .from_path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            cx.new(|_| DraggedTabPreview {
                                label: label.into(),
                            })
                        },
                    )
                    .on_mouse_down(MouseButton::Middle, {
                        let middle_editor = editor.clone();
                        move |_event, window, cx| {
                            if let Err(error) = middle_editor.update(cx, |editor, cx| {
                                editor.close_workspace_document(&middle_click_path, window, cx);
                            }) {
                                eprintln!("关闭标签失败：{error}");
                            }
                            cx.stop_propagation();
                        }
                    })
                    .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                        if let Err(error) = context_editor.update(cx, |editor, cx| {
                            if let Some(index) = editor
                                .workspace
                                .open_documents
                                .iter()
                                .position(|tab| tab.path == context_path)
                            {
                                editor.open_tab_context_menu(event.position, index, cx);
                            }
                        }) {
                            eprintln!("打开标签菜单失败：{error}");
                        }
                        cx.stop_propagation();
                    })
                    .into_any_element();
                (path_hash, tab)
            })
            .collect::<Vec<_>>();

        tabs
    }

    pub(super) fn reveal_document_tab(
        &mut self,
        group: EntityId,
        path: PathBuf,
        index: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let key = WorkspaceTabRevealKey {
            group,
            path,
            index,
            window_width: window.viewport_size().width,
            viewport_width: self.workspace.tabs_scroll_handle.bounds().size.width,
            rem_size: window.rem_size(),
            font_family: cx.theme().font_family.clone(),
        };
        if self.workspace.tabs_revealed.as_ref() != Some(&key) {
            self.workspace.tabs_scroll_handle.scroll_to_item(index);
            self.workspace.tabs_revealed = Some(key);
            // 首帧的视口尚未落地；补帧后重核几何，普通重绘不干预手动滚动。
            self.schedule_followup_frame(cx);
        }
    }

    fn on_document_tab_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(focus) = self.workspace.tabs_focus.as_ref().cloned() else {
            return;
        };
        if !focus.is_focused(window) || event.keystroke.modifiers.modified() {
            return;
        }
        let Some(index) = self
            .workspace
            .open_documents
            .iter()
            .position(|tab| Some(&tab.path) == self.workspace.active_document.as_ref())
        else {
            return;
        };
        let last = self.workspace.open_documents.len().saturating_sub(1);
        let next = match event.keystroke.key.as_str() {
            "left" => index.saturating_sub(1),
            "right" => index.saturating_add(1).min(last),
            "home" => 0,
            "end" => last,
            _ => return,
        };
        if next != index {
            let Some(path) = self
                .workspace
                .open_documents
                .get(next)
                .map(|tab| tab.path.clone())
            else {
                return;
            };
            self.open_workspace_file_in_mode(path, WorkspaceOpenMode::Activate, window, cx);
            if !self.modal_is_open() {
                self.pending_focus = None;
                window.focus(&focus, cx);
            }
            if self.split_parent.is_none() {
                self.workspace.tabs_scroll_handle.scroll_to_item(next);
            }
        }
        cx.stop_propagation();
    }
}
