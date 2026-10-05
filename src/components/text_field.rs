//! 通用单行文本输入组件。
//!
//! 设置页(AI 服务地址/密钥/模型)与编辑器内 AI 面板(自定义指令)共用。
//! 输入走 `EntityInputHandler`(与工作区搜索框、命令面板同一条 IME 路由,
//! 中文与非 ASCII 文本能输入),按键处理在本组件内完成:方向键/行首行尾/
//! 退格/删除/全选/剪贴板,回车交给宿主的 `on_enter` 回调。
//!
//! 与搜索框保持同一视觉语言:聚焦描边、占位符灰显、不画自定义光标
//! (单行小输入,选区高亮与光标绘制的收益抵不过复杂度)。

use std::ops::Range;

use gpui::*;
use unicode_segmentation::UnicodeSegmentation;

/// 回车回调签名。
type OnEnterCallback = Box<dyn Fn(&mut TextField, &mut Window, &mut Context<TextField>) + 'static>;

/// 单行文本输入。
pub(crate) struct TextField {
    value: String,
    /// UTF-8 字节偏移;总是落在字符边界上。
    selected_range: Range<usize>,
    /// IME 组词中的候选区间。
    marked_range: Option<Range<usize>>,
    focus: FocusHandle,
    placeholder: SharedString,
    /// 回车回调(AI 面板用它触发自定义指令;设置页不用)。
    on_enter: Option<OnEnterCallback>,
}

