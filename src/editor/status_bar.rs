//! Bottom status bar: cursor position, word count, and custom buttons.

use gpui::*;
use gpui_kit::component::{
    Icon, Selectable as _, Sizable as _,
    breadcrumb::{Breadcrumb, BreadcrumbItem},
    button::{Button, ButtonVariants},
    status_bar::StatusBar,
};
use unicode_segmentation::UnicodeSegmentation;

const VIEW_SOURCE_ICON: &str = "icon/workspace/view-source.svg";

use super::Editor;
use crate::config::preferences::{StatusBarButton, StatusBarPreferences};
use crate::i18n::I18nStrings;
use crate::theme::Theme;

impl Editor {
    /// vim 的模式名保持英文词汇；纯文本显示，不随界面语言翻译。
    fn vim_mode_label(&self) -> &'static str {
        use super::vim::{VimMode, VisualKind};
        match self.vim.mode {
            VimMode::Insert => "Insert",
            VimMode::Replace => "Replace",
            VimMode::Visual(VisualKind::Char) => "Visual",
            VimMode::Visual(VisualKind::Line) => "Visual Line",
            VimMode::Visual(VisualKind::Block) => "Visual Block",
            VimMode::Normal => "Normal",
        }
    }

    /// 状态栏整篇统计的静默窗口：打字期间不重扫。
    const STATUS_SCAN_SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

    /// 整篇统计（字数、超长块提示）现在能不能重算。
    ///
    /// 打字期间每个按键都会递增 `document_revision`，旧实现据此重算两遍整篇
    /// 统计（1 MiB 文档里整篇分词 29ms、扫行十几 ms）。这里改成：修订变了先
    /// 沿用上次结果，等文档安静 250ms 再补算一次——用户看到的数字晚一拍，
    /// 但不再为每个字符付整篇扫描。
    fn status_scan_ready(&mut self, revision: u64, cx: &mut Context<Self>) -> bool {
        if self.status_scan_settled_revision == Some(revision) {
            return true;
        }
        self.schedule_status_scan(cx);
        false
    }

    fn schedule_status_scan(&mut self, cx: &mut Context<Self>) {
        if self.status_scan_task.is_some() {
            return;
        }
        let weak_editor = cx.entity().downgrade();
        self.status_scan_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    let Ok(revision) = weak_editor.update(cx, |this, _cx| this.document_revision)
                    else {
                        return;
                    };
                    cx.background_executor()
                        .timer(Self::STATUS_SCAN_SETTLE)
                        .await;
                    let keep_waiting = weak_editor
                        .update(cx, |this, cx| {
                            if this.document_revision != revision {
                                // 还 在打字：再等一个静默窗口。
                                return true;
                            }
                            this.status_scan_task = None;
                            this.status_scan_settled_revision = Some(revision);
                            cx.notify();
                            false
                        })
                        .unwrap_or(false);
                    if !keep_waiting {
                        return;
                    }
                }
            },
        ));
    }

    /// 状态栏整篇字数（P4a 缓存 + P2 静默窗口）：渲染每帧读，但只在文档
    /// 安静下来后重扫一次。
    pub(super) fn cached_total_word_count(&mut self, cx: &mut Context<Self>) -> usize {
        let revision = self.document_revision;
        if let Some((cached_revision, count)) = self.word_count_cache.get() {
            if cached_revision == revision {
                return count;
            }
            if !self.status_scan_ready(revision, cx) {
                return count;
            }
        }
        self.word_count_scans.set(self.word_count_scans.get() + 1);
        let count = count_words(&self.buffer.text());
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

        // vim 模式指示：开着一律显示当前模式（vim 使用者靠它确认落点）。
        if self.vim_enabled {
            right_items.push(
                div()
                    .id("status-bar-vim-mode")
                    .debug_selector(|| "status-bar-vim-mode".into())
                    .text_size(px(d.status_bar_text_size))
                    .text_color(c.status_bar_text_dim)
                    .child(self.vim_mode_label())
                    .into_any_element(),
            );
        }

        if prefs.show_cursor_position && self.view_mode == super::ViewMode::Source {
            right_items.push(render_cursor(
                self.compute_source_cursor_position(cx),
                theme,
            ));
        }

        if let Some(preview) = &self.image_preview {
            if let Some(image) = &preview.image {
                let size = image.size(0);
                right_items.push(
                    div()
                        .text_size(px(d.status_bar_text_size))
                        .text_color(c.status_bar_text_dim)
                        .child(format!("{} × {}", size.width.0, size.height.0))
                        .into_any_element(),
                );
            }
        } else if let Some(lines) = self.active_code_line_count(cx) {
            right_items.push(
                div()
                    .text_size(px(d.status_bar_text_size))
                    .text_color(c.status_bar_text_dim)
                    .child(format!("{lines} 行"))
                    .into_any_element(),
            );
        } else if prefs.show_word_count {
            let total_count = self.cached_total_word_count(cx);
            // 只算选中文本的词数：这里每帧都会跑，不能走 O(整篇) 的 markdown 序列化。
            let selection_count = self.selected_visible_text(cx).as_deref().map(count_words);
            right_items.push(render_word_count(
                selection_count,
                total_count,
                theme,
                strings,
            ));
        }

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
            right_items.push(render_custom_button(button, cx));
        }

        let mut mode_controls = div()
            .id("status-bar-mode-controls")
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap_0p5()
            .child(self.render_reading_mode_toggle(theme, strings, cx));
        if !(self.code_document
            || self.source_mode_fallback_required
            || self.image_preview.is_some())
        {
            mode_controls = mode_controls.child(self.render_view_mode_toggle(theme, strings, cx));
        }
        right_items.push(mode_controls.into_any_element());

        // 面包屑：工作区根 → 当前文档相对路径（roadmap E8）。点击在树中定位。
        let breadcrumb = self.render_breadcrumb(theme, cx);

        let bar = div()
            .id("status-bar")
            .w_full()
            .flex_shrink_0()
            .child(
                StatusBar::new()
                    .h(px(d.status_bar_height))
                    .py_0()
                    .px(rems(d.status_bar_padding_x / 16.0))
                    .right(
                        div()
                            .flex()
                            .items_center()
                            .min_w_0()
                            .gap(rems(d.status_bar_item_gap / 16.0))
                            .child(breadcrumb)
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .gap(rems(d.status_bar_item_gap / 16.0))
                                    .children(right_items),
                            ),
                    ),
            )
            .into_any_element();

        Some(bar)
    }

    /// 文档是否含超长单块（roadmap B12）；同字数一样走静默窗口，
    /// 不再每个按键都扫一遍整篇行。
    fn long_source_block_hint(&mut self, cx: &mut Context<Self>) -> bool {
        let revision = self.document_revision;
        let cached = self.long_source_block_hint;
        match cached {
            Some((cached, value)) if cached == revision => value,
            Some((_, value)) if !self.status_scan_ready(revision, cx) => value,
            _ => {
                let value = document_has_long_source_block(&self.buffer.text());
                self.long_source_block_hint = Some((revision, value));
                value
            }
        }
    }

    /// 工作区根/子路径/文件名 面包屑；无工作区根时隐藏。
    fn render_breadcrumb(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let d = &theme.dimensions;
        let Some(document_path) = self.file_path.as_ref() else {
            return div().into_any_element();
        };
        let Some(root) = self.workspace_root_path() else {
            return div().into_any_element();
        };
        // 两边的写法可能不同：Windows 上 `canonicalize` 会带 `\\?\` 前缀，而路径从
        // 会话/编辑器那边过来时可能已经剥过。按原始字符串比就会整条面包屑消失，
        // 所以两边都归一（`strip_verbatim_prefix` 是剥前缀的唯一判据）。
        let normalized_document = crate::file_url::strip_verbatim_prefix(document_path);
        let normalized_root = crate::file_url::strip_verbatim_prefix(root);
        let Ok(relative) = normalized_document.strip_prefix(&normalized_root) else {
            return div().into_any_element();
        };
        let root_name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        let relative_text = relative.to_string_lossy().into_owned();
        let reveal_path = document_path.clone();

        Button::new("status-bar-breadcrumb")
            .ghost()
            .small()
            .min_w_0()
            .flex_shrink_1()
            .max_w(rems(30.0))
            .accessibility_label(format!("{root_name} › {relative_text}"))
            .tooltip(document_path.to_string_lossy().into_owned())
            .child(
                Breadcrumb::new()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(px(d.status_bar_text_size))
                    .child(
                        BreadcrumbItem::new(root_name)
                            .flex_shrink_0()
                            .max_w(rems(10.0))
                            .truncate(),
                    )
                    .child(BreadcrumbItem::new(relative_text).min_w_0().truncate()),
            )
            .on_click(cx.listener(move |editor, _, _, cx| {
                editor.reveal_path_in_tree(&reveal_path);
                cx.notify();
            }))
            .into_any_element()
    }

    fn status_bar_preferences(&self, cx: &App) -> StatusBarPreferences {
        crate::config::preferences::EditorSettings::status_bar_preferences(cx)
    }

    /// Returns (line, col), both 1-based, from the source-mode selection snapshot.
    ///
    /// 行列号在缓冲区自己的坐标里算：状态栏每帧都要读它，拿整篇序列化出来的文本去数
    /// 换行就是每帧一次 O(文档) 的搬运，文档多大都跟着它贵。
    pub(super) fn compute_source_cursor_position(&self, cx: &App) -> (usize, usize) {
        let snapshot = self.capture_source_selection_snapshot(cx);
        let offset = snapshot.range.end.min(self.buffer.byte_len());
        let line = self.buffer.line_of(offset);
        let line_start = self.buffer.line_range(line).start;
        let column = self
            .buffer
            .slice(line_start..offset)
            .graphemes(true)
            .count();
        (line + 1, column + 1)
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
    fn render_reading_mode_toggle(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.is_read_only();
        let label = if active {
            &strings.reading_mode_enabled
        } else {
            &strings.command_toggle_reading_mode
        }
        .clone();
        Button::new("status-bar-reading-mode")
            .debug_selector(|| "status-bar-reading-mode".into())
            .ghost()
            .small()
            .selected(active)
            .accessibility_label(label.clone())
            .tooltip(label)
            .icon(
                Icon::empty()
                    .path("icon/workspace/reading-mode.svg")
                    .text_color(if active {
                        theme.colors.text_link
                    } else {
                        theme.colors.status_bar_text_dim
                    }),
            )
            .on_click(cx.listener(|editor, _, _, cx| editor.toggle_reading_mode(cx)))
            .into_any_element()
    }

    fn render_view_mode_toggle(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let source = self.view_mode == super::ViewMode::Source;
        let label = if source {
            &strings.view_mode_switch_to_rendered
        } else {
            &strings.view_mode_switch_to_source
        };
        Button::new("status-bar-view-mode-toggle")
            .debug_selector(|| "status-bar-view-mode-toggle".into())
            .ghost()
            .small()
            .selected(source)
            .accessibility_label(label.clone())
            .tooltip(label.clone())
            .icon(
                Icon::empty()
                    .path(VIEW_SOURCE_ICON)
                    .text_color(theme.colors.status_bar_text_dim),
            )
            .on_click(cx.listener(|editor, _, _, cx| editor.toggle_view_mode_from_ui(cx)))
            .into_any_element()
    }
}

