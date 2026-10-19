use super::*;
use gpui::prelude::FluentBuilder;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _,
    button::{Button, ButtonGroup, ButtonVariants},
    spinner::Spinner,
    tooltip::Tooltip,
};

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
        let replace_visible = self.workspace.replace_visible && !self.is_read_only();
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
        editor: &WeakEntity<Editor>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.workspace.search_error.is_some() {
            return div().into_any_element();
        }
        // 只在还没有任何结果可显示时才用「…」占位：重新搜索期间继续显示上一次
        // 的结果，避免侧栏闪空。
        if self.workspace.search_pending && self.workspace.search_results.is_empty() {
            return div()
                .id("workspace-search-loading")
                .role(Role::ProgressIndicator)
                .aria_label("Searching")
                .p_3()
                .child(Spinner::new().small())
                .into_any_element();
        }
        if self.workspace.search_results.is_empty() {
            let message = if self.workspace.search_scope == WorkspaceSearchScope::Document {
                &strings.workspace_no_document_find_results
            } else {
                &strings.workspace_no_search_results
            };
            return div()
                .id("workspace-search-empty")
                .test_support()
                .role(Role::Status)
                .aria_label(message.clone())
                .size_full()
                .child(self.render_workspace_empty_state("", message, theme))
                .into_any_element();
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

        let total = rows.len();
        let window = self.workspace_list_window(total, theme);
        let mut visible: Vec<AnyElement> = Vec::with_capacity(window.len());
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
                // 文件头整行可点击并代表该组第一条命中：此前文件名本身点不了，
                // 只有它下面一条没有内容的空行能点。文件名命中没有行号，
                // 点击就只是打开这个文件；内容命中则跳到该处匹配。
                let header_selected = self.workspace.search_active_index == Some(index);
                let header_editor = editor.clone();
                visible.push(
                    div()
                        .id(("workspace-search-file", index))
                        .debug_selector(move || format!("workspace-search-file-{index}"))
                        .h(px(Self::workspace_node_height(theme)))
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
                                .text_size(px(theme.typography.ui_text_size(11.0)))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c.text_default)
                                .child(hit.label.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(theme.typography.ui_text_size(10.0)))
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
            // 行高必须与 workspace_node_height 一致：窗口按它算行号。文档范围
            // 原先在命中行前多挂一行文件名——同一篇文档里每行都是同一个名字
            // （标签页已经写着），两行高的列表按视口裁不出来，所以并成一行。
            visible.push(
                div()
                    .id(("workspace-search-hit", index))
                    .debug_selector(move || format!("workspace-search-hit-{index}"))
                    .h(px(Self::workspace_node_height(theme)))
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
                                    .text_size(px(theme.typography.ui_text_size(11.0)))
                                    .text_color(c.dialog_muted)
                                    .child(format!("{line}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(px(theme.typography.ui_text_size(11.0)))
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
        self.workspace_windowed_body(total, window, visible, theme, cx)
    }

    /// Opens the hit's match: document-scope hits select the byte range in the
    /// live document; workspace-scope hits open the file first, then select
    /// the match using its line/column information.
    pub(crate) fn open_search_hit(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.workspace.search_results.get(index) else {
            return;
        };
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
    }
}