impl TextField {
    pub(crate) fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            value: String::new(),
            selected_range: 0..0,
            marked_range: None,
            focus: cx.focus_handle(),
            placeholder: placeholder.into(),
            on_enter: None,
        }
    }

    /// 注册回车回调。
    pub(crate) fn on_enter(
        mut self,
        callback: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Self {
        self.on_enter = Some(Box::new(callback));
        self
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    pub(crate) fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub(crate) fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    /// 让输入框拿到焦点(光标移到末尾)。
    pub(crate) fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.value.len(), false);
        window.focus(&self.focus);
        cx.notify();
    }

    /// 整体替换内容,光标移到末尾(打开面板回填默认值用)。
    pub(crate) fn set_value(&mut self, value: &str, cx: &mut Context<Self>) {
        self.value = value.to_string();
        self.selected_range = self.value.len()..self.value.len();
        self.marked_range = None;
        cx.notify();
    }

    fn normalized_selection(&self) -> Range<usize> {
        self.selected_range.start.min(self.selected_range.end)
            ..self.selected_range.start.max(self.selected_range.end)
    }

    fn selected_text(&self) -> &str {
        &self.value[self.normalized_selection()]
    }

    /// 用 `text` 替换指定区间(区间会被夹回字符边界),光标停在插入文本之后。
    fn replace_range(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        let range = clamp_to_char_boundaries(&self.value, range);
        self.value.replace_range(range.clone(), text);
        let caret = range.start + text.len();
        self.selected_range = caret..caret;
        cx.notify();
    }

    fn move_to(&mut self, offset: usize, extend: bool) {
        let offset = offset.min(self.value.len());
        if extend {
            self.selected_range.end = offset;
        } else {
            self.selected_range = offset..offset;
        }
    }

    fn move_caret_left(&mut self, extend: bool) {
        let (start, end) = (self.selected_range.start, self.selected_range.end);
        if extend {
            // Shift 扩选:锚(start)不动,头(end)向左退一个字素。
            let target = self.value[..end]
                .grapheme_indices(true)
                .next_back()
                .map(|(index, _)| index)
                .unwrap_or(end);
            self.selected_range.end = target;
            return;
        }
        if start != end {
            self.move_to(start, false);
            return;
        }
        let target = self.value[..start]
            .grapheme_indices(true)
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(start);
        self.move_to(target, false);
    }

    fn move_caret_right(&mut self, extend: bool) {
        let (start, end) = (self.selected_range.start, self.selected_range.end);
        if extend {
            let from = end;
            let target = self.value[from..]
                .grapheme_indices(true)
                .nth(1)
                .map(|(index, _)| from + index)
                .unwrap_or(self.value.len());
            self.selected_range.end = target;
            return;
        }
        if start != end {
            self.move_to(end, false);
            return;
        }
        let from = start;
        let target = self.value[from..]
            .grapheme_indices(true)
            .nth(1)
            .map(|(index, _)| from + index)
            .unwrap_or(self.value.len());
        self.move_to(target, false);
    }

    fn select_all(&mut self) {
        self.selected_range = 0..self.value.len();
    }

    fn delete_backwards(&mut self, cx: &mut Context<Self>) {
        let selection = self.normalized_selection();
        let range = if selection.is_empty() {
            let start = self.value[..selection.start]
                .grapheme_indices(true)
                .next_back()
                .map(|(index, _)| index)
                .unwrap_or(selection.start);
            start..selection.start
        } else {
            selection
        };
        if !range.is_empty() {
            self.replace_range(range, "", cx);
        }
    }

    fn delete_forwards(&mut self, cx: &mut Context<Self>) {
        let selection = self.normalized_selection();
        let range = if selection.is_empty() {
            let end = self.value[selection.end..]
                .grapheme_indices(true)
                .nth(1)
                .map(|(index, _)| selection.end + index)
                .unwrap_or(self.value.len());
            selection.end..end
        } else {
            selection
        };
        if !range.is_empty() {
            self.replace_range(range, "", cx);
        }
    }

    fn copy_selection(&self, cx: &mut Context<Self>) {
        let text = self.selected_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        }
    }

    fn cut_selection(&mut self, cx: &mut Context<Self>) {
        let selection = self.normalized_selection();
        if selection.is_empty() {
            return;
        }
        let text = self.selected_text().to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.replace_range(selection, "", cx);
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            // 单行输入:粘贴内容里的换行折成空格,不悄悄截断。
            let text = text.replace(['\r', '\n'], " ");
            let selection = self.normalized_selection();
            self.replace_range(selection, &text, cx);
        }
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.to_ascii_lowercase();
        let secondary = event.keystroke.modifiers.secondary();
        let shift = event.keystroke.modifiers.shift;
        match key.as_str() {
            // 交给宿主层:面板用它关闭自己,设置页用全局 Escape 语义。
            "escape" => return,
            "left" => {
                self.move_caret_left(shift);
                window.prevent_default();
                cx.stop_propagation();
            }
            "right" => {
                self.move_caret_right(shift);
                window.prevent_default();
                cx.stop_propagation();
            }
            "home" => {
                self.move_to(0, shift);
                window.prevent_default();
                cx.stop_propagation();
            }
            "end" => {
                self.move_to(self.value.len(), shift);
                window.prevent_default();
                cx.stop_propagation();
            }
            "backspace" => {
                self.delete_backwards(cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "delete" => {
                self.delete_forwards(cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "a" if secondary => {
                self.select_all();
                window.prevent_default();
                cx.stop_propagation();
            }
            "c" if secondary => {
                self.copy_selection(cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "x" if secondary => {
                self.cut_selection(cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "v" if secondary => {
                self.paste(cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "enter" => {
                let callback = self.on_enter.take();
                if let Some(callback) = callback {
                    callback(self, window, cx);
                    self.on_enter = Some(callback);
                }
                window.prevent_default();
                cx.stop_propagation();
            }
            // 其余按键(含普通字符、IME 组词键)交给平台输入路由,
            // 最终从 EntityInputHandler 进来。
            _ => return,
        }
        cx.notify();
    }
}

/// UTF-16 偏移 → UTF-8 字节偏移(IME 汇报的选区都是 UTF-16)。
fn utf16_to_utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut utf8_offset = 0usize;
    let mut seen_utf16 = 0usize;
    for (index, ch) in text.char_indices() {
        if seen_utf16 >= utf16_offset {
            return index;
        }
        seen_utf16 += ch.len_utf16();
        utf8_offset = index + ch.len_utf8();
    }
    if seen_utf16 < utf16_offset {
        return text.len();
    }
    utf8_offset
}

/// UTF-8 字节偏移 → UTF-16 偏移。
fn utf8_to_utf16_offset(text: &str, utf8_offset: usize) -> usize {
    text[..utf8_offset.min(text.len())].chars().map(|ch| ch.len_utf16()).sum()
}

/// 把区间夹回字符边界(IME/换算给出的偏移可能落在多字节字符中间)。
fn clamp_to_char_boundaries(text: &str, range: Range<usize>) -> Range<usize> {
    fn clamp(text: &str, mut offset: usize) -> usize {
        offset = offset.min(text.len());
        while offset > 0 && !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }
    clamp(text, range.start)..clamp(text, range.end)
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let start = utf16_to_utf8_offset(&self.value, range.start);
        let end = utf16_to_utf8_offset(&self.value, range.end).max(start);
        *actual_range = Some(
            utf8_to_utf16_offset(&self.value, start)..utf8_to_utf16_offset(&self.value, end),
        );
        Some(self.value[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let selection = self.normalized_selection();
        Some(UTF16Selection {
            range: utf8_to_utf16_offset(&self.value, selection.start)
                ..utf8_to_utf16_offset(&self.value, selection.end),
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let marked = self.marked_range.clone()?;
        Some(
            utf8_to_utf16_offset(&self.value, marked.start)
                ..utf8_to_utf16_offset(&self.value, marked.end),
        )
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|range| {
                utf16_to_utf8_offset(&self.value, range.start)
                    ..utf16_to_utf8_offset(&self.value, range.end)
            })
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.normalized_selection());
        self.marked_range = None;
        self.replace_range(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|range| {
                utf16_to_utf8_offset(&self.value, range.start)
                    ..utf16_to_utf8_offset(&self.value, range.end)
            })
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.normalized_selection());
        self.replace_range(range.clone(), new_text, cx);
        let marked_start = range.start;
        let marked_end = range.start + new_text.len();
        self.marked_range = Some(marked_start..marked_end);
        if let Some(selected) = new_selected_range {
            self.selected_range = utf16_to_utf8_offset(&self.value, selected.start)
                ..utf16_to_utf8_offset(&self.value, selected.end);
        }
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(utf8_to_utf16_offset(&self.value, self.selected_range.end))
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<crate::theme::ThemeManager>().current().clone();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let focused = self.focus.is_focused(window);
        let empty = self.value.is_empty();
        let focus = self.focus.clone();
        let entity = cx.entity();

        div()
            .id("text-field")
            .track_focus(&self.focus)
            .w_full()
            .h(px(32.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .rounded(px(d.menu_item_radius))
            .border_1()
            .border_color(if focused {
                c.dialog_primary_button_bg
            } else {
                c.dialog_border
            })
            .bg(c.editor_background)
            .text_size(px(theme.typography.dialog_body_size))
            .text_color(if empty {
                c.dialog_muted
            } else {
                c.text_default
            })
            .child(if empty {
                self.placeholder.clone()
            } else {
                self.value.clone().into()
            })
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                    },
                )
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .left_0(),
            )
            .on_click(cx.listener(|this, _event, window, cx| this.focus(window, cx)))
            .on_key_down(cx.listener(Self::handle_key_down))
    }
}

#[cfg(test)]
mod tests;
