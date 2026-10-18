use super::*;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::base::TextView;
use gpui_kit::component::{
    ActiveTheme as _,
    message::{Message, MessageAlignment, MessageContent},
    message_scroller::{MessageScroller, MessageScrollerState},
    text::{MarkdownExtensions, TextViewState},
};

mod paragraph;
mod rich;

struct MessageRow {
    source: Arc<str>,
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
        cx: &mut Context<Self>,
    ) -> Self {
        let theme = cx.global::<crate::theme::ThemeManager>().derived_theme(cx);
        let mut this = Self {
            owner,
            session_id: saved.id.clone(),
            quick: saved.kind == SessionKind::QuickCommand,
            base: saved.workspace.root.clone(),
            rows: Vec::new(),
            scroll: cx.new(|cx| MessageScrollerState::new(0, cx)),
            code_highlighter: rich::code_highlighter(theme.clone()),
            highlight_theme: theme,
            metrics: None,
        };
        this.sync(saved, cx);
        this
    }

    pub(super) fn sync(&mut self, saved: &SavedSession, cx: &mut Context<Self>) {
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
                let user = message.role == "user";
                changed |= row.user != user;
                row.user = user;
            } else {
                self.rows.push(MessageRow {
                    source: message.text.clone(),
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
        let Some(row) = self.rows.get_mut(index) else {
            return div().into_any_element();
        };
        if row.source.is_empty() || (self.quick && row.user) {
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
            })
            .w_full();
        let body = MessageContent::new()
            .when(row.user, |body| {
                body.max_w(relative(0.9))
                    .rounded_lg()
                    .bg(cx.theme().muted)
                    .px_3()
                    .py_2()
            })
            .child(text);
        // Message 是展示组件，外层提供可观察且稳定的消息身份。
        div()
            .id(SharedString::from(format!(
                "ai-message:{}:{index}",
                self.session_id
            )))
            .role(Role::ListItem)
            .test_support()
            .w_full()
            .pb_4()
            .child(
                Message::new()
                    .alignment(if row.user {
                        MessageAlignment::End
                    } else {
                        MessageAlignment::Start
                    })
                    .content(body),
            )
            .into_any_element()
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
                let path = match path.canonicalize() {
                    Ok(path) if path.starts_with(base) => path,
                    Ok(_) => return,
                    Err(error) => {
                        eprintln!("读取 AI 资料链接失败：{error}");
                        return;
                    }
                };
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
        if self
            .metrics
            .as_ref()
            .is_some_and(|previous| *previous != metrics)
        {
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
