//! 通用文本输入组件，默认单行，可选择软换行的多行模式。
//!
//! 鼠标定位、光标与 IME 候选框共用实际文字布局，避免多字节文本下位置漂移。

use std::ops::Range;

use gpui::*;
use unicode_segmentation::UnicodeSegmentation;

mod element;
mod multiline;


struct TextEdit {
    start: usize,
    removed: String,
    inserted: String,
    selection: Range<usize>,
}

/// 单行与多行文本输入。
pub(crate) struct TextField {
    value: String,
    /// UTF-8 字节偏移;总是落在字符边界上。
    selected_range: Range<usize>,
    /// IME 组词中的候选区间。
    marked_range: Option<Range<usize>>,
    focus: FocusHandle,
    placeholder: SharedString,
    last_line: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_x: f32,
    multiline: Option<multiline::State>,
    undo: Vec<TextEdit>,
    redo: Vec<TextEdit>,
    composition_active: bool,
    dragging: bool,
    #[cfg(test)]
    painted_caret: bool,
    #[cfg(test)]
    painted_selection: bool,
    #[cfg(test)]
    painted_marked: bool,
}

impl TextField {
    pub(crate) fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    fn display_offset(&self, source: usize) -> usize {
        self.value.floor_char_boundary(source.min(self.value.len()))
    }

