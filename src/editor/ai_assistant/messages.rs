use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::base::TextView;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _,
    Icon, IconName,
    message::{Message, MessageAlignment, MessageContent},
    message_scroller::{MessageScroller, MessageScrollerState},
    text::{MarkdownExtensions, TextViewState},
};

mod paragraph;
mod rich;

struct MessageRow {
    source: Arc<str>,
    thinking: Arc<str>,
    user: bool,
    content: Option<MessageText>,
}

struct MessageText {
    state: Entity<TextViewState>,
    rich: rich::RichContent,
    _observation: Subscription,
}

pub(in crate::editor) struct MessageTranscript {
    owner: WeakEntity<Editor>,
    session_id: String,
    quick: bool,
    base: Option<PathBuf>,
    rows: Vec<MessageRow>,
    /// 每条 assistant 消息对应的工具调用，按 message_index 分组内联渲染。
    tool_calls: Vec<ToolCallRecord>,
    running: bool,
    show_thinking: bool,
    chain_open: std::collections::BTreeSet<usize>,
    thinking_open: std::collections::BTreeSet<usize>,
    scroll: Entity<MessageScrollerState>,
    highlight_theme: Arc<crate::theme::Theme>,
    code_highlighter: Arc<rich::CodeHighlighter>,
    metrics: Option<(
        Pixels,
        SharedString,
        SharedString,
        Pixels,
        SharedString,
        Pixels,
    )>,
}

impl MessageTranscript {
    pub(super) fn new(
        owner: WeakEntity<Editor>,
        saved: &SavedSession,
        show_thinking: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme = cx.global::<crate::theme::ThemeManager>().derived_theme(cx);
        let mut this = Self {
            owner,
            session_id: saved.id.clone(),
            quick: saved.kind == SessionKind::QuickCommand,
            base: saved.workspace.root.clone(),
            rows: Vec::new(),
            tool_calls: Vec::new(),
            running: saved.running,
            show_thinking,
            chain_open: Default::default(),
            thinking_open: Default::default(),
            scroll: cx.new(|cx| MessageScrollerState::new(0, cx)),
            code_highlighter: rich::code_highlighter(theme.clone()),
            highlight_theme: theme,
            metrics: None,
        };
        this.sync(saved, show_thinking, cx);
        this
    }