fn render_custom_button(button: &StatusBarButton, cx: &mut Context<Editor>) -> AnyElement {
    let action_id = button.action_id.clone();
    Button::new(ElementId::Name(
        format!("status-bar-custom-button-{}", button.id).into(),
    ))
    .ghost()
    .small()
    .label(button.label.clone())
    .on_click(cx.listener(move |editor, _, window, cx| {
        editor.dispatch_status_bar_command(&action_id, window, cx);
    }))
    .into_any_element()
}

impl Editor {
    /// 状态栏自定义按钮派发动作的唯一入口：按 `action_id` 在命令注册表里找那条动作并派发，
    /// 与键位、正文右键菜单、选中工具栏、命令面板用同一份命令表（不变量 11）。查不到返回 `false`。
    pub(crate) fn dispatch_status_bar_command(
        &mut self,
        action_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(spec) = crate::commands::commands()
            .iter()
            .find(|spec| spec.id == action_id)
        else {
            eprintln!("状态栏按钮的 action_id {action_id:?} 不在命令注册表里，这一下不派发");
            return false;
        };
        window.dispatch_action(spec.boxed_action(), cx);
        true
    }
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

    /// 渲染侧必须让按钮调用派发函数。`.cursor_pointer()` 明示可点而函数体里只有
    /// `.on_hover`，配了 `action_id` 的按钮就是个静态装饰（评审必修 A2）。
    #[test]
    fn custom_button_render_body_dispatches_on_click() {
        let body = include_str!("status_bar.rs")
            .split("fn render_custom_button")
            .nth(1)
            .expect("渲染函数应在本文件里")
            .split("\n}\n")
            .next()
            .unwrap_or_default();
        assert!(body.contains(".on_click"), "按钮没有点击处理器：{body}");
        assert!(
            body.contains("dispatch_status_bar_command"),
            "点击没有调用派发函数：{body}"
        );
    }

    #[test]
    fn long_source_block_hint_tracks_single_long_line() {
        let short = "alpha\nbeta";
        let long_line = "x".repeat(crate::components::LONG_BLOCK_SOURCE_LIMIT);
        let just_over = format!(
            "x{}",
            "y".repeat(crate::components::LONG_BLOCK_SOURCE_LIMIT)
        );
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