    fn index_for_point(&self, position: Point<Pixels>) -> usize {
        if let Some(state) = &self.multiline
            && let Some((bounds, layout)) = self.last_bounds.zip(state.layout.as_ref())
        {
            let offset = layout.index(position - bounds.origin + point(px(0.0), state.scroll_y));
            return self.value.floor_char_boundary(offset.min(self.value.len()));
        }
        let Some((bounds, line)) = self.last_bounds.zip(self.last_line.as_ref()) else {
            return self.value.len();
        };
        let display = line.closest_index_for_x(position.x - bounds.left() + px(self.scroll_x));
        self.value.floor_char_boundary(display.min(self.value.len()))
    }
    pub(crate) fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            value: String::new(),
            selected_range: 0..0,
            marked_range: None,
            focus: cx.focus_handle(),
            placeholder: placeholder.into(),
            last_line: None,
            last_bounds: None,
            scroll_x: 0.0,
            multiline: None,
            undo: Vec::new(),
            redo: Vec::new(),
            composition_active: false,
            dragging: false,
            #[cfg(test)]
            painted_caret: false,
            #[cfg(test)]
            painted_selection: false,
            #[cfg(test)]
            painted_marked: false,
        }
    }

    pub(crate) fn multiline(mut self, minimum_rows: usize, maximum_rows: usize) -> Self {
        self.multiline = Some(multiline::State {
            minimum_rows: minimum_rows.max(1),
            maximum_rows: maximum_rows.max(minimum_rows).max(1),
            layout: None,
            scroll_y: px(0.0),
            reveal_caret: true,
        });
        self
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    /// 让输入框拿到焦点(光标移到末尾)。
    #[cfg(test)]
    pub(crate) fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.value.len(), false);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// 整体替换内容,光标移到末尾(打开面板回填默认值用)。
    pub(crate) fn set_value(&mut self, value: &str, cx: &mut Context<Self>) {
        self.value = value.to_string();
        self.selected_range = self.value.len()..self.value.len();
        self.marked_range = None;
        self.undo.clear();
        self.redo.clear();
        self.composition_active = false;
        self.reveal_caret();
        cx.notify();
    }

    fn normalized_selection(&self) -> Range<usize> {
        self.selected_range.start.min(self.selected_range.end)
            ..self.selected_range.start.max(self.selected_range.end)
    }

    fn selected_text(&self) -> &str {
        self.value
            .get(self.normalized_selection())
            .unwrap_or_default()
    }

    /// 用 `text` 替换指定区间(区间会被夹回字符边界),光标停在插入文本之后。
    fn replace_range(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        let range = clamp_to_char_boundaries(&self.value, range);
        // 组词期间的编辑(整段替换,或 marked 区间内的子区间微调)都折进栈顶
        // 同一条撤销记录:分段确认的输入法改的是组词的局部,若按整段对齐判合并,
        // 一次组词会被拆成多步撤销。
        let mut merged = false;
        if self.composition_active
            && let Some(previous) = self.undo.last().filter(|edit| {
                let composed_end = edit.start + edit.inserted.len();
                range.start >= edit.start && range.end <= composed_end
            })
        {
            let head = previous
                .inserted
                .get(..range.start - previous.start)
                .unwrap_or_default()
                .to_owned();
            let tail = previous
                .inserted
                .get(range.end - previous.start..)
                .unwrap_or_default()
                .to_owned();
            let folded = format!("{head}{text}{tail}");
            if let Some(previous) = self.undo.last_mut() {
                previous.inserted = folded;
                merged = true;
            }
        }
        if !merged {
            self.undo.push(TextEdit {
                start: range.start,
                removed: self.value.get(range.clone()).unwrap_or_default().to_owned(),
                inserted: text.to_owned(),
                selection: self.selected_range.clone(),
            });
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.value.replace_range(range.clone(), text);
        let caret = range.start + text.len();
        self.selected_range = caret..caret;
        self.reveal_caret();
        cx.notify();
    }

    fn reveal_caret(&mut self) {
        if let Some(state) = self.multiline.as_mut() {
            state.reveal_caret = true;
        }
    }

    fn undo_edit(&mut self, redo: bool, cx: &mut Context<Self>) {
        let edit = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        };
        let Some(edit) = edit else {
            return;
        };
        let (before, after) = if redo {
            (&edit.removed, &edit.inserted)
        } else {
            (&edit.inserted, &edit.removed)
        };
        let range = edit.start..edit.start + before.len();
        if self.value.get(range.clone()) != Some(before.as_str()) {
            eprintln!("输入撤销状态与当前文本不一致，已跳过失效编辑");
            return;
        }
        self.value.replace_range(range, after);
        self.selected_range = if redo {
            let caret = edit.start + edit.inserted.len();
            caret..caret
        } else {
            edit.selection.clone()
        };
        self.marked_range = None;
        self.composition_active = false;
        if redo {
            self.undo.push(edit);
        } else {
            self.redo.push(edit);
        }
        self.reveal_caret();
        cx.notify();
    }

    fn move_vertically(&mut self, down: bool, extend: bool) {
        let Some(layout) = self
            .multiline
            .as_ref()
            .and_then(|state| state.layout.clone())
        else {
            return;
        };
        let position = layout.position(self.selected_range.end);
        let target = point(
            position.x,
            (position.y + layout.line_height * if down { 1.0 } else { -1.0 }).max(px(0.0)),
        );
        self.move_to(layout.index(target), extend);
    }

    fn move_to(&mut self, offset: usize, extend: bool) {
        let offset = self.value.floor_char_boundary(offset.min(self.value.len()));
        if extend {
            self.selected_range.end = offset;
        } else {
            self.selected_range = offset..offset;
        }
        self.reveal_caret();
    }

    fn move_caret_left(&mut self, extend: bool) {
        let (start, end) = (self.selected_range.start, self.selected_range.end);
        if extend {
            // Shift 扩选:锚(start)不动,头(end)向左退一个字素。
            let target = self
                .value
                .get(..end)
                .unwrap_or_default()
                .grapheme_indices(true)
                .next_back()
                .map(|(index, _)| index)
                .unwrap_or(end);
            self.selected_range.end = target;
            return;
        }
        if start != end {
            self.move_to(start.min(end), false);
            return;
        }
        let target = self
            .value
            .get(..start)
            .unwrap_or_default()
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
            let target = self
                .value
                .get(from..)
                .unwrap_or_default()
                .grapheme_indices(true)
                .nth(1)
                .map(|(index, _)| from + index)
                .unwrap_or(self.value.len());
            self.selected_range.end = target;
            return;
        }
        if start != end {
            self.move_to(start.max(end), false);
            return;
        }
        let from = start;
        let target = self
            .value
            .get(from..)
            .unwrap_or_default()
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
            let start = self
                .value
                .get(..selection.start)
                .unwrap_or_default()
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
            let end = self
                .value
                .get(selection.end..)
                .unwrap_or_default()
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
            // 地址和密钥保持单行；指令编辑保留粘贴内容的段落。
            let text = if self.multiline.is_some() {
                text
            } else {
                text.replace(['\r', '\n'], " ")
            };
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
        if self.marked_range.is_some() && matches!(key.as_str(), "enter" | "escape") {
            cx.stop_propagation();
            return;
        }
        match key.as_str() {
            "up" | "down" if self.multiline.is_some() => {
                self.move_vertically(key == "down", shift);
                window.prevent_default();
                cx.stop_propagation();
            }
            "z" if secondary => {
                self.undo_edit(shift, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            "y" if secondary => {
                self.undo_edit(true, cx);
                window.prevent_default();
                cx.stop_propagation();
            }
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
                let start = if self.multiline.is_some() && !secondary {
                    self.value.get(..self.selected_range.end).unwrap_or_default().rfind('\n').map_or(0, |offset| offset + 1)
                } else { 0 };
                self.move_to(start, shift);
                window.prevent_default();
                cx.stop_propagation();
            }
            "end" => {
                let end = if self.multiline.is_some() && !secondary {
                    self.value.get(self.selected_range.end..).and_then(|tail| tail.find('\n'))
                        .map_or(self.value.len(), |offset| self.selected_range.end + offset)
                } else { self.value.len() };
                self.move_to(end, shift);
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
                if self.multiline.is_some() {
                    self.replace_range(self.normalized_selection(), "\n", cx);
                }
                window.prevent_default();
                cx.stop_propagation();
            }
            // 其余按键(含普通字符、IME 组词键)交给平台输入路由,
            // 最终从 EntityInputHandler 进来。
            _ => return,
        }
        self.reveal_caret();
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
    text.get(..text.floor_char_boundary(utf8_offset.min(text.len())))
        .unwrap_or_default()
        .chars()
        .map(|ch| ch.len_utf16())
        .sum()
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
        *actual_range =
            Some(utf8_to_utf16_offset(&self.value, start)..utf8_to_utf16_offset(&self.value, end));
        Some(self.value.get(start..end).unwrap_or_default().to_owned())
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
            reversed: self.selected_range.start > self.selected_range.end,
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

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        self.composition_active = false;
        // macOS 失焦/输入法取消组词后没有后续提交,不主动请求重绘的话
        // 组词下划线要等下一次无关重绘才消失。
        cx.notify();
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
        self.composition_active = false;
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
        self.composition_active = true;
        let marked_start = range.start;
        let marked_end = range.start + new_text.len();
        self.marked_range = Some(marked_start..marked_end);
        if let Some(selected) = new_selected_range {
            self.selected_range = marked_start + utf16_to_utf8_offset(new_text, selected.start)
                ..marked_start + utf16_to_utf8_offset(new_text, selected.end);
        }
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let source = utf16_to_utf8_offset(&self.value, range.start);
        if let Some(state) = &self.multiline
            && let Some(layout) = &state.layout
        {
            let bounds = self.last_bounds.unwrap_or(bounds);
            let position = layout.position(source);
            return Some(Bounds::new(
                bounds.origin + position - point(px(0.0), state.scroll_y),
                size(px(2.0), layout.line_height),
            ));
        }
        let offset = self.display_offset(source);
        let x = self
            .last_line
            .as_ref()
            .map(|line| line.x_for_index(offset))
            .unwrap_or(px(0.0));
        let bounds = self.last_bounds.unwrap_or(bounds);
        Some(Bounds::new(
            point(bounds.left() + x - px(self.scroll_x), bounds.top()),
            size(px(2.0), bounds.size.height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(utf8_to_utf16_offset(
            &self.value,
            self.index_for_point(position),
        ))
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<crate::theme::ThemeManager>().derived_theme(cx);
        let focused = self.focus.is_focused(window);
        div()
            .id("text-field")
            .track_focus(&self.focus)
            .w_full()
            .min_w_0()
            .min_h(px(32.0))
            .px(px(10.0))
            .py(px(4.0))
            .flex()
            .items_center()
            .rounded(px(theme.dimensions.menu_item_radius))
            .border_1()
            .border_color(if focused {
                theme.colors.dialog_primary_button_bg
            } else {
                theme.colors.dialog_border
            })
            .bg(theme.colors.editor_background)
            .text_size(px(theme.typography.dialog_body_size))
            .font(font(crate::config::EditorSettings::fonts(cx).ui_family))
            .child(if self.multiline.is_some() {
                multiline::TextAreaElement { input: cx.entity() }.into_any_element()
            } else {
                element::TextFieldElement { input: cx.entity() }.into_any_element()
            })
            .on_scroll_wheel(cx.listener(|input, event: &ScrollWheelEvent, _, cx| {
                let Some(state) = input.multiline.as_mut() else {
                    return;
                };
                let Some((bounds, layout)) = input.last_bounds.zip(state.layout.as_ref()) else {
                    return;
                };
                let next = (state.scroll_y - event.delta.pixel_delta(layout.line_height).y)
                    .max(px(0.0))
                    .min((layout.height - bounds.size.height).max(px(0.0)));
                if next != state.scroll_y {
                    state.scroll_y = next;
                    state.reveal_caret = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|input, event: &MouseDownEvent, window, cx| {
                    let offset = input.index_for_point(event.position);
                    input.move_to(offset, event.modifiers.shift);
                    input.dragging = true;
                    window.focus(&input.focus, cx);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|input, event: &MouseMoveEvent, _, cx| {
                if input.dragging && event.pressed_button == Some(MouseButton::Left) {
                    input.move_to(input.index_for_point(event.position), true);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|input, _, _, _| input.dragging = false),
            )
            // on_mouse_up 只在释放点命中本元素时触发;框内按下、框外释放的拖动
            // 靠捕获阶段的 out 钩子复位 dragging,否则残留的拖动态会让之后
            // 路过本框的其他拖动改写选区。
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|input, _, _, _| input.dragging = false),
            )
            .on_key_down(cx.listener(Self::handle_key_down))
    }
}

#[cfg(test)]
mod tests;
