//! 只读代码文件窗口，复用 Markdown 代码块的 tree-sitter 高亮。
//! 基于 Velotype 修改：代码文件作为独立只读视图打开。

use std::path::PathBuf;

use gpui::*;

use crate::components::markdown::code_highlight::{CodeHighlightResult, code_highlight_color};
use crate::theme::{Theme, ThemeManager};
use crate::window_chrome::maksher_window_options;

pub(crate) struct CodeViewer {
    path: PathBuf,
    source: SharedString,
    highlight: Option<CodeHighlightResult>,
    system_appearance_subscription: Option<Subscription>,
}

impl CodeViewer {
    fn render_code(&self, theme: &Theme) -> AnyElement {
        let text = self.source.clone();
        let base_color = theme.colors.text_default;
        let code_font = if cfg!(target_os = "windows") {
            font("Consolas")
        } else {
            font("Menlo")
        };
        let base_run = TextRun {
            len: text.len(),
            font: code_font,
            color: base_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let spans = self
            .highlight
            .as_ref()
            .map(|result| result.spans.as_slice())
            .unwrap_or(&[]);
        let mut runs = Vec::with_capacity(spans.len() * 2 + 1);
        let mut cursor = 0;
        for span in spans {
            if span.range.start > cursor {
                runs.push(TextRun {
                    len: span.range.start - cursor,
                    ..base_run.clone()
                });
            }
            runs.push(TextRun {
                len: span.range.end - span.range.start,
                color: code_highlight_color(&theme.colors, span.class),
                ..base_run.clone()
            });
            cursor = span.range.end;
        }
        if cursor < text.len() {
            runs.push(TextRun {
                len: text.len() - cursor,
                ..base_run
            });
        }
        if text.is_empty() {
            runs.clear();
        }

        div()
            .id("code-viewer-scroll")
            .w_full()
            .h_full()
            .overflow_y_scroll()
            .overflow_x_scroll()
            .bg(theme.colors.code_bg)
            .px(px(24.0))
            .py(px(20.0))
            .text_size(px(theme.typography.text_size * 0.92))
            .child(StyledText::new(text).with_runs(runs))
            .into_any_element()
    }
}

impl Render for CodeViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.system_appearance_subscription.is_none() {
            self.system_appearance_subscription =
                Some(cx.observe_window_appearance(window, |_viewer, window, cx| {
                    let appearance = window.appearance();
                    cx.update_global::<ThemeManager, _>(|manager, _cx| {
                        manager.set_system_appearance(appearance)
                    });
                    cx.refresh_windows();
                }));
        }
        let theme = cx.global::<ThemeManager>().current_arc();
        let file_name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned());
        let path_text = self.path.to_string_lossy().into_owned();
        let c = &theme.colors;

        div()
            .id("maksher-code-viewer")
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .font(font(".SystemUIFont"))
            .bg(c.editor_background)
            .child(
                div()
                    .w_full()
                    .h(px(46.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(18.0))
                    .border_b(px(1.0))
                    .border_color(c.dialog_border)
                    .bg(c.dialog_surface)
                    .child(
                        div()
                            .text_size(px(theme.typography.text_size))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c.text_default)
                            .child(file_name),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .text_size(px(theme.typography.text_size * 0.78))
                            .text_color(c.dialog_muted)
                            .child(path_text),
                    )
                    .child(
                        div()
                            .px(px(9.0))
                            .py(px(4.0))
                            .rounded(px(6.0))
                            .bg(c.selection)
                            .text_size(px(theme.typography.text_size * 0.75))
                            .text_color(c.text_default)
                            .child("只读"),
                    ),
            )
            .child(self.render_code(&theme))
    }
}

pub(crate) fn open_code_viewer_window(
    cx: &mut App,
    path: PathBuf,
    source: String,
    highlight: Option<CodeHighlightResult>,
) -> anyhow::Result<()> {
    let title = path
        .file_name()
        .map(|name| format!("maksher - {}", name.to_string_lossy()))
        .unwrap_or_else(|| "maksher".to_string());
    let bounds = Bounds::centered(None, size(px(980.0), px(720.0)), cx);
    let _ = cx.open_window(
        maksher_window_options(title.into(), bounds),
        move |_, cx| {
            cx.new(|_| CodeViewer {
                path,
                source: source.into(),
                highlight,
                system_appearance_subscription: None,
            })
        },
    )?;
    Ok(())
}
