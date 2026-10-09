use super::*;

impl Editor {

    /// VS Code-style search header: query input, collapsible replace input,
    /// option toggles, and a document/workspace scope switch. Rendered above
    /// the result list when the Search tab is active.
    pub(crate) fn render_search_header(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();

        let query_focused = self
            .workspace
            .search_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window));
        let search_input = self.render_search_input(
            "workspace-search-query",
            self.workspace.search_query.clone(),
            strings.workspace_search_placeholder.clone(),
            SearchInputKind::Query,
            query_focused,
            theme,
            cx,
        );
        let replace_visible = self.workspace.replace_visible;
        let replace_input = replace_visible.then(|| {
            let replace_focused = self
                .workspace
                .replace_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window));
            self.render_search_input(
                "workspace-search-replace",
                self.workspace.replace_query.clone(),
                strings.search_replace_placeholder.clone(),
                SearchInputKind::Replace,
                replace_focused,
                theme,
                cx,
            )
        });

        let toggle_editor = editor.clone();
        let toggle_button = div()
            .id("workspace-search-toggle-replace")
            .w(px(24.0))
            .h(px(24.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.0))
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_color(if replace_visible {
                c.dialog_primary_button_bg
            } else {
                c.dialog_muted
            })
            .child(
                svg()
                    .path(if replace_visible {
                        CHEVRON_DOWN_ICON
                    } else {
                        CHEVRON_RIGHT_ICON
                    })
                    .size(px(14.0))
                    .text_color(if replace_visible {
                        c.dialog_primary_button_bg
                    } else {
                        c.dialog_muted
                    }),
            )
            .tooltip(|_, cx| {
                cx.new(|_| WorkspaceTooltip {
                    label: "显示替换".into(),
                })
                .into()
            })
            .on_click(move |_, _, cx| {
                let _ = toggle_editor.update(cx, |editor, cx| {
                    editor.workspace.replace_visible = !editor.workspace.replace_visible;
                    cx.notify();
                });
            });

        let options_row = self.render_search_options_row(theme, strings, cx);

        let header = div()
            .id("workspace-search-header")
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .px(px(6.0))
            .pt(px(8.0))
            .pb(px(2.0))
            .border_b(px(1.0))
            .border_color(c.dialog_border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(search_input)
                    .child(toggle_button),
            )
            .children(replace_input)
            .child(options_row)
            .children(self.render_search_pattern_error(strings, theme));
        header.into_any_element()
    }

    /// 模式编译失败时的那一行提示：本地化的前缀 + 匹配引擎交回的原始诊断。
    /// 诊断原文是多行的（带一个 `^` 指出出错列），所以整行截断，鼠标悬停看全文。
    pub(crate) fn render_search_pattern_error(
        &self,
        strings: &I18nStrings,
        theme: &Theme,
    ) -> Option<AnyElement> {
        let message = self.workspace.search_error.as_ref()?;
        let c = &theme.colors;
        let summary = message.lines().next().unwrap_or(message.as_str()).to_string();
        let detail = message.clone();
        Some(
            div()
                .id("workspace-search-pattern-error")
                .w_full()
                .px(px(2.0))
                .text_size(px(11.0))
                .text_color(c.dialog_danger_button_text)
                .truncate()
                .child(format!("{}{summary}", strings.search_invalid_pattern))
                .tooltip(move |_, cx| {
                    let detail = detail.clone();
                    cx.new(|_| WorkspaceTooltip { label: detail.into() }).into()
                })
                .into_any_element(),
        )
    }

    /// Option toggles (case / whole word / regex / fuzzy), the scope switch,
    /// and — for the document scope — the replace action buttons.
    pub(crate) fn render_search_options_row(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();

        let toggle_chip = |editor: &WeakEntity<Self>,
                           id: &'static str,
                           label: String,
                           selected: bool,
                           tooltip: String| {
            let chip_editor = editor.clone();
            div()
                .id(id)
                .px(px(6.0))
                .h(px(22.0))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(if selected {
                    c.dialog_primary_button_bg
                } else {
                    c.dialog_border
                })
                .bg(if selected {
                    c.selection
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                })
                .text_size(px(11.0))
                .text_color(if selected {
                    c.dialog_primary_button_bg
                } else {
                    c.dialog_muted
                })
                .cursor_pointer()
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .tooltip(move |_, cx| {
                    let tooltip = tooltip.clone();
                    cx.new(|_| WorkspaceTooltip { label: tooltip }).into()
                })
                .child(label)
                .on_click(move |_, _, cx| {
                    let _ = chip_editor.update(cx, |editor, cx| {
                        match id {
                            "workspace-search-case" => {
                                editor.workspace.search_match_case =
                                    !editor.workspace.search_match_case;
                            }
                            "workspace-search-word" => {
                                editor.workspace.search_whole_word =
                                    !editor.workspace.search_whole_word;
                            }
                            "workspace-search-regex" => {
                                editor.workspace.search_use_regex =
                                    !editor.workspace.search_use_regex;
                            }
                            "workspace-search-fuzzy" => {
                                editor.workspace.search_fuzzy = !editor.workspace.search_fuzzy;
                            }
                            _ => {}
                        }
                        editor.schedule_workspace_search(cx);
                        cx.notify();
                    });
                })
        };

        let mut options = div()
            .id("workspace-search-options")
            .w_full()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(4.0))
            .child(toggle_chip(
                &editor,
                "workspace-search-case",
                "Aa".to_string(),
                self.workspace.search_match_case,
                strings.search_case_sensitive.clone(),
            ))
            .child(toggle_chip(
                &editor,
                "workspace-search-word",
                "ab".to_string(),
                self.workspace.search_whole_word,
                strings.search_whole_word.clone(),
            ))
            .child(toggle_chip(
                &editor,
                "workspace-search-regex",
                ".*".to_string(),
                self.workspace.search_use_regex,
                format!("{} — {}", strings.search_regex, strings.search_regex_hint),
            ))
            .child(toggle_chip(
                &editor,
                "workspace-search-fuzzy",
                strings.search_fuzzy_short.clone(),
                self.workspace.search_fuzzy,
                strings.search_fuzzy.clone(),
            ));

        // Scope switch: current document vs. whole workspace (the latter needs
        // a scanned tree).
        let scope = self.workspace.search_scope;
        let scope_button =
            |editor: &WeakEntity<Self>, id: &'static str, label: String, selected: bool| {
                let scope_editor = editor.clone();
                div()
                    .id(id)
                    .px(px(6.0))
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .bg(if selected {
                        c.selection
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .text_size(px(11.0))
                    .text_color(if selected {
                        c.dialog_primary_button_bg
                    } else {
                        c.dialog_muted
                    })
                    .cursor_pointer()
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        let _ = scope_editor.update(cx, |editor, cx| {
                            editor.workspace.search_scope = match id {
                                "workspace-scope-document" => WorkspaceSearchScope::Document,
                                _ => WorkspaceSearchScope::Workspace,
                            };
                            editor.workspace.search_active_index = None;
                            editor.workspace.document_active_range = None;
                            editor.schedule_workspace_search(cx);
                            cx.notify();
                        });
                    })
            };
        let match_count = self.workspace.search_results.len();
        let count_label = if match_count > 0 && scope == WorkspaceSearchScope::Document {
            Some(
                strings
                    .search_result_count
                    .replace("{n}", &match_count.to_string()),
            )
        } else {
            None
        };

        options = options.child(
            div()
                .id("workspace-search-scope-row")
                .w_full()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(scope_button(
                    &editor,
                    "workspace-scope-document",
                    strings.search_scope_document.clone(),
                    scope == WorkspaceSearchScope::Document,
                ))
                .child(scope_button(
                    &editor,
                    "workspace-scope-workspace",
                    strings.search_scope_workspace.clone(),
                    scope == WorkspaceSearchScope::Workspace,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .justify_end()
                        .children(count_label.map(|label| {
                            div()
                                .text_size(px(11.0))
                                .text_color(c.dialog_muted)
                                .child(label)
                        })),
                ),
        );

        // Replace actions (document scope only for now; workspace replace
        // lives behind the same buttons when the workspace scope is active).
        if self.workspace.replace_visible && !self.workspace.replace_query.is_empty() {
            let replace_editor = editor.clone();
            let replace_all_editor = editor.clone();
            let is_document_scope = scope == WorkspaceSearchScope::Document;
            let replace_row = div()
                .w_full()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(6.0))
                .child(
                    div()
                        .id("workspace-search-replace-current")
                        .px(px(8.0))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .border_1()
                        .border_color(c.dialog_border)
                        .text_size(px(11.0))
                        .text_color(c.text_default)
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .child(strings.search_replace_current.clone())
                        .on_click(move |_, window, cx| {
                            let _ = replace_editor.update(cx, |editor, cx| {
                                if is_document_scope {
                                    editor.replace_active_document_match(window, cx);
                                }
                                cx.notify();
                            });
                            cx.stop_propagation();
                        }),
                )
                .child(
                    div()
                        .id("workspace-search-replace-all")
                        .px(px(8.0))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .rounded(px(5.0))
                        .border_1()
                        .border_color(c.dialog_border)
                        .text_size(px(11.0))
                        .text_color(c.text_default)
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .child(strings.search_replace_all.clone())
                        .on_click(move |_, window, cx| {
                            let _ = replace_all_editor.update(cx, |editor, cx| {
                                if is_document_scope {
                                    editor.replace_all_document_matches(window, cx);
                                } else {
                                    let replaced =
                                        editor.replace_all_workspace_matches(window, cx);
                                    if replaced > 0 {
                                        editor.schedule_workspace_search(cx);
                                    }
                                }
                                cx.notify();
                            });
                            cx.stop_propagation();
                        }),
                );
            options = options.child(replace_row);
        }

        options.into_any_element()
    }

    /// Shared single-line input used by the query and replace fields. Clicks
    /// focus the field; key handling and IME route through `SearchInputKind`.
    pub(crate) fn render_search_input(
        &mut self,
        id: &'static str,
        value: String,
        placeholder: String,
        kind: SearchInputKind,
        focused: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let focus = match kind {
            SearchInputKind::Query => self
                .workspace
                .search_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone(),
            SearchInputKind::Replace => self
                .workspace
                .replace_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone(),
        };
        let focus_for_click = focus.clone();
        let focus_for_input = focus.clone();
        let input_editor = cx.entity();
        let editor = cx.entity().downgrade();

        // The placeholder disappears as soon as the field is focused, not
        // just once text is typed.
        let (label, muted) = if !value.is_empty() {
            (value, false)
        } else if focused {
            (String::new(), true)
        } else {
            (placeholder, true)
        };

        div()
            .id(id)
            .relative()
            .track_focus(&focus)
            .flex_1()
            .min_w(px(0.0))
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(if focused {
                c.dialog_primary_button_bg
            } else {
                c.dialog_border
            })
            .bg(c.editor_background)
            .text_size(px(12.0))
            .text_color(if muted {
                c.dialog_muted
            } else {
                c.text_default
            })
            .child(label)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        window.handle_input(
                            &focus_for_input,
                            ElementInputHandler::new(bounds, input_editor.clone()),
                            cx,
                        );
                    },
                )
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .left_0(),
            )
            .on_click(move |_event, window, _cx| window.focus(&focus_for_click))
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.to_ascii_lowercase();
                let secondary = event.keystroke.modifiers.secondary();
                match key.as_str() {
                    "escape" => {
                        let _ = editor.update(cx, |editor, cx| match kind {
                            SearchInputKind::Query => {
                                editor.workspace.search_query.clear();
                                editor.workspace.search_selected_range = 0..0;
                                editor.workspace.search_marked_range = None;
                                editor.workspace.active_tab = WorkspaceTab::Files;
                                editor.workspace.search_focus_pending = false;
                                editor.schedule_workspace_search(cx);
                            }
                            SearchInputKind::Replace => {
                                editor.workspace.replace_visible = false;
                            }
                        });
                    }
                    "a" if secondary => {
                        let _ = editor.update(cx, |editor, cx| {
                            match kind {
                                SearchInputKind::Query => {
                                    editor.workspace.search_selected_range =
                                        0..editor.workspace.search_query.len();
                                }
                                SearchInputKind::Replace => {
                                    editor.workspace.replace_selected_range =
                                        0..editor.workspace.replace_query.len();
                                }
                            }
                            cx.notify();
                        });
                    }
                    "backspace" => {
                        let handled = editor.update(cx, |editor, cx| {
                            let (text, selected, marked) = match kind {
                                SearchInputKind::Query => (
                                    editor.workspace.search_query.clone(),
                                    editor.workspace.search_selected_range.clone(),
                                    editor.workspace.search_marked_range.clone(),
                                ),
                                SearchInputKind::Replace => (
                                    editor.workspace.replace_query.clone(),
                                    editor.workspace.replace_selected_range.clone(),
                                    editor.workspace.replace_marked_range.clone(),
                                ),
                            };
                            if marked.is_some() {
                                return false;
                            }
                            let range = if selected.start == selected.end {
                                let before = &text[..selected.start];
                                let start = before
                                    .grapheme_indices(true)
                                    .last()
                                    .map(|(start, _)| start)
                                    .unwrap_or(selected.start);
                                start..selected.start
                            } else {
                                selected
                            };
                            editor.replace_overlay_input_text(kind, range, "", None, false, cx);
                            true
                        });
                        if !matches!(handled, Ok(true)) {
                            return;
                        }
                    }
                    "v" if secondary => {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            let _ = editor.update(cx, |editor, cx| {
                                let selected = match kind {
                                    SearchInputKind::Query => {
                                        editor.workspace.search_selected_range.clone()
                                    }
                                    SearchInputKind::Replace => {
                                        editor.workspace.replace_selected_range.clone()
                                    }
                                };
                                editor.replace_overlay_input_text(
                                    kind, selected, &text, None, false, cx,
                                );
                            });
                        }
                    }
                    "enter" => {
                        let reverse = event.keystroke.modifiers.shift;
                        let _ = editor.update(cx, |editor, cx| match kind {
                            SearchInputKind::Query => {
                                editor.advance_search_match(reverse, window, cx);
                            }
                            SearchInputKind::Replace => {
                                editor.replace_active_document_match(window, cx);
                            }
                        });
                    }
                    _ => return,
                }
                cx.stop_propagation();
            })
            .into_any_element()
    }
    pub(crate) fn render_search_results(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 只在还没有任何结果可显示时才用「…」占位：重新搜索期间继续显示上一次
        // 的结果，避免侧栏闪空（用户报修）。
        if self.workspace.search_pending && self.workspace.search_results.is_empty() {
            return div()
                .p(px(12.0))
                .text_size(px(14.0))
                .text_color(theme.colors.dialog_muted)
                .child("…")
                .into_any_element();
        }
        if self.workspace.search_results.is_empty() {
            return self.render_workspace_empty_state(
                "",
                if self.workspace.search_scope == WorkspaceSearchScope::Document {
                    &strings.workspace_no_document_find_results
                } else {
                    &strings.workspace_no_search_results
                },
                theme,
            );
        }
        let c = &theme.colors;
        let is_document_scope = self.workspace.search_scope == WorkspaceSearchScope::Document;
        // 行先摊平成描述符（文件头 / 命中行），再按视口取一窗：命中表封顶 200 条，
        // 但「一个文件一条命中」时就是 400 行元素（dev 构建实测一帧 62 ms），
        // 与文件树一样要按视口裁。
        let mut rows: Vec<(usize, bool)> = Vec::new();
        let mut current_file: Option<PathBuf> = None;
        for (index, hit) in self.workspace.search_results.iter().enumerate() {
            // Workspace scope groups hits under a file header row; document
            // scope lists matches flat with the file name on each row.
            if !is_document_scope && current_file.as_ref() != Some(&hit.path) {
                current_file = Some(hit.path.clone());
                rows.push((index, true));
            }
            // 文件名命中只由文件头代表：再渲染一行没有行号、没有预览的行
            // 只会得到一条看得见点不着（或看不见）的空条。
            if !is_document_scope && hit.line.is_none() {
                continue;
            }
            rows.push((index, false));
        }

        let window = self.workspace_list_window(rows.len());
        self.panel_rows_rendered.set(window.len() as u64);
        self.panel_first_row_rendered.set(window.start as u64);
        let needs_fill = rows.len() > PANEL_WINDOW_THRESHOLD_ROWS
            && f32::from(self.workspace.tree_scroll_handle.bounds().size.height) <= 0.0;
        let mut elements: Vec<AnyElement> = Vec::with_capacity(window.len() + 2);
        // 上下各垫一段等高空白：滚动条长度与位置仍按整张表算，中间只挂窗口里的行。
        if window.start > 0 {
            elements.push(
                div()
                    .h(px(window.start as f32 * WORKSPACE_NODE_HEIGHT))
                    .flex_shrink_0()
                    .into_any_element(),
            );
        }
        for (index, is_header) in &rows[window.clone()] {
            let index = *index;
            let hit = &self.workspace.search_results[index];
            if *is_header {
                let file_hit_count = self
                    .workspace
                    .search_results
                    .iter()
                    .filter(|other| other.path == hit.path)
                    .count();
                // 文件头整行可点击并代表该组第一条命中（用户报修：此前文件名
                // 本身点不了，只有它下面一条没有内容的空行能点）。文件名命中
                // 没有行号，点击就只是打开这个文件；内容命中则跳到该处匹配。
                let header_selected = self.workspace.search_active_index == Some(index);
                let header_editor = editor.clone();
                elements.push(
                    div()
                        .id(("workspace-search-file", index))
                        .debug_selector(move || format!("workspace-search-file-{index}"))
                        .h(px(WORKSPACE_NODE_HEIGHT))
                        .w_full()
                        .px(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .rounded(px(5.0))
                        .bg(if header_selected {
                            c.selection
                        } else {
                            hsla(0.0, 0.0, 0.0, 0.0)
                        })
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .child(
                            svg()
                                .path(if is_code_file(&hit.path) {
                                    CODE_ICON
                                } else {
                                    MARKDOWN_ICON
                                })
                                .size(px(13.0))
                                .text_color(c.dialog_primary_button_bg),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c.text_default)
                                .child(hit.label.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(c.dialog_muted)
                                .child(file_hit_count.to_string()),
                        )
                        .on_click(move |event, window, cx| {
                            if !event.standard_click() {
                                return;
                            }
                            let _ = header_editor.update(cx, |editor, cx| {
                                editor.open_search_hit(index, window, cx);
                            });
                        })
                        .into_any_element(),
                );
                continue;
            }
            let selected = self.workspace.search_active_index == Some(index);
            let hit_editor = editor.clone();
            let line_number = hit.line;
            let preview = hit.preview.clone();
            // 行高必须与 WORKSPACE_NODE_HEIGHT 一致：窗口按它算行号。文档范围
            // 原先在命中行前多挂一行文件名——同一篇文档里每行都是同一个名字
            // （标签页已经写着），两行高的列表按视口裁不出来，所以并成一行。
            elements.push(
                div()
                    .id(("workspace-search-hit", index))
                    .debug_selector(move || format!("workspace-search-hit-{index}"))
                    .h(px(WORKSPACE_NODE_HEIGHT))
                    .w_full()
                    .pl(px(if is_document_scope { 10.0 } else { 24.0 }))
                    .pr(px(6.0))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .rounded(px(5.0))
                    .bg(if selected {
                        c.selection
                    } else {
                        hsla(0.0, 0.0, 0.0, 0.0)
                    })
                    .cursor_pointer()
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .children(line_number.map(|line| {
                        div()
                            .flex()
                            .gap(px(6.0))
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(c.dialog_muted)
                                    .child(format!("{line}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(c.text_default)
                                    .child(preview),
                            )
                    }))
                    .on_click(move |event, window, cx| {
                        if !event.standard_click() {
                            return;
                        }
                        let _ = hit_editor.update(cx, |editor, cx| {
                            editor.open_search_hit(index, window, cx);
                        });
                    })
                    .into_any_element(),
            );
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
        let element = div()
            .w_full()
            .flex()
            .flex_col()
            .children(elements)
            .into_any_element();
        // 首帧还没量过滚动视口：先铺一小段，排下一帧补齐（与文件树/大纲同一手法）。
        if needs_fill && self.panel_fill_frames < PANEL_FILL_MAX_FRAMES {
            self.panel_fill_frames += 1;
            self.schedule_followup_frame(cx);
        } else if !needs_fill {
            self.panel_fill_frames = 0;
        }
        element
    }

    /// Opens the hit's match: document-scope hits select the byte range in the
    /// live document; workspace-scope hits open the file first, then select
    /// the match using its line/column information.
    pub(crate) fn open_search_hit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.workspace.search_results.get(index) else {
            return;
        };
        self.workspace.search_active_index = Some(index);
        if let Some(range) = hit.source_range.clone() {
            self.workspace.document_active_range = Some(range.clone());
            self.jump_to_document_search_range(range, cx);
            return;
        }
        let path = hit.path.clone();
        let match_ordinal = hit.match_ordinal;
        // 搜索结果是「点开看看」：开成预览标签，点下一条替换上一条，不堆标签栏
        // （用户需求）。改过的那篇由 `finish_dirty` 就地转固定，不会被替换掉。
        self.open_workspace_file_in_mode(path.clone(), WorkspaceOpenMode::Preview, window, cx);
        // 路径表示可能不一致（树扫描 canonicalize，打开路径未必；macOS
        // /var ↔ /private/var）：字面比较失败会让整个跳转块静默跳过——
        // 用户报修「点了完全没反应」。双方 canonicalize 后再比。
        let same_file = match (self.file_path.as_ref(), path.canonicalize()) {
            (Some(open), Ok(canonical)) => {
                open == &path
                    || open.canonicalize().map(|open| open == canonical).unwrap_or(false)
            }
            (Some(open), Err(_)) => open == &path,
            _ => false,
        };
        if same_file
            && let Some(ordinal) = match_ordinal
        {
            // 行号语义（用户定盘）：工作区搜索读磁盘，报的是磁盘行号。读磁盘
            // 就允许文档是脏的——未保存的编辑会把行往上或往下推，行号与字节
            // 就近都不可靠。但「文件内第 k 个含词行」的对应关系不灭：编辑要么
            // 增删含词行（那时命中本身也变了），要么只挪位置。
            // （用户报修：两个不同命中被解析到同一处，点击无反应。）
            //
            // 现在这层对应关系读的是文档命中表（§4.3 那张），取「第 k 个含命中
            // 起点的行」的第一个命中——与旧的手写循环对非跨行查询逐位相同，但
            // 跨行命中也能跳对，而且不再每次点击重扫整篇文档。
            //
            // 注意这与「读取侧换源到缓冲区」不是同一件事：文档内查找（⌘F）扫的
            // 是缓冲区，行号即文件行号；工作区扫描跨文件读磁盘，才需要这层对应。
            // 表里没有第 `ordinal` 组（未保存的编辑把那一行删掉了）时，
            // `document_range_for_line_ordinal` 自己就退回表里的第一个命中；
            // 表是空的说明这篇文档里已经没有命中了，此时不再有可退的位置——
            // 旧实现那句 `find_document_match_from` 兜底在这个前提下是死代码
            // （同一份文本、同一个引擎，取不到另一条结果），所以不再保留。
            let range = self.document_range_for_line_ordinal(ordinal, cx);
            // 区间出自命中表，两端一定在字符边界上（`search_engine.rs` 的硬闸门），
            // 旧这里那句冗余的边界核对已经跟着手写层一起退役。
            if let Some(range) = range {
                self.workspace.document_active_range = Some(range.clone());
                self.jump_to_document_search_range(range, cx);
            }
        }
    }
}



