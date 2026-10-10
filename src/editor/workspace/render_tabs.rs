use super::*;

impl Editor {
    pub(crate) fn render_document_tabs(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.ensure_current_document_tab(cx);
        if self.workspace.open_documents.is_empty() {
            return None;
        }

        let editor = cx.entity().downgrade();
        let c = &theme.colors;
        let tabs = self
            .workspace
            .open_documents
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let path = tab.path.clone();
                let click_path = path.clone();
                let middle_click_path = path.clone();
                let drag_from_path = path.clone();
                let drop_target_path = path.clone();
                let close_path = path.clone();
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
                let tab_shows_code_icon = is_code_file(&path)
                    || path.extension().is_none()
                    || self.unsupported_preview_path.as_ref() == Some(&path);
                // The close button carries its own hover state: highlighting
                // the whole tab was indistinguishable from the tab's own
                // hover background, so the X lights up only under the pointer.
                let close_button = div()
                    .id(("document-tab-close", index))
                    .group("doc-tab-close")
                    .w(px(16.0))
                    .h(px(16.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .hover(|this| {
                        // One step darker than the tab's own hover fill —
                        // neutral, no loud accent colors.
                        this.bg({
                            let mut bg = c.dialog_secondary_button_hover;
                            bg.l = (bg.l - 0.05).max(0.0);
                            bg
                        })
                    })
                    .cursor_pointer()
                    .child({
                        // Faded body color rather than dialog_muted: inactive
                        // tabs show no other text, so the X needs to stand on
                        // its own while staying quieter than the tab title.
                        let mut idle_icon = c.text_default;
                        idle_icon.a *= 0.6;
                        svg()
                            .path(TAB_CLOSE_ICON)
                            .size(px(10.0))
                            .text_color(idle_icon)
                            .group_hover("doc-tab-close", |this| this.text_color(c.text_default))
                    })
                    .on_click({
                        let close_editor = editor.clone();
                        move |_event, window, cx| {
                            let _ = close_editor.update(cx, |editor, cx| {
                                editor.close_workspace_document(&close_path, window, cx);
                            });
                            cx.stop_propagation();
                        }
                    })
                    .into_any_element();
                div()
                    .id(("document-tab", stable_node_hash(&path.to_string_lossy())))
                    .debug_selector(move || format!("document-tab-{index}"))
                    .group("doc-tab")
                    .h_full()
                    .min_w(px(120.0))
                    .max_w(px(220.0))
                    .px(px(10.0))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .relative()
                    .border_r(px(1.0))
                    .border_color(c.dialog_border)
                    .bg(if active {
                        c.editor_background
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .hover(|this| {
                        this.bg(if active {
                            c.editor_background
                        } else {
                            c.dialog_secondary_button_hover
                        })
                    })
                    .cursor_pointer()
                    .text_size(px(theme.typography.ui_text_size(12.0)))
                    .text_color(if active {
                        c.text_default
                    } else {
                        c.dialog_muted
                    })
                    .children(active.then(|| {
                        div()
                            .absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(px(2.0))
                            .bg(c.dialog_primary_button_bg)
                    }))
                    .child(
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
                            .child(if tab_shows_code_icon { "⌘" } else { "M" }),
                    )
                    .child({
                        // 预览标签用斜体区分（用户需求：单击预览/双击固定）。
                        let mut title_el = div().flex_1().min_w(px(0.0)).truncate();
                        if tab.preview {
                            title_el = title_el.italic();
                        }
                        title_el.child(title)
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
                        let _ = tab_editor.update(cx, |editor, cx| {
                            // 点标签栏只是激活：不把预览标签升级为固定（用户需求
                            // 的预览语义：只有双击树节点或产生修改才固定）。
                            editor.open_workspace_file_in_mode(
                                click_path.clone(),
                                WorkspaceOpenMode::Activate,
                                window,
                                cx,
                            );
                        });
                    })
                    .on_drag(
                        TabDrag {
                            from_path: drag_from_path.clone(),
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
                    .drag_over::<TabDrag>({
                        let drag_hover_path = path.clone();
                        move |style, drag, _window, cx| {
                            if drag.from_path == drag_hover_path {
                                return style;
                            }
                            // 拖拽中的目标位置高亮（roadmap E3）：左侧强调边 +
                            // 悬浮底色，明确落点。
                            let theme = cx.global::<ThemeManager>().current_arc();
                            style
                                .border_l(px(2.0))
                                .border_color(theme.colors.dialog_primary_button_bg)
                                .bg(theme.colors.dialog_secondary_button_hover)
                        }
                    })
                    .on_drop({
                        let drop_editor = editor.clone();
                        move |drag: &TabDrag, window, cx| {
                        let _ = drop_editor.update(cx, |editor, cx| {
                            editor.move_tab_to_position(
                                &drag.from_path,
                                &drop_target_path,
                                window,
                                cx,
                            );
                        });
                        }
                    })
                    .on_mouse_down(MouseButton::Middle, {
                        let middle_editor = editor.clone();
                        move |_event, window, cx| {
                            let _ = middle_editor.update(cx, |editor, cx| {
                                editor.close_workspace_document(&middle_click_path, window, cx);
                            });
                            cx.stop_propagation();
                        }
                    })
                    .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                        let _ = context_editor.update(cx, |editor, cx| {
                            editor.open_tab_context_menu(event.position, index, cx);
                        });
                        cx.stop_propagation();
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        Some(
            div()
                .id("document-tabs")
                .h_full()
                .min_w(px(0.0))
                .flex()
                .track_scroll(&self.workspace.tabs_scroll_handle)
                .overflow_x_scroll()
                .children(tabs)
                .into_any_element(),
        )
    }
}
