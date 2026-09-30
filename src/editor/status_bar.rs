//! Bottom status bar: cursor position, word count, and custom buttons.

use gpui::*;
use unicode_segmentation::UnicodeSegmentation;

const VIEW_SOURCE_ICON: &str = "icon/workspace/view-source.svg";

use super::Editor;
use crate::config::preferences::{StatusBarButton, StatusBarPreferences};
use crate::i18n::I18nStrings;
use crate::theme::Theme;

#[derive(Default)]
pub(super) struct StatusBarState {
    custom_button_hovered: Option<String>,
}

impl Editor {
    /// 状态栏整篇字数（P4a：按 document_revision 缓存，避免每帧全文扫描）。
    pub(super) fn cached_total_word_count(&self) -> usize {
        let revision = self.document_revision;
        if let Some((cached_revision, count)) = self.word_count_cache.get()
            && cached_revision == revision
        {
            return count;
        }
        self.word_count_scans.set(self.word_count_scans.get() + 1);
        let count = count_words(&self.last_stable_source_text);
        self.word_count_cache.set(Some((revision, count)));
        count
    }

    pub(super) fn render_status_bar(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let prefs = self.status_bar_preferences(cx);
        if !prefs.enabled {
            return None;
        }

        let c = &theme.colors;
        let d = &theme.dimensions;

        let mut right_items: Vec<AnyElement> = Vec::new();

        if prefs.show_cursor_position && self.view_mode == super::ViewMode::Source {
            right_items.push(render_cursor(
                self.compute_source_cursor_position(cx),
                theme,
            ));
        }

        if let Some(lines) = self.active_code_line_count(cx) {
            right_items.push(
                div()
                    .text_size(px(d.status_bar_text_size))
                    .text_color(c.status_bar_text_dim)
                    .child(format!("{lines} 行"))
                    .into_any_element(),
            );
        } else if prefs.show_word_count {
            let total_count = self.cached_total_word_count();
            // 只算选中文本的词数：这里每帧都会跑，不能走 O(整篇) 的 markdown 序列化。
            let selection_count = self.selected_visible_text(cx).as_deref().map(count_words);
            right_items.push(render_word_count(
                selection_count,
                total_count,
                theme,
                strings,
            ));
        }

        // 右下角的模式切换按钮（用户需求：阅读时长信息换成源码切换）。
        right_items.push(self.render_view_mode_toggle(theme, cx));

        if self.long_source_block_hint(cx) {
            right_items.push(
                div()
                    .id("status-long-block-hint")
                    .text_size(px(d.status_bar_text_size))
                    .text_color(c.status_bar_text_dim)
                    .child(strings.status_bar_long_block_source.clone())
                    .into_any_element(),
            );
        }

        for button in &prefs.custom_buttons {
            right_items.push(render_custom_button(
                &mut self.status_bar,
                button,
                theme,
                cx,
            ));
        }

        // 面包屑：工作区根 → 当前文档相对路径（roadmap E8）。点击在树中定位。
        let breadcrumb = self.render_breadcrumb(theme, cx);

        let bar = div()
            .id("status-bar")
            .h(px(d.status_bar_height))
            .w_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_end()
            .px(px(d.status_bar_padding_x))
            .bg(c.status_bar_background)
            .border_t(px(1.0))
            .border_color(c.dialog_border)
            .child(breadcrumb)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(d.status_bar_item_gap))
                    .children(right_items),
            )
            .into_any_element();

        Some(bar)
    }

    /// 文档是否含超长单块（roadmap B12）；按文档修订缓存，避免逐帧扫描。
    fn long_source_block_hint(&mut self, _cx: &mut Context<Self>) -> bool {
        let revision = self.document_revision;
        match self.long_source_block_hint {
            Some((cached, value)) if cached == revision => value,
            _ => {
                let value = document_has_long_source_block(&self.last_stable_source_text);
                self.long_source_block_hint = Some((revision, value));
                value
            }
        }
    }

    /// 工作区根/子路径/文件名 面包屑；无工作区根时隐藏。
    fn render_breadcrumb(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let Some(document_path) = self.file_path.as_ref() else {
            return div().into_any_element();
        };
        let Some(root) = self.workspace_root_path() else {
            return div().into_any_element();
        };
        let Ok(relative) = document_path.strip_prefix(root) else {
            return div().into_any_element();
        };
        let root_name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        let relative_text = relative.to_string_lossy().into_owned();
        let editor = cx.entity().downgrade();
        let reveal_path = document_path.clone();

        div()
            .id("status-bar-breadcrumb")
            .mr(px(d.status_bar_item_gap))
            .flex()
            .items_center()
            .gap(px(4.0))
            .text_size(px(d.status_bar_text_size))
            .text_color(c.status_bar_text_dim)
            .cursor_pointer()
            .hover(|this| this.text_color(c.status_bar_text))
            .child(root_name)
            .child("›")
            .child(relative_text)
            .on_click(move |_event, _window, cx| {
                let _ = editor.update(cx, |editor, cx| {
                    editor.reveal_path_in_tree(&reveal_path);
                    cx.notify();
                });
            })
            .into_any_element()
    }

    fn status_bar_preferences(&self, cx: &App) -> StatusBarPreferences {
        crate::config::preferences::EditorSettings::status_bar_preferences(cx)
    }

    /// Returns (line, col), both 1-based, from the source-mode selection snapshot.
    fn compute_source_cursor_position(&self, cx: &App) -> (usize, usize) {
        let snapshot = self.capture_source_selection_snapshot(cx);
        let cursor_offset = snapshot.range.end;
        let text = self.document.raw_source_text(cx);
        let clamped = cursor_offset.min(text.len());

        let line = text[..clamped].matches('\n').count() + 1;
        let last_newline = text[..clamped].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = text[last_newline..clamped].graphemes(true).count() + 1;
        (line, col)
    }
}

