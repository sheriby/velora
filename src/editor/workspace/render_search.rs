use super::*;
use gpui::prelude::FluentBuilder;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _,
    button::{Button, ButtonGroup, ButtonVariants},
    input::{Enter as InputEnter, Escape as InputEscape, Input, InputEvent, InputState},
    list::ListItem,
    spinner::Spinner,
    tooltip::Tooltip,
};

impl WorkspaceSearchHitKey {
    fn row_id(&self, header: bool) -> u64 {
        use std::hash::{Hash as _, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (self, header).hash(&mut hasher);
        hasher.finish()
    }
}

impl Editor {

    pub(super) fn search_input_state(&self, kind: SearchInputKind) -> Option<&SearchInputState> {
        match kind {
            SearchInputKind::Query => self.workspace.search_input_state.as_ref(),
            SearchInputKind::Replace => self.workspace.replace_input_state.as_ref(),
        }
    }

    pub(super) fn sync_workspace_search_input(
        &mut self,
        kind: SearchInputKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let value = match kind {
            SearchInputKind::Query => self.workspace.search_query.clone(),
            SearchInputKind::Replace => self.workspace.replace_query.clone(),
        };
        if let Some(field) = self.search_input_state(kind) {
            let input = field.input.clone();
            if field.model_value != value {
                // 外部跨行查询折叠为单行显示，匹配串仍由领域模型保留。
                input.update(cx, |input, cx| {
                    input.set_value(search_input_display_value(&value), window, cx)
                });
                let field = match kind {
                    SearchInputKind::Query => self.workspace.search_input_state.as_mut(),
                    SearchInputKind::Replace => self.workspace.replace_input_state.as_mut(),
                };
                if let Some(field) = field {
                    field.model_value = value;
                }
            }
            return input;
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx).default_value(search_input_display_value(&value))
        });
        let focus = input.read(cx).focus_handle(cx);
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |editor, source, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Focus) {
                    editor.workspace.search_focus_pending = false;
                    editor.workspace.search_select_all_pending = false;
                    editor.pending_focus = None;
                }
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let value = source.read(cx).value().to_string();
                let field = match kind {
                    SearchInputKind::Query => editor.workspace.search_input_state.as_mut(),
                    SearchInputKind::Replace => editor.workspace.replace_input_state.as_mut(),
                };
                let Some(field) = field.filter(|field| field.input == *source) else {
                    return;
                };
                field.model_value = value.clone();
                match kind {
                    SearchInputKind::Query => {
                        if editor.workspace.search_query != value {
                            editor.workspace.search_query = value;
                            editor.schedule_workspace_search(cx);
                        }
                    }
                    SearchInputKind::Replace => editor.workspace.replace_query = value,
                }
                cx.notify();
            },
        );
        let field = SearchInputState {
            input: input.clone(),
            model_value: value,
            _subscription: subscription,
        };
        match kind {
            SearchInputKind::Query => {
                self.workspace.search_input_state = Some(field);
                self.workspace.search_focus = Some(focus);
            }
            SearchInputKind::Replace => {
                self.workspace.replace_input_state = Some(field);
                self.workspace.replace_focus = Some(focus);
            }
        }
        input
    }

    pub(super) fn render_search_input(
        &mut self,
        kind: SearchInputKind,
        placeholder: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = self.sync_workspace_search_input(kind, window, cx);
        let id = match kind {
            SearchInputKind::Query => "workspace-search-query",
            SearchInputKind::Replace => "workspace-search-replace",
        };
        if input.read(cx).presentation().placeholder().as_ref() != placeholder.as_str() {
            input.update(cx, |input, cx| {
                input.set_placeholder(placeholder.clone(), window, cx)
            });
        }
        let paste_input = input.clone();
        div()
            .id(("workspace-search-input", input.entity_id()))
            .cursor(CursorStyle::IBeam)
            .debug_selector(move || id.to_string())
            .flex_1()
            .min_w_0()
            .capture_any_mouse_down(cx.listener(|editor, _, _, _| {
                editor.workspace.search_focus_pending = false;
                editor.workspace.search_select_all_pending = false;
                editor.pending_focus = None;
            }))
            .child(
                Input::new(&input)
                    .id(id)
                    .accessibility_id(id)
                    .aria_label(placeholder.clone())
                    .small()
                    .on_paste(move |clipboard, window, cx| {
                        let Some(text) =
                            clipboard.text().filter(|text| text.contains(['\r', '\n']))
                        else {
                            return false;
                        };
                        paste_input.update(cx, |input, cx| {
                            input.replace(search_input_display_value(&text), window, cx)
                        });
                        true
                    }),
            )
            .on_action(cx.listener(move |editor, action: &InputEnter, window, cx| {
                let Some(input) = editor
                    .search_input_state(kind)
                    .map(|field| field.input.clone())
                else {
                    return;
                };
                if input.update(cx, |input, cx| {
                    input.marked_text_range(window, cx).is_some()
                }) {
                    return;
                }
                match kind {
                    SearchInputKind::Query => editor.advance_search_match(action.shift, window, cx),
                    SearchInputKind::Replace => {
                        editor.replace_current_search_match(window, cx);
                    }
                }
            }))
            .on_action(cx.listener(move |editor, _: &InputEscape, window, cx| {
                match kind {
                    SearchInputKind::Query => {
                        editor.workspace.search_query.clear();
                        editor.workspace.search_focus_pending = false;
                        editor.workspace.search_select_all_pending = false;
                        editor.workspace.active_tab = WorkspaceTab::Files;
                        editor.schedule_workspace_search(cx);
                        editor.focus_workspace_tree(window, cx);
                    }
                    SearchInputKind::Replace => {
                        editor.workspace.replace_visible = false;
                        if let Some(focus) = editor.workspace.search_focus.as_ref() {
                            window.focus(focus, cx);
                        }
                    }
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// VS Code-style search header: query input, collapsible replace input,
    /// option toggles, and a document/workspace scope switch. Rendered above
    /// the result list when the Search tab is active.
    pub(crate) fn render_search_header(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let search_input = self.render_search_input(SearchInputKind::Query,
            strings.workspace_search_placeholder.clone(), window, cx);
        let replace_visible = self.workspace.replace_visible && !self.is_read_only();
        let replace_input = replace_visible.then(|| self.render_search_input(SearchInputKind::Replace,
            strings.search_replace_placeholder.clone(), window, cx));

        let toggle_button = Button::new("workspace-search-toggle-replace")
            .debug_selector(|| "workspace-search-toggle-replace".to_string())
            .small()
            .ghost()
            .icon(if replace_visible {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .selected(replace_visible)
            .toggled(replace_visible)
            .accessibility_label(strings.search_replace_placeholder.clone())
            .tooltip(strings.search_replace_placeholder.clone())
            .on_click(cx.listener(|editor, _, _, cx| {
                if editor.is_read_only() {
                    return;
                }
                editor.workspace.replace_visible = !editor.workspace.replace_visible;
                cx.notify();
            }));

        let options_row = self.render_search_options_row(theme, strings, cx);

        let header = div()
            .id("workspace-search-header")
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .px_2()
            .pt_2()
            .pb_1()
            .border_b_1()
            .border_color(c.dialog_border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(search_input)
                    .when(!self.is_read_only(), |this| this.child(toggle_button)),
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
        let summary = message.lines().next().unwrap_or(message.as_str()).to_string();
        let detail = message.clone();
        Some(
            div()
                .id("workspace-search-pattern-error")
                .test_support()
                .role(Role::Alert)
                .aria_label(format!("{}{summary}", strings.search_invalid_pattern))
                .w_full()
                .px_0p5()
                .text_xs()
                .text_color(theme.colors.dialog_danger_button_bg)
                .truncate()
                .child(format!("{}{summary}", strings.search_invalid_pattern))
                .tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
                .into_any_element(),
        )
    }

    pub(crate) fn render_search_options_row(
        &mut self,
        _theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let option = |id: &'static str, icon, selected, label: String| {
            Button::new(id)
                .debug_selector(move || id.to_string())
                .xsmall()
                .ghost()
                .icon(icon)
                .selected(selected)
                .toggled(selected)
                .accessibility_label(label.clone())
                .tooltip(label)
        };
        let mut controls = div()
            .id("workspace-search-options")
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .child(
                option(
                    "workspace-search-case",
                    IconName::CaseSensitive,
                    self.workspace.search_match_case,
                    strings.search_case_sensitive.clone(),
                )
                .on_click(cx.listener(|editor, _, _, cx| {
                    editor.workspace.search_match_case = !editor.workspace.search_match_case;
                    editor.schedule_workspace_search(cx);
                    cx.notify();
                })),
            )
            .child(
                option(
                    "workspace-search-word",
                    IconName::WholeWord,
                    self.workspace.search_whole_word,
                    strings.search_whole_word.clone(),
                )
                .on_click(cx.listener(|editor, _, _, cx| {
                    editor.workspace.search_whole_word = !editor.workspace.search_whole_word;
                    editor.schedule_workspace_search(cx);
                    cx.notify();
                })),
            )
            .child(
                option(
                    "workspace-search-regex",
                    IconName::Regex,
                    self.workspace.search_use_regex,
                    format!("{} — {}", strings.search_regex, strings.search_regex_hint),
                )
                .on_click(cx.listener(|editor, _, _, cx| {
                    editor.workspace.search_use_regex = !editor.workspace.search_use_regex;
                    editor.schedule_workspace_search(cx);
                    cx.notify();
                })),
            )
            .child(
                Button::new("workspace-search-fuzzy")
                    .debug_selector(|| "workspace-search-fuzzy".to_string())
                    .xsmall()
                    .ghost()
                    .label(strings.search_fuzzy_short.clone())
                    .selected(self.workspace.search_fuzzy)
                    .toggled(self.workspace.search_fuzzy)
                    .accessibility_label(strings.search_fuzzy.clone())
                    .tooltip(strings.search_fuzzy.clone())
                    .on_click(cx.listener(|editor, _, _, cx| {
                        editor.workspace.search_fuzzy = !editor.workspace.search_fuzzy;
                        editor.schedule_workspace_search(cx);
                        cx.notify();
                    })),
            );
        let scope = self.workspace.search_scope;
        if self.workspace.replace_visible && !self.is_read_only() {
            let replace_row = div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_1()
                .children((scope == WorkspaceSearchScope::Document).then(|| {
                    Button::new("workspace-search-replace-current")
                        .debug_selector(|| "workspace-search-replace-current".to_string())
                        .xsmall()
                        .ghost()
                        .icon(IconName::Replace)
                        .accessibility_label(strings.search_replace_current.clone())
                        .tooltip(strings.search_replace_current.clone())
                        .on_click(cx.listener(|editor, _, window, cx| {
                            editor.replace_current_search_match(window, cx);
                        }))
                }))
                .child(
                    Button::new("workspace-search-replace-all")
                        .debug_selector(|| "workspace-search-replace-all".to_string())
                        .xsmall()
                        .ghost()
                        .icon(IconName::ReplaceAll)
                        .accessibility_label(strings.search_replace_all.clone())
                        .tooltip(strings.search_replace_all.clone())
                        .on_click(cx.listener(|editor, _, _, cx| {
                            editor.request_replace_all_matches(cx);
                        })),
                );
            controls = controls.child(div().flex_1().min_w_0()).child(replace_row);
        }
        let scope_button =
            |id, label: String, selected| Button::new(id).label(label).selected(selected);
        let scope_switch = ButtonGroup::new("workspace-search-scope")
            .xsmall()
            .ghost()
            .compact()
            .child(
                scope_button(
                    "workspace-scope-document",
                    strings.search_scope_document.clone(),
                    scope == WorkspaceSearchScope::Document,
                )
                .on_click(cx.listener(|editor, _, _, cx| {
                    editor.set_workspace_search_scope(WorkspaceSearchScope::Document, cx);
                })),
            )
            .child(
                scope_button(
                    "workspace-scope-workspace",
                    strings.search_scope_workspace.clone(),
                    scope == WorkspaceSearchScope::Workspace,
                )
                .on_click(cx.listener(|editor, _, _, cx| {
                    editor.set_workspace_search_scope(WorkspaceSearchScope::Workspace, cx);
                })),
            );
        let match_count = self.workspace.search_results.len();
        let count_label = (match_count > 0 && scope == WorkspaceSearchScope::Document).then(|| {
            strings
                .search_result_count
                .replace("{n}", &match_count.to_string())
        });
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(controls)
            .child(
                div()
                    .id("workspace-search-scope-row")
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(scope_switch)
                    .child(div().flex_1().min_w_0().flex().justify_end().children(
                        count_label.map(|label| {
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(label)
                        }),
                    )),
            )
            .into_any_element()
    }

    fn set_workspace_search_scope(&mut self, scope: WorkspaceSearchScope, cx: &mut Context<Self>) {
        self.workspace.search_scope = scope;
        self.workspace.search_active_index = None;
        self.workspace.document_active_range = None;
        self.schedule_workspace_search(cx);
        cx.notify();
    }

    pub(crate) fn render_search_results(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = if self.workspace.search_error.is_some() {
            Some(div().into_any_element())
        } else if self.workspace.search_pending && self.workspace.search_results.is_empty() {
            Some(
                div()
                    .id("workspace-search-loading")
                    .test_support()
                    .role(Role::ProgressIndicator)
                    .aria_label("Searching")
                    .p_3()
                    .child(Spinner::new().small())
                    .into_any_element(),
            )
        } else if self.workspace.search_results.is_empty() {
            let message = if self.workspace.search_scope == WorkspaceSearchScope::Document {
                &strings.workspace_no_document_find_results
            } else {
                &strings.workspace_no_search_results
            };
            Some(
                div()
                    .id("workspace-search-empty")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(message.clone())
                    .size_full()
                    .child(self.render_workspace_empty_state("", message, theme))
                    .into_any_element(),
            )
        } else {
            None
        };
        let total = if state.is_some() {
            0
        } else {
            self.workspace_search_rows().len()
        };
        let viewport = self.render_workspace_virtual_list(total, None, theme, cx);
        // 空态仍保留滚动所有者，数据收缩后的偏移交给 Kit 钳制。
        if let Some(state) = state {
            div()
                .relative()
                .size_full()
                .min_h_0()
                .child(viewport)
                .child(div().absolute().inset_0().child(state))
                .into_any_element()
        } else {
            viewport
        }
    }

    fn workspace_search_rows(&self) -> Vec<(usize, bool)> {
        let document_scope = self.workspace.search_scope == WorkspaceSearchScope::Document;
        let mut rows = Vec::new();
        let mut previous_file: Option<&Path> = None;
        for (index, hit) in self.workspace.search_results.iter().enumerate() {
            if !document_scope && previous_file != Some(hit.path.as_path()) {
                previous_file = Some(hit.path.as_path());
                rows.push((index, true));
            }
            if document_scope || hit.line.is_some() {
                rows.push((index, false));
            }
        }
        rows
    }

    pub(super) fn render_workspace_search_rows(
        &mut self,
        range: Range<usize>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let rows = self.workspace_search_rows();
        range
            .filter_map(|index| rows.get(index).copied())
            .filter_map(|(index, header)| {
                let hit = self.workspace.search_results.get(index)?;
                let key = WorkspaceSearchHitKey::new(hit);
                let prefix = if header {
                    "workspace-search-file"
                } else {
                    "workspace-search-hit"
                };
                let selected = self.workspace.search_active_index == Some(index)
                    && (!header || hit.line.is_none());
                let content =
                    if header {
                        let markdown_document = is_markdown_document(&hit.path);
                        let icon =
                            if crate::components::Block::has_supported_image_extension(&hit.path) {
                                IMAGE_ICON
                            } else if is_code_file(&hit.path) {
                                CODE_ICON
                            } else if markdown_document {
                                MARKDOWN_ICON
                            } else {
                                GENERIC_FILE_ICON
                            };
                        div()
                            .flex()
                            .items_center()
                            .min_w_0()
                            .w_full()
                            .gap_1()
                            .child(svg().path(icon).size_4().flex_none().text_color(
                                if markdown_document {
                                    theme.colors.dialog_primary_button_bg
                                } else {
                                    theme.colors.dialog_muted
                                },
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(hit.label.clone()),
                            )
                            .child(
                                div().text_color(theme.colors.dialog_muted).child(
                                    self.workspace
                                        .search_results
                                        .iter()
                                        .filter(|candidate| candidate.path == hit.path)
                                        .count()
                                        .to_string(),
                                ),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .items_center()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .when(
                                self.workspace.search_scope == WorkspaceSearchScope::Workspace,
                                |row| row.pl_4(),
                            )
                            .children(hit.line.map(|line| {
                                div()
                                    .text_color(theme.colors.dialog_muted)
                                    .child(line.to_string())
                            }))
                            .child(div().flex_1().min_w_0().truncate().child(
                                if hit.line.is_some() {
                                    hit.preview.clone()
                                } else {
                                    hit.label.clone()
                                },
                            ))
                            .into_any_element()
                    };
                let label = hit
                    .line
                    .map(|line| format!("{}:{line} {}", hit.label, hit.preview))
                    .unwrap_or_else(|| hit.label.clone());
                Some(
                    ListItem::new((prefix, key.row_id(header)))
                        .debug_selector(move || format!("{prefix}-{index}"))
                        .role(Role::ListItem)
                        .aria_selected(selected)
                        .accessibility_label(label)
                        .selected(selected)
                        .h(px(Self::workspace_node_height(theme)))
                        .w_full()
                        .py_0()
                        .px_2()
                        .text_xs()
                        .rounded(cx.theme().radius)
                        .child(content)
                        .on_click(cx.listener(move |editor, event: &ClickEvent, window, cx| {
                            if !event.standard_click() {
                                return;
                            }
                            if let Some(index) = editor
                                .workspace
                                .search_results
                                .iter()
                                .position(|hit| key.matches(hit))
                            {
                                editor.open_search_hit(index, window, cx);
                            }
                        }))
                        .into_any_element(),
                )
            })
            .collect()
    }

    pub(super) fn reveal_workspace_search_hit(&self, index: usize) {
        if !self.workspace.is_open || self.workspace.active_tab != WorkspaceTab::Search {
            return;
        }
        let rows = self.workspace_search_rows();
        let row = rows
            .iter()
            .position(|(hit, header)| *hit == index && !header)
            .or_else(|| rows.iter().position(|(hit, _)| *hit == index));
        if let Some(row) = row
            && let Some(handle) = self.workspace.tree_virtual_scroll.as_ref()
        {
            handle.scroll_to_item(row, ScrollStrategy::Nearest);
        }
    }

    pub(super) fn navigate_workspace_search_results(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let count = self.workspace.search_results.len();
        if !matches!(key, "up" | "down" | "home" | "end") {
            return false;
        }
        if count == 0 {
            return true;
        }
        let selected = self
            .workspace
            .search_active_index
            .filter(|index| *index < count);
        let next = match key {
            "home" => 0,
            "end" => count.saturating_sub(1),
            "up" => selected.unwrap_or(0).saturating_sub(1),
            _ => selected
                .map(|index| (index + 1).min(count.saturating_sub(1)))
                .unwrap_or(0),
        };
        self.workspace.search_active_index = Some(next);
        self.reveal_workspace_search_hit(next);
        cx.notify();
        true
    }

    /// Opens the hit's match: document-scope hits select the byte range in the
    /// live document; workspace-scope hits open the file first, then select
    /// the match using its line/column information.
    pub(crate) fn open_search_hit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.workspace.search_results.get(index) else {
            return;
        };
        self.reveal_workspace_search_hit(index);
        self.workspace.document_match_jump_pending = false;
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
        // /var ↔ /private/var）：字面比较失败会让整个跳转块静默跳过。
        // 双方 canonicalize 后再比。
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
            // （旧解析曾把两个不同命中解析到同一处，导致点击无反应。）
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
        // 仅文件名命中没有正文范围跳转，也需要归还查询焦点。
        if same_file
            && self.workspace.is_open
            && self.workspace.active_tab == WorkspaceTab::Search
            && !self.modal_is_open()
        {
            self.workspace.search_focus_pending = true;
            cx.notify();
        }
    }
}

fn search_input_display_value(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}