    pub(super) fn sync(
        &mut self,
        saved: &SavedSession,
        show_thinking: bool,
        cx: &mut Context<Self>,
    ) {
        if self.show_thinking != show_thinking {
            self.show_thinking = show_thinking;
            cx.notify();
        }
        if self.tool_calls != saved.tool_calls {
            self.tool_calls = saved.tool_calls.clone();
            cx.notify();
        }
        if self.running != saved.running {
            self.running = saved.running;
            cx.notify();
        }
        let old_count = self.rows.len();
        let mut changed = old_count != saved.messages.len();
        self.rows.truncate(saved.messages.len());
        for (index, message) in saved.messages.iter().enumerate() {
            if let Some(row) = self.rows.get_mut(index) {
                if !Arc::ptr_eq(&row.source, &message.text) {
                    changed = true;
                    row.source = message.text.clone();
                    if let Some(content) = &row.content {
                        content
                            .state
                            .update(cx, |state, cx| state.set_text(&message.text, cx));
                    }
                }
                if !Arc::ptr_eq(&row.thinking, &message.thinking) {
                    changed = true;
                    row.thinking = message.thinking.clone();
                }
                let user = message.role == "user";
                changed |= row.user != user;
                row.user = user;
            } else {
                self.rows.push(MessageRow {
                    source: message.text.clone(),
                    thinking: message.thinking.clone(),
                    user: message.role == "user",
                    content: None,
                });
            }
        }
        let new_count = self.rows.len();
        if new_count != old_count {
            self.scroll.update(cx, |state, cx| {
                if new_count == 0 && old_count > 0 {
                    state.reset(0, cx);
                } else if new_count > old_count {
                    state.append(new_count - old_count, cx);
                } else if new_count < old_count {
                    state.splice(new_count..old_count, 0, cx);
                }
            });
        }
        if changed {
            cx.notify();
        }
    }

    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let rows_len = self.rows.len();
        let records: Vec<ToolCallRecord> = self
            .tool_calls
            .iter()
            .filter(|record| record.message_index == index)
            .cloned()
            .collect();
        let (user, thinking, source_empty, body) = {
            let Some(row) = self.rows.get_mut(index) else {
                return div().into_any_element();
            };
            let thinking = row.thinking.clone();
            let user = row.user;
            let source_empty = row.source.is_empty();
            let shows_thinking = self.show_thinking && !thinking.is_empty();
            // 快捷指令只隐藏自动生成的首条指令；后续追问照常显示。
            if (source_empty && records.is_empty() && !shows_thinking)
                || (self.quick && user && index == 0)
            {
                return div().h_0().into_any_element();
            }
            if row.content.is_none() {
                let content = cx.new(|cx| TextViewState::markdown(&row.source, cx));
                let observation = cx.observe(&content, move |this, _, cx| {
                    this.scroll.update(cx, |state, cx| {
                        state.remeasure_items(index..index + 1, cx);
                    });
                });
                let rich = rich::extensions(
                    content.downgrade(),
                    cx.weak_entity(),
                    index,
                    self.base.clone(),
                );
                row.content = Some(MessageText {
                    state: content,
                    rich,
                    _observation: observation,
                });
            }
            let Some(content) = row.content.as_ref() else {
                return div().into_any_element();
            };
            let base = self.base.clone();
            let owner = cx.weak_entity();
            let text = TextView::new(&content.state)
                .selectable(true)
                .w_full()
                .markdown_extensions(content.rich.extensions())
                .shared_code_block_highlighter(self.code_highlighter.clone())
                .code_block_actions(|block, _, cx| {
                    gpui_kit::component::clipboard::Clipboard::new("ai-code-copy")
                        .value(block.code())
                        .accessibility_label(
                            cx.global::<crate::i18n::I18nManager>()
                                .strings()
                                .ai_panel_copy
                                .clone(),
                        )
                })
                .image_source(move |uri| rich::image_source(uri, base.as_deref()))
                .on_link_click(move |target, _, window, cx| {
                    if let Err(error) =
                        owner.update(cx, |this, cx| this.open_link(index, target, window, cx))
                    {
                        eprintln!("打开 AI 消息链接失败：{error}");
                    }
                });
            let body = MessageContent::new()
                .when(user, |body| {
                    body.rounded_lg()
                        .bg(cx.theme().muted)
                        .px_3()
                        .py_2()
                })
                .child(text);
            (user, thinking, source_empty, body)
        };
        // 模型还没吐字且仍在运行时，链条与思考保持展开给用户反馈；一旦开始
        // 输出正文就自动收起，用户仍可手动展开。
        let auto_open = self.running && !user && source_empty && index + 1 == rows_len;
        let chain =
            (!user && !records.is_empty()).then(|| {
                let theme = self.highlight_theme.clone();
                self.render_tool_chain(&theme, &records, auto_open, index, cx)
            });
        let thinking_block = (!user && self.show_thinking && !thinking.is_empty()).then(|| {
            let theme = self.highlight_theme.clone();
            self.render_thinking_block(&theme, &thinking, auto_open, index, cx)
        });
        let mut column = div().w_full().flex().flex_col();
        if let Some(thinking_block) = thinking_block {
            column = column.child(thinking_block);
        }
        if let Some(chain) = chain {
            column = column.child(chain);
        }
        // Message 是展示组件，外层提供可观察且稳定的消息身份。
        div()
            .id(SharedString::from(format!(
                "ai-message:{}:{index}",
                self.session_id
            )))
            .role(Role::ListItem)
            .test_support()
            // 用例要按浮窗宽度量这一行的实际排版边界（换行是否跟着宽度走）。
            .debug_selector(move || format!("ai-message-row-{index}"))
            .w_full()
            .pb_4()
            .child(column.child(
                Message::new()
                    .alignment(MessageAlignment::Start)
                    .content(body),
            ))
            .into_any_element()
    }

    /// 工具调用链条：模型没吐字时逐步展示每次调用，一行一条；正文开始后
    /// 自动收起，只剩一行可展开的标题。
    fn render_tool_chain(
        &mut self,
        theme: &crate::theme::Theme,
        records: &[ToolCallRecord],
        auto_open: bool,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.chain_open.contains(&index) || auto_open;
        let header = div()
            .id(SharedString::from(format!("ai-tool-chain:{index}")))
            .debug_selector(|| "ai-tool-chain".into())
            .flex()
            .items_center()
            .gap(px(4.0))
            .py(px(2.0))
            .text_size(px(11.0))
            .text_color(theme.colors.dialog_muted)
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !this.chain_open.remove(&index) {
                    this.chain_open.insert(index);
                }
                cx.notify();
            }))
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .text_color(theme.colors.dialog_muted),
            )
            .child(format!(
                "{} · {}",
                cx.global::<crate::i18n::I18nManager>()
                    .strings()
                    .ai_tool_trace
                    .clone(),
                records.len()
            ));
        let mut section = div()
            .mb(px(4.0))
            .flex()
            .flex_col()
            .child(header);
        if open {
            let mut body = div()
                .id("ai-tool-chain-body")
                .debug_selector(|| "ai-tool-chain-body".into())
                .flex()
                .flex_col()
                .gap(px(2.0))
                .max_h(px(200.0))
                .overflow_y_scroll();
            for (offset, record) in records.iter().enumerate() {
                body = body.child(Self::render_tool_chain_row(theme, record, offset));
            }
            section = section.child(body);
        }
        section.into_any_element()
    }

    fn render_tool_chain_row(
        theme: &crate::theme::Theme,
        record: &ToolCallRecord,
        index: usize,
    ) -> AnyElement {
        use crate::ai::session::ToolCallStatus;
        let status_icon = match record.status {
            ToolCallStatus::Running => Spinner::new().small().into_any_element(),
            ToolCallStatus::Succeeded => Icon::new(IconName::CircleCheck)
                .text_color(theme.colors.callout_tip_border)
                .into_any_element(),
            ToolCallStatus::Failed => Icon::new(IconName::CircleX)
                .text_color(theme.colors.callout_warning_border)
                .into_any_element(),
        };
        div()
            .id(SharedString::from(format!("ai-tool-call-{index}")))
            .debug_selector(move || format!("ai-tool-call-{index}"))
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(6.0))
            .py(px(3.0))
            .rounded(px(4.0))
            .text_size(px(11.0))
            .text_color(theme.colors.dialog_muted)
            .child(status_icon)
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::MEDIUM)
                    .child(record.name.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .child(tool_trace_preview(&record.arguments, 120)),
            )
            .into_any_element()
    }

    /// 思考过程：灰色小字，默认随运行展开、正文开始后收起，可手动展开。
    fn render_thinking_block(
        &mut self,
        theme: &crate::theme::Theme,
        thinking: &str,
        auto_open: bool,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.thinking_open.contains(&index) || auto_open;
        let header = div()
            .id(SharedString::from(format!("ai-thinking:{index}")))
            .debug_selector(|| "ai-thinking".into())
            .flex()
            .items_center()
            .gap(px(4.0))
            .py(px(2.0))
            .text_size(px(11.0))
            .text_color(theme.colors.dialog_muted)
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !this.thinking_open.remove(&index) {
                    this.thinking_open.insert(index);
                }
                cx.notify();
            }))
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .text_color(theme.colors.dialog_muted),
            )
            .child(
                cx.global::<crate::i18n::I18nManager>()
                    .strings()
                    .ai_thinking_title
                    .clone(),
            );
        let mut section = div().mb(px(4.0)).flex().flex_col().child(header);
        if open {
            section = section.child(
                div()
                    .id("ai-thinking-body")
                    .debug_selector(|| "ai-thinking-body".into())
                    .text_size(px(11.0))
                    .text_color(theme.colors.dialog_muted)
                    .line_height(relative(1.5))
                    .whitespace_normal()
                    .max_h(px(160.0))
                    .overflow_y_scroll()
                    .child(thinking.to_owned()),
            );
        }
        section.into_any_element()
    }

    pub(in crate::editor) fn open_link(
        &mut self,
        index: usize,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::editor::window_state::{LinkTarget, classify_link_target};
        use crate::file_url::resolve_local_link_path;
        let target = match classify_link_target(target) {
            LinkTarget::External(url) => url,
            LinkTarget::Anchor(anchor) => {
                let Some(row) = self.rows.get(index) else {
                    return;
                };
                let Some(content) = &row.content else {
                    return;
                };
                let source = row.source.clone();
                let state = content.state.clone();
                cx.defer(move |cx| {
                    let Some(line) = crate::export::html::heading_line_for_anchor(&source, &anchor)
                    else {
                        return;
                    };
                    let start = source
                        .split_inclusive('\n')
                        .take(line)
                        .map(str::len)
                        .sum::<usize>();
                    let end = source
                        .get(start..)
                        .and_then(|line| line.find('\n'))
                        .map_or(source.len(), |end| start + end);
                    let range = state.read(cx).rendered_text().range_for_source(start..end);
                    if let Some(range) = range {
                        state.update(cx, |state, cx| {
                            if let Err(error) = state.reveal_range(range, cx) {
                                eprintln!("定位 AI 消息标题失败：{error}");
                            }
                        });
                    }
                });
                return;
            }
            LinkTarget::Local { path, anchor } => {
                let Some(base) = &self.base else {
                    return;
                };
                let path = resolve_local_link_path(Some(base), &path);
                // 用 `canonical_path`：它在 Windows 上会剥掉 `\\?\` 前缀，
                // 而 `Url::from_file_path` 不认那个前缀——不剥就整条打不开。
                let path = crate::file_url::canonical_path(&path);
                let inside = path.starts_with(base)
                    || path.starts_with(crate::file_url::strip_verbatim_prefix(base));
                if !inside {
                    return;
                }
                let Ok(mut url) = url::Url::from_file_path(path) else {
                    return;
                };
                url.set_fragment(anchor.as_deref());
                url.to_string()
            }
        };
        let owner = self.owner.clone();
        let handle = window.window_handle();
        cx.defer(move |cx| {
            match handle.update(cx, |_, window, cx| {
                owner.update(cx, |editor, cx| editor.open_link_target(target, window, cx))
            }) {
                Ok(Ok(())) => {}
                Ok(Err(error)) | Err(error) => eprintln!("打开 AI 资料失败：{error}"),
            }
        });
    }

    pub(super) fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.rows
            .iter()
            .filter_map(|row| row.content.as_ref())
            .any(|content| {
                content.state.read(cx).focus_handle().is_focused(window)
                    || content.rich.contains_focus(window, cx)
            })
    }

    #[cfg(test)]
    pub(in crate::editor) fn message_state(&self, index: usize) -> Option<Entity<TextViewState>> {
        self.rows
            .get(index)?
            .content
            .as_ref()
            .map(|content| content.state.clone())
    }
}