/// 是否存在超过长块阈值的源码行（roadmap B12）。
fn document_has_long_source_block(source: &str) -> bool {
    source
        .split('\n')
        .any(|line| line.len() > crate::components::LONG_BLOCK_SOURCE_LIMIT)
}

fn render_cursor((line, col): (usize, usize), theme: &Theme) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;

    let label = format!("{} : {}", &line.to_string(), &col.to_string());

    div()
        .text_size(px(d.status_bar_text_size))
        .text_color(c.status_bar_text)
        .child(label)
        .into_any_element()
}

fn render_word_count(
    selection_count: Option<usize>,
    total_count: usize,
    theme: &Theme,
    strings: &I18nStrings,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;

    let label = if let Some(sel) = selection_count {
        format!(
            "{} / {} {}",
            sel, total_count, strings.status_bar_word_count_suffix
        )
    } else {
        format!("{} {}", total_count, strings.status_bar_word_count_suffix)
    };

    div()
        .text_size(px(d.status_bar_text_size))
        .text_color(c.status_bar_text_dim)
        .child(label)
        .into_any_element()
}

/// 右下角的视图模式切换按钮：`</>` 图标，点击在渲染态/源码态之间切换（用户需求）。
impl Editor {
    fn render_view_mode_toggle(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let c = &theme.colors;
        let editor = cx.entity().downgrade();
        // 图标颜色必须设在 svg 自身上：gpui 的 svg 元素只读自己的 style.text.color，
        // 父容器的 text_color 不继承，漏设就一个像素都不画
        // （docs/architecture/overview.md §GPUI 限制）。悬停提亮走 group_hover。
        div()
            .id("status-bar-view-mode-toggle")
            .debug_selector(|| "status-bar-view-mode-toggle".to_string())
            .group("status-bar-view-mode-toggle")
            .cursor_pointer()
            .child(
                svg()
                    .path(VIEW_SOURCE_ICON)
                    .size(px(14.0))
                    .flex_shrink_0()
                    .text_color(c.status_bar_text_dim)
                    .group_hover("status-bar-view-mode-toggle", |this| {
                        this.text_color(c.status_bar_text)
                    }),
            )
            .on_click(move |_event, _window, cx| {
                let _ = editor.update(cx, |editor, cx| editor.toggle_view_mode_from_ui(cx));
            })
            .into_any_element()
    }
}

fn render_custom_button(
    state: &mut StatusBarState,
    button: &StatusBarButton,
    theme: &Theme,
    cx: &mut Context<Editor>,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;

    let id = button.id.clone();
    let hovered = state.custom_button_hovered.as_deref() == Some(&button.id);

    div()
        .id(ElementId::Name(
            format!("status-bar-custom-button-{}", button.id).into(),
        ))
        .h(px(d.status_bar_height - 4.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .rounded(px(4.0))
        .bg(if hovered {
            c.status_bar_button_hover
        } else {
            hsla(0., 0., 0., 0.)
        })
        .cursor_pointer()
        .text_size(px(d.status_bar_text_size))
        .text_color(c.status_bar_text)
        .child(button.label.clone())
        .on_hover(cx.listener(
            move |editor: &mut Editor,
                  hovered: &bool,
                  _window: &mut Window,
                  cx: &mut Context<Editor>| {
                if *hovered {
                    editor.status_bar.custom_button_hovered = Some(id.clone());
                } else if editor.status_bar.custom_button_hovered.as_deref() == Some(&id) {
                    editor.status_bar.custom_button_hovered = None;
                }
                cx.notify();
            },
        ))
        .into_any_element()
}

/// Count words in mixed CJK / Latin text.
///
/// Every CJK character counts as one word. Latin words are split on whitespace.
pub fn count_words(text: &str) -> usize {
    let mut count = 0;
    let mut in_latin_word = false;

    for ch in text.chars() {
        if is_cjk_char(ch) {
            if in_latin_word {
                count += 1;
                in_latin_word = false;
            }
            count += 1;
        } else if ch.is_whitespace() {
            if in_latin_word {
                count += 1;
                in_latin_word = false;
            }
        } else {
            in_latin_word = true;
        }
    }
    if in_latin_word {
        count += 1;
    }
    count
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        // CJK Unified Ideographs
        0x4E00..=0x9FFF
        // CJK Unified Ideographs Extension A
        | 0x3400..=0x4DBF
        // CJK Unified Ideographs Extension B
        | 0x20000..=0x2A6DF
        // CJK Compatibility Ideographs
        | 0xF900..=0xFAFF
        // CJK Radicals Supplement / Kangxi Radicals
        | 0x2E80..=0x2EFF
        | 0x2F00..=0x2FDF
        // Hiragana / Katakana (Japanese)
        | 0x3040..=0x309F
        | 0x30A0..=0x30FF
        // Hangul Syllables (Korean)
        | 0xAC00..=0xD7AF
    )
}

#[cfg(test)]
mod tests {
    use super::{count_words, document_has_long_source_block};

    #[test]
    fn long_source_block_hint_tracks_single_long_line() {
        let short = "alpha\nbeta";
        let long_line = "x".repeat(crate::components::LONG_BLOCK_SOURCE_LIMIT);
        let just_over = format!("x{}", "y".repeat(crate::components::LONG_BLOCK_SOURCE_LIMIT));
        let multi_line_long = format!("a\n{just_over}");

        assert!(!document_has_long_source_block(short));
        assert!(!document_has_long_source_block(&long_line));
        assert!(document_has_long_source_block(&just_over));
        assert!(document_has_long_source_block(&multi_line_long));
    }

    #[test]
    fn empty_text_has_zero_words() {
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn english_words_are_counted() {
        assert_eq!(count_words("hello world"), 2);
        assert_eq!(count_words("one two three four"), 4);
    }

    #[test]
    fn cjk_characters_are_counted_individually() {
        assert_eq!(count_words("你好世界"), 4);
        assert_eq!(count_words("中文"), 2);
    }

    #[test]
    fn mixed_cjk_and_english() {
        assert_eq!(count_words("hello 世界"), 3);
        assert_eq!(count_words("你好 world foo"), 4);
    }

    #[test]
    fn whitespace_handling() {
        assert_eq!(count_words("  hello   world  "), 2);
        assert_eq!(count_words("   "), 0);
    }
}