/// 轨迹预览只取前 `limit` 个字符：完整参数与结果留在对话历史里。
fn tool_trace_preview(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

impl Render for MessageTranscript {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.global::<crate::theme::ThemeManager>().derived_theme(cx);
        if !Arc::ptr_eq(&self.highlight_theme, &palette) {
            self.code_highlighter = rich::code_highlighter(palette.clone());
            self.highlight_theme = palette;
        }
        let theme = cx.theme();
        let fonts = crate::config::EditorSettings::fonts(cx);
        let family: SharedString = if fonts.markdown_family == "theme" {
            self.highlight_theme
                .typography
                .body_font_family
                .clone()
                .into()
        } else {
            fonts.markdown_family.into()
        };
        let body_size = px(self.highlight_theme.typography.text_size);
        let metrics = (
            window.rem_size(),
            theme.font_family.clone(),
            theme.mono_font_family.clone(),
            theme.mono_font_size,
            family.clone(),
            body_size,
        );
        let remeasured = self
            .metrics
            .as_ref()
            .is_some_and(|previous| *previous != metrics);
        if remeasured {
            self.scroll.update(cx, |state, cx| state.remeasure(cx));
        }
        self.metrics = Some(metrics);
        let owner = cx.weak_entity();
        let scroll = MessageScroller::new(
            "ai-native-message-list",
            self.scroll.clone(),
            move |index, _, cx| {
                owner
                    .update(cx, |this, cx| this.render_row(index, cx))
                    .unwrap_or_else(|error| {
                        eprintln!("读取 AI 消息失败：{error}");
                        div().into_any_element()
                    })
            },
        )
        .with_row_style(StyleRefinement::default().pb_0().pl_0())
        .with_jump_button_label(
            cx.global::<crate::i18n::I18nManager>()
                .strings()
                .ai_panel_latest
                .clone(),
        );
        div()
            .id("ai-message-stream")
            .role(Role::Group)
            .test_support()
            .size_full()
            .min_h_0()
            .min_w_0()
            .font(font(family))
            .text_size(body_size)
            .text_color(self.highlight_theme.colors.text_default)
            .line_height(relative(self.highlight_theme.typography.text_line_height))
            .child(scroll)
    }
}
