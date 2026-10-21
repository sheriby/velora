//! vim 操作符执行：删除/修改/复制/缩进/大小写、寄存器、粘贴、标记、
//! 宏跳转表、替换模式与插入会话。
//!
//! 写路径一律复用应用既有入口：块内改写走 `replace_text_in_visible_range`
//! （带撤销捕获），跨块合并复刻 `RequestMergeFromNext` 的结构协议，整块
//! 删除经空块退格事件走 `RequestDelete`，换行/插行走块自己的 `on_newline`。
//! 行级复制按显示文本拼（块边界即 '\n'），不读缓冲区映射。

use std::collections::VecDeque;

use gpui::{App, Context, Entity, EntityInputHandler as _, Window};

use crate::components::{Block, Delete, Newline, UndoCaptureKind};
use crate::editor::Editor;

use super::motion::{next_grapheme_start, prev_grapheme_start, VimDocStream, VimPosition};
use super::{
    DotRecord, InsertPlacement, OperatorTarget, VimKey, VimMode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operator {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
    Lowercase,
    Uppercase,
    SwapCase,
}

/// 文本对象（iw/aw/i"/a" 等）。范围解析见 `vim_text_object_range`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextObject {
    Word { big: bool, inner: bool },
    Sentence { inner: bool },
    Paragraph { inner: bool },
    Quote(char, bool),
    Bracket(char, bool),
}

impl TextObject {
    pub(super) fn parse(inner: bool, ch: Option<char>) -> Option<Self> {
        let ch = ch?;
        Some(match ch {
            'w' => Self::Word { big: false, inner },
            'W' => Self::Word { big: true, inner },
            's' => Self::Sentence { inner },
            'p' => Self::Paragraph { inner },
            '"' | '\'' | '`' => Self::Quote(ch, inner),
            '(' | ')' | 'b' => Self::Bracket('(', inner),
            '[' | ']' => Self::Bracket('[', inner),
            '{' | '}' | 'B' => Self::Bracket('{', inner),
            '<' | '>' => Self::Bracket('<', inner),
            _ => return None,
        })
    }
}

/// 寄存器内容：字符级（不含换行结构）或行级。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegisterContent {
    pub text: String,
    pub linewise: bool,
}

/// vim 寄存器组。`"`（未命名）、`0`（最近复制）、`1-9`（删除移位）、`-`（行内
/// 小删除）、`a-z`（具名，大写追加）、`+`（系统剪贴板，读写直通不缓存）。
#[derive(Default)]
pub(crate) struct VimRegisters {
    unnamed: Option<RegisterContent>,
    yank_register: Option<RegisterContent>,
    small_delete: Option<RegisterContent>,
    numbered: VecDeque<RegisterContent>,
    named: std::collections::BTreeMap<char, RegisterContent>,
}

impl VimRegisters {
    pub(super) fn read(&self, name: char, cx: &App, use_system_clipboard: bool) -> Option<RegisterContent> {
        match name {
            '"' => self.unnamed.clone(),
            '0' => self.yank_register.clone(),
            '1'..='9' => {
                let index = name as usize - '1' as usize;
                self.numbered.get(index).cloned()
            }
            '-' => self.small_delete.clone(),
            '_' => None,
            '+' => cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .map(|text| RegisterContent { text, linewise: false }),
            _ if name.is_ascii_lowercase() => self.named.get(&name).cloned(),
            _ if name.is_ascii_uppercase() => self.named.get(&name.to_ascii_lowercase()).cloned(),
            _ if use_system_clipboard => cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .map(|text| RegisterContent { text, linewise: false }),
            _ => None,
        }
    }

    /// 写寄存器：`"` 未命名、`a-z` 具名（大写追加）、`0`/`1-9`/`-` 按 vim 的
    /// 自动移位规则填（显式给名时不碰编号链），`"` 与 `+` 按设置同步剪贴板。
    pub(super) fn write(
        &mut self,
        name: char,
        content: RegisterContent,
        is_yank: bool,
        cx: &App,
        use_system_clipboard: bool,
    ) {
        if name != '_' {
            self.unnamed = Some(content.clone());
        }
        if name.is_ascii_uppercase() {
            let key = name.to_ascii_lowercase();
            let merged = match self.named.get(&key) {
                Some(previous) => RegisterContent {
                    text: format!("{}{}", previous.text, content.text),
                    linewise: previous.linewise || content.linewise,
                },
                None => content.clone(),
            };
            self.named.insert(key, merged);
        } else if name.is_ascii_lowercase() {
            self.named.insert(name, content.clone());
        }
        if name == '"' {
            if is_yank {
                self.yank_register = Some(content.clone());
            } else if content.linewise || content.text.contains('\n') {
                self.numbered.push_front(content.clone());
                self.numbered.truncate(9);
            } else {
                self.small_delete = Some(content.clone());
            }
        }
        if use_system_clipboard && (name == '"' || name == '+') {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(content.text.clone()));
        }
    }
}

/// 操作目标区间（显示坐标；linewise 表示盖住整硬行）。
#[derive(Clone, Debug)]
pub(super) struct VimRange {
    pub start: VimPosition,
    pub end: VimPosition,
    pub linewise: bool,
}

impl Editor {
    /// 活动块的只读信息包（避免散落的 read/update 交替）。
    pub(super) fn vim_active_block(&self) -> Option<Entity<Block>> {
        let id = self.active_entity_id?;
        self.focusable_entity_by_id(id)
    }

    // ---- 操作符主入口 ----

    pub(super) fn vim_apply_operator_inner(
        &mut self,
        operator: Operator,
        target: OperatorTarget,
        count: u32,
        register: Option<char>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entities = self.vim_navigation_entities(cx);
        let Some(cursor) = self.vim_current_position(&entities, cx) else {
            return;
        };
        let register = register.unwrap_or('"');
        let range: Option<VimRange> = match target {
            OperatorTarget::CurrentLines => self.vim_lines_range(&entities, cursor, count.max(1), cx),
            OperatorTarget::Selection => super::visual::vim_selection_as_range(self, &entities, cx),
            OperatorTarget::Motion(motion, motion_count) => {
                self.vim_motion_range(
                    &entities,
                    cursor,
                    operator,
                    motion,
                    motion_count.max(1).max(count.max(1)),
                    cx,
                )
            }
            OperatorTarget::TextObject(object) => {
                self.vim_text_object_range(&entities, cursor, object, cx)
            }
        };
        let Some(range) = range else {
            return;
        };
        match operator {
            Operator::Yank => {
                self.vim_yank_range(&entities, &range, register, cx);
            }
            Operator::Delete => {
                self.vim_delete_range(&entities, &range, register, window, cx);
                self.vim_place_cursor_after_delete(&range);
            }
            Operator::Change => {
                self.vim_delete_range(&entities, &range, register, window, cx);
                self.vim_enter_insert_inner(InsertPlacement::AtCursor, 0, &[], window, cx);
            }
            Operator::Indent | Operator::Outdent => {
                self.vim_indent_range(&entities, &range, operator == Operator::Indent, window, cx);
            }
            Operator::Lowercase | Operator::Uppercase | Operator::SwapCase => {
                self.vim_case_range(&entities, &range, operator, cx);
            }
        }
        let _ = cx;
    }

    /// 光标落点：字符级删到词首式回退，行级落到段首。
    fn vim_place_cursor_after_delete(&mut self, range: &VimRange) {
        // 删除实现里已把光标放好；这里留作行级删除后的钳制钩子。
        let _ = range;
    }

    // ---- 目标区间解析 ----

    /// 光标所在硬行起 N 个硬行（可跨块）。
    fn vim_lines_range(
        &self,
        entities: &[Entity<Block>],
        cursor: VimPosition,
        count: u32,
        cx: &App,
    ) -> Option<VimRange> {
        let stream = VimDocStream::new(entities, cx);
        let current = cursor;
        // 起点行首。
        let first_line = stream.hard_line_range(current.block_index, current.offset);
        let start = VimPosition { block_index: current.block_index, offset: first_line.start };
        let mut end = start;
        for _ in 0..count.max(1) {
            let line = stream.hard_line_range(end.block_index, end.offset);
            let text = stream.text_for_line(end.block_index);
            if line.end < text.len() {
                // 块内还有下一硬行。
                end = VimPosition { block_index: end.block_index, offset: line.end + 1 };
            } else if end.block_index + 1 < entities.len() {
                end = VimPosition { block_index: end.block_index + 1, offset: 0 };
            } else {
                end = VimPosition { block_index: end.block_index, offset: text.len() };
                break;
            }
        }
        Some(VimRange { start, end, linewise: true })
    }

    /// 动作区间：字符级（按 inclusive/exclusive 调整）或行级（j/k）。
    fn vim_motion_range(
        &self,
        entities: &[Entity<Block>],
        cursor: VimPosition,
        operator: Operator,
        motion: super::motion::Motion,
        count: u32,
        cx: &App,
    ) -> Option<VimRange> {
        use super::motion::Motion;
        // cw 特例：只有修改操作符按 ce 处理（vim 的 cw 不吃尾随空白）；
        // dw/yw 仍按 w 的排他区间吃空格。
        let motion = match motion {
            Motion::WordForward { big: false }
                if operator == Operator::Change
                    && VimDocStream::new(entities, cx).class_at_public(cursor, false) != 0 =>
            {
                Motion::WordEnd { big: false }
            }
            other => other,
        };
        if matches!(motion, Motion::Down | Motion::Up) {
            return self.vim_lines_range(entities, cursor, count.max(1), cx);
        }
        let kind = motion.kind();
        let stream = VimDocStream::new(entities, cx);
        let mut target = cursor;
        match motion {
            Motion::Left | Motion::Right | Motion::ForwardChars | Motion::BackwardChars => {
                let forward = !matches!(motion, Motion::Left | Motion::BackwardChars);
                for _ in 0..count.max(1) {
                    let next = if forward {
                        stream.next_grapheme(target)
                    } else {
                        stream.prev_grapheme(target)
                    };
                    target = next?;
                }
                if !forward {
                    return Some(VimRange {
                        start: target,
                        end: cursor,
                        linewise: false,
                    });
                }
            }
            Motion::HardLineStart | Motion::FirstNonBlank => {
                let line = stream.hard_line_range(cursor.block_index, cursor.offset);
                target = VimPosition { block_index: cursor.block_index, offset: line.start };
            }
            Motion::HardLineEnd | Motion::LastNonBlank | Motion::ColumnNo(_) => {
                // 结尾类动作按普通移动算目标。
                let saved = self.vim_line_target_offset(entities, cursor, motion, cx);
                target = saved;
            }
            _ => {
                // 词/查找/段落/句子：按移动函数算目标。
                target = self.vim_resolve_motion_target(entities, cursor, motion, count, cx)?;
            }
        }
        let end = if kind == super::motion::MotionKind::Inclusive {
            stream.next_grapheme(target).unwrap_or(target)
        } else {
            target
        };
        Some(VimRange { start: cursor, end, linewise: false })
    }

    fn vim_resolve_motion_target(
        &self,
        entities: &[Entity<Block>],
        cursor: VimPosition,
        motion: super::motion::Motion,
        count: u32,
        cx: &App,
    ) -> Option<VimPosition> {
        use super::motion::Motion;
        let stream = VimDocStream::new(entities, cx);
        let mut current = cursor;
        match motion {
            Motion::WordForward { big } => {
                for _ in 0..count.max(1) {
                    current = stream.word_forward(current, big)?;
                }
            }
            Motion::WordBackward { big } => {
                for _ in 0..count.max(1) {
                    current = stream.word_backward(current, big)?;
                }
            }
            Motion::WordEnd { big } => {
                for _ in 0..count.max(1) {
                    current = stream.word_end(current, big)?;
                }
            }
            Motion::WordEndBackward { big } => {
                for _ in 0..count.max(1) {
                    current = stream.word_end_backward(current, big)?;
                }
            }
            Motion::FindChar { ch, reverse, till } => {
                for _ in 0..count.max(1) {
                    current = if reverse {
                        stream.find_char_backward(current, ch, till)?
                    } else {
                        stream.find_char_forward(current, ch, till)?
                    };
                }
            }
            Motion::MatchPair => current = stream.match_pair(current)?,
            Motion::ParagraphForward | Motion::ParagraphBackward => {
                let forward = motion == Motion::ParagraphForward;
                return self.vim_paragraph_landing(entities, current, forward, cx);
            }
            Motion::SentenceForward | Motion::SentenceBackward => {
                let forward = motion == Motion::SentenceForward;
                for _ in 0..count.max(1) {
                    current = super::motion::sentence_step(&stream, current, forward)?;
                }
            }
            _ => return None,
        }
        Some(current)
    }

    fn vim_line_target_offset(
        &self,
        entities: &[Entity<Block>],
        cursor: VimPosition,
        motion: super::motion::Motion,
        cx: &App,
    ) -> VimPosition {
        use super::motion::Motion;
        let stream = VimDocStream::new(entities, cx);
        let line = stream.hard_line_range(cursor.block_index, cursor.offset);
        let text = stream.text_for_line(cursor.block_index);
        match motion {
            Motion::HardLineEnd => {
                let offset = if line.end > line.start {
                    prev_grapheme_start(text, line.end).unwrap_or(line.start)
                } else {
                    line.start
                };
                VimPosition { block_index: cursor.block_index, offset }
            }
            Motion::LastNonBlank => {
                let mut offset = if line.end > line.start {
                    prev_grapheme_start(text, line.end).unwrap_or(line.start)
                } else {
                    line.start
                };
                while offset > line.start {
                    let Some(prev) = prev_grapheme_start(text, offset) else { break };
                    if !text[prev..offset].chars().all(|ch| ch.is_whitespace()) {
                        break;
                    }
                    offset = prev;
                }
                VimPosition { block_index: cursor.block_index, offset }
            }
            Motion::ColumnNo(column) => {
                let mut offset = line.start;
                let mut index = 1u32;
                while index < column {
                    let Some(next) = next_grapheme_start(text, offset) else { break };
                    if next >= line.end { break; }
                    offset = next;
                    index += 1;
                }
                VimPosition { block_index: cursor.block_index, offset }
            }
            _ => cursor,
        }
    }

    /// 文本对象区间。
    pub(super) fn vim_text_object_range(
        &self,
        entities: &[Entity<Block>],
        cursor: VimPosition,
        object: TextObject,
        cx: &App,
    ) -> Option<VimRange> {
        let stream = VimDocStream::new(entities, cx);
        let text = stream.text_for_line(cursor.block_index).to_string();
        match object {
            TextObject::Word { big, inner } => {
                let class = |offset: usize| -> u8 {
                    let end = next_grapheme_start(&text, offset).unwrap_or(text.len());
                    let grapheme = &text[offset..end];
                    if big {
                        if grapheme.chars().next().is_some_and(|ch| ch.is_whitespace()) { 0 } else { 1 }
                    } else {
                        super::motion::word_class_public(grapheme)
                    }
                };
                if cursor.offset >= text.len() {
                    return None;
                }
                let current_class = class(cursor.offset);
                let mut start = cursor.offset;
                while start > 0 {
                    let Some(prev) = prev_grapheme_start(&text, start) else { break };
                    if class(prev) != current_class { break; }
                    start = prev;
                }
                let mut end = cursor.offset;
                while let Some(next) = next_grapheme_start(&text, end) {
                    if next >= text.len() || class(next) != current_class { break; }
                    end = next;
                }
                let end = next_grapheme_start(&text, end).unwrap_or(text.len());
                if !inner {
                    // aw：优先吃尾随空白，行尾则吃前导空白。
                    let mut extended_end = end;
                    while let Some(next) = next_grapheme_start(&text, extended_end) {
                        let stop = next_grapheme_start(&text, next).unwrap_or(text.len());
                        if text[next..stop].chars().all(|ch| ch.is_whitespace()) {
                            extended_end = stop;
                        } else {
                            break;
                        }
                    }
                    if extended_end > end {
                        return Some(VimRange {
                            start: VimPosition { block_index: cursor.block_index, offset: start },
                            end: VimPosition { block_index: cursor.block_index, offset: extended_end },
                            linewise: false,
                        });
                    }
                    let mut extended_start = start;
                    while extended_start > 0 {
                        let Some(prev) = prev_grapheme_start(&text, extended_start) else { break };
                        if text[prev..extended_start].chars().all(|ch| ch.is_whitespace()) {
                            extended_start = prev;
                        } else {
                            break;
                        }
                    }
                    return Some(VimRange {
                        start: VimPosition { block_index: cursor.block_index, offset: extended_start },
                        end: VimPosition { block_index: cursor.block_index, offset: end },
                        linewise: false,
                    });
                }
                Some(VimRange {
                    start: VimPosition { block_index: cursor.block_index, offset: start },
                    end: VimPosition { block_index: cursor.block_index, offset: end },
                    linewise: false,
                })
            }
            TextObject::Quote(quote, inner) => {
                let line = stream.hard_line_range(cursor.block_index, cursor.offset);
                // 找光标两侧最近的同类引号对。
                let positions: Vec<usize> = line
                    .clone()
                    .filter(|byte| text.as_bytes().get(*byte) == Some(&(quote as u8)))
                    .collect();
                let mut pair: Option<(usize, usize)> = None;
                for window in positions.windows(2) {
                    if window[0] <= cursor.offset && cursor.offset <= window[1] {
                        pair = Some((window[0], window[1]));
                        break;
                    }
                }
                let (open, close) = pair?;
                if inner {
                    let inner_start = next_grapheme_start(&text, open).unwrap_or(open + 1);
                    Some(VimRange {
                        start: VimPosition { block_index: cursor.block_index, offset: inner_start.min(close) },
                        end: VimPosition { block_index: cursor.block_index, offset: close },
                        linewise: false,
                    })
                } else {
                    let after_close = next_grapheme_start(&text, close).unwrap_or(close + 1);
                    Some(VimRange {
                        start: VimPosition { block_index: cursor.block_index, offset: open },
                        end: VimPosition { block_index: cursor.block_index, offset: after_close },
                        linewise: false,
                    })
                }
            }
            TextObject::Bracket(open, inner) => {
                let close = match open {
                    '(' => ')',
                    '[' => ']',
                    '{' => '}',
                    '<' => '>',
                    _ => return None,
                };
                // 块内扫描配对（显示文本内，嵌套计数）。
                let mut depth = 0i32;
                let mut open_at: Option<usize> = None;
                let mut close_at: Option<usize> = None;
                let mut offset = 0usize;
                while offset < text.len() {
                    let end = next_grapheme_start(&text, offset).unwrap_or(text.len());
                    let ch = text[offset..end].chars().next().unwrap_or(' ');
                    if ch == open {
                        if depth == 0 {
                            open_at = Some(offset);
                        }
                        depth += 1;
                    } else if ch == close {
                        depth -= 1;
                        if depth == 0 {
                            close_at = Some(offset);
                            // 光标在 [open, close] 内即命中。
                            if open_at.is_some_and(|at| at <= cursor.offset && cursor.offset <= offset + 1)
                            {
                                break;
                            }
                            open_at = None;
                        }
                    }
                    offset = end;
                }
                let (open, close) = (open_at?, close_at?);
                if inner {
                    let inner_start = next_grapheme_start(&text, open).unwrap_or(open + 1);
                    Some(VimRange {
                        start: VimPosition { block_index: cursor.block_index, offset: inner_start.min(close) },
                        end: VimPosition { block_index: cursor.block_index, offset: close },
                        linewise: false,
                    })
                } else {
                    let after_close = next_grapheme_start(&text, close).unwrap_or(close + 1);
                    Some(VimRange {
                        start: VimPosition { block_index: cursor.block_index, offset: open },
                        end: VimPosition { block_index: cursor.block_index, offset: after_close },
                        linewise: false,
                    })
                }
            }
            TextObject::Paragraph { inner } => {
                // 段落 = 块。ip = 本块硬行全体；ap 追加后随空块。
                let length = text.len();
                let mut end_block = cursor.block_index;
                let mut end_offset = length;
                if !inner {
                    let next_empty = end_block + 1 < entities.len()
                        && entities[end_block + 1].read(cx).display_text().is_empty();
                    if next_empty {
                        end_block += 1;
                        end_offset = 0;
                    }
                }
                Some(VimRange {
                    start: VimPosition { block_index: cursor.block_index, offset: 0 },
                    end: VimPosition { block_index: end_block, offset: end_offset },
                    linewise: true,
                })
            }
            TextObject::Sentence { inner } => {
                let stream_ref = VimDocStream::new(entities, cx);
                let before = super::motion::sentence_step(&stream_ref, cursor, false)?;
                let after = super::motion::sentence_step(&stream_ref, cursor, true)?;
                let _ = inner;
                Some(VimRange { start: before, end: after, linewise: false })
            }
        }
    }

    // ---- 复制 / 删除 / 缩进 / 大小写 ----

    pub(super) fn vim_yank_range(
        &mut self,
        entities: &[Entity<Block>],
        range: &VimRange,
        register: char,
        cx: &mut Context<Self>,
    ) {
        let mut text = String::new();
        if range.start.block_index == range.end.block_index {
            let block = entities[range.start.block_index].read(cx);
            let display = block.display_text();
            let end = range.end.offset.min(display.len());
            let start = range.start.offset.min(end);
            text.push_str(&display[start..end]);
        } else {
            let first = entities[range.start.block_index].read(cx);
            let display = first.display_text();
            text.push_str(&display[range.start.offset.min(display.len())..]);
            for index in range.start.block_index + 1..range.end.block_index {
                text.push('\n');
                text.push_str(entities[index].read(cx).display_text());
            }
            text.push('\n');
            let last = entities[range.end.block_index].read(cx);
            let display = last.display_text();
            text.push_str(&display[..range.end.offset.min(display.len())]);
        }
        let content = if range.linewise {
            RegisterContent { text: format!("{text}\n"), linewise: true }
        } else {
            RegisterContent { text, linewise: false }
        };
        let use_system = crate::config::EditorSettings::vim(cx).use_system_clipboard;
        self.vim.registers.write(register, content, true, cx, use_system);
        // vim：yank 后光标回到区间起点。
        self.vim_set_cursor(entities, range.start, cx);
    }

    pub(super) fn vim_delete_range(
        &mut self,
        entities: &[Entity<Block>],
        range: &VimRange,
        register: char,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 先复制进寄存器（vim：删除即入寄存器）。
        self.vim_yank_quiet(entities, range, register, cx);
        if range.start.block_index == range.end.block_index {
            self.vim_delete_within_block(
                entities[range.start.block_index].clone(),
                range.start.offset,
                range.end.offset,
                window,
                cx,
            );
            return;
        }
        // 跨块：首块裁尾、末块裁头、中间整块删除，随后两两合并。
        let first = entities[range.start.block_index].clone();
        let last = entities[range.end.block_index].clone();
        self.vim_delete_within_block(
            first.clone(),
            range.start.offset,
            first.read(cx).display_text().len(),
            window,
            cx,
        );
        for index in range.start.block_index + 1..range.end.block_index {
            let middle = entities[index].clone();
            self.vim_delete_whole_block(middle, window, cx);
        }
        let last_len = last.read(cx).display_text().len();
        if last_len > 0 || range.end.offset > 0 {
            self.vim_delete_within_block(last.clone(), 0, range.end.offset.min(last_len), window, cx);
        }
        // 合并接缝：把末块并入首块（若末块还在）。
        let entities_after = self.vim_navigation_entities(cx);
        let first_still_there = entities_after.iter().any(|entity| entity.entity_id() == first.entity_id());
        let last_still_there = entities_after.iter().any(|entity| entity.entity_id() == last.entity_id());
        if first_still_there && last_still_there {
            self.vim_merge_blocks(&first, &last, cx);
        }
        // 光标回到首块的删除点（结构变了，按下标重找）。
        let entities_after = self.vim_navigation_entities(cx);
        if let Some(index) = entities_after
            .iter()
            .position(|entity| entity.entity_id() == first.entity_id())
        {
            self.vim_set_cursor(
                &entities_after,
                VimPosition { block_index: index, offset: range.start.offset },
                cx,
            );
        }
    }

    fn vim_yank_quiet(
        &mut self,
        entities: &[Entity<Block>],
        range: &VimRange,
        register: char,
        cx: &mut Context<Self>,
    ) {
        let mut text = String::new();
        if range.start.block_index == range.end.block_index {
            let block = entities[range.start.block_index].read(cx);
            let display = block.display_text();
            let end = range.end.offset.min(display.len());
            text.push_str(&display[range.start.offset.min(end)..end]);
        } else {
            let first = entities[range.start.block_index].read(cx);
            let display = first.display_text();
            text.push_str(&display[range.start.offset.min(display.len())..]);
            for index in range.start.block_index + 1..range.end.block_index {
                text.push('\n');
                text.push_str(entities[index].read(cx).display_text());
            }
            text.push('\n');
            let last = entities[range.end.block_index].read(cx);
            text.push_str(&last.display_text()[..range.end.offset.min(last.display_text().len())]);
        }
        let content = if range.linewise {
            RegisterContent { text: format!("{text}\n"), linewise: true }
        } else {
            RegisterContent { text, linewise: false }
        };
        let use_system = crate::config::EditorSettings::vim(cx).use_system_clipboard;
        self.vim.registers.write(register, content, false, cx, use_system);
    }

    /// 块内删除（含撤销捕获），光标落在删除点。
    pub(super) fn vim_delete_within_block(
        &mut self,
        block: Entity<Block>,
        start: usize,
        end: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = window;
        let length = block.read(cx).visible_len();
        let end = end.min(length);
        let start = start.min(end);
        if start == end {
            Self::reset_block_cursor(&block, start, cx);
            return;
        }
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
            block.replace_text_in_visible_range(start..end, "", Some(0..0), false, cx);
        });
        self.mark_dirty(cx);
    }

    /// 整块删除：清空后走块自己的退格事件（空块退格 = 结构删除）。
    fn vim_delete_whole_block(
        &mut self,
        block: Entity<Block>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let length = block.read(cx).visible_len();
        if length > 0 {
            block.update(cx, |block, cx| {
                block.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
                block.replace_text_in_visible_range(0..length, "", Some(0..0), false, cx);
            });
            self.mark_dirty(cx);
        }
        Self::reset_block_cursor(&block, 0, cx);
        block.update(cx, |block, cx| {
            block.on_delete_back(&crate::components::DeleteBack, window, cx);
        });
    }

    /// 复刻 RequestMergeFromNext 的结构协议：把 second 并入 first。
    fn vim_merge_blocks(&mut self, first: &Entity<Block>, second: &Entity<Block>, cx: &mut Context<Self>) {
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let cursor_pos = first.read(cx).display_text().len();
        let adopted_children =
            crate::editor::tree::DocumentTree::take_children(second, cx);
        let removed_entity_id = second.entity_id();
        let second_title = second.read(cx).record.title.clone();

        self.document.with_structure_mutation(cx, |document, cx| {
            first.update(cx, {
                let second_title = second_title.clone();
                let adopted_children = adopted_children.clone();
                move |block, cx| {
                    let mut merged = block.record.title.clone();
                    merged.append_tree(second_title);
                    block.record.set_title(merged);
                    block.sync_render_cache();
                    block.children.extend(adopted_children);
                    block.selected_range = cursor_pos..cursor_pos;
                    block.selection_reversed = false;
                    block.marked_range = None;
                    block.vertical_motion_x = None;
                    block.cursor_blink_epoch = std::time::Instant::now();
                    cx.notify();
                }
            });
            let _ = document.remove_block_by_id_raw(removed_entity_id, cx);
        });

        self.focus_block(first.entity_id());
        if self.write_back_structural_change(first, None, cx) {
            self.mark_dirty_written_back(cx);
        } else {
            self.mark_dirty(cx);
        }
        self.rebuild_image_runtimes(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
    }

    /// >>/<<：缩进按块算（列表层级/段首四空格），复用块自己的缩进动作。
    fn vim_indent_range(
        &mut self,
        entities: &[Entity<Block>],
        range: &VimRange,
        indent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for index in range.start.block_index..=range.end.block_index.min(entities.len().saturating_sub(1)) {
            let block = entities[index].clone();
            block.update(cx, |block, cx| {
                if indent {
                    block.on_indent_block(&crate::components::IndentBlock, window, cx);
                } else {
                    block.on_outdent_block(&crate::components::OutdentBlock, window, cx);
                }
            });
        }
    }

    /// gu/gU/g~：块内字符级大小写转换。
    fn vim_case_range(
        &mut self,
        entities: &[Entity<Block>],
        range: &VimRange,
        operator: Operator,
        cx: &mut Context<Self>,
    ) {
        let block = entities[range.start.block_index].clone();
        let length = block.read(cx).visible_len();
        let end = range.end.offset.min(length);
        let start = range.start.offset.min(end);
        if start == end {
            return;
        }
        block.update(cx, |block, cx| {
            let text = block.display_text();
            let transformed: String = text[start..end]
                .chars()
                .map(|ch| match operator {
                    Operator::Lowercase => ch.to_lowercase().next().unwrap_or(ch),
                    Operator::Uppercase => ch.to_uppercase().next().unwrap_or(ch),
                    _ => {
                        if ch.is_lowercase() {
                            ch.to_uppercase().next().unwrap_or(ch)
                        } else {
                            ch.to_lowercase().next().unwrap_or(ch)
                        }
                    }
                })
                .collect();
            block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
            block.replace_text_in_visible_range(start..end, &transformed, Some(0..0), false, cx);
        });
        self.mark_dirty(cx);
    }

    // ---- r / R ----

    pub(super) fn vim_replace_char_inner(
        &mut self,
        ch: char,
        count: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.vim_active_block() else { return };
        let entities = self.vim_navigation_entities(cx);
        let Some(cursor) = self.vim_current_position(&entities, cx) else { return };
        let stream = VimDocStream::new(&entities, cx);
        let mut end = cursor;
        for _ in 0..count.max(1) {
            let Some(next) = stream.next_grapheme(end) else { return };
            if next.block_index != cursor.block_index {
                return;
            }
            end = next;
        }
        let replaced = ch.to_string().repeat(count.max(1) as usize);
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
            block.replace_text_in_visible_range(
                cursor.offset..end.offset,
                &replaced,
                Some(replaced.len().saturating_sub(1)..replaced.len().saturating_sub(1)),
                false,
                cx,
            );
        });
        self.mark_dirty(cx);
        let _ = window;
    }

    pub(super) fn vim_visual_replace_char_inner(
        &mut self,
        ch: char,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entities = self.vim_navigation_entities(cx);
        let Some(range) = super::visual::vim_selection_as_range(self, &entities, cx) else {
            return;
        };
        if range.start.block_index != range.end.block_index {
            return;
        }
        let block = entities[range.start.block_index].clone();
        let length = block.read(cx).visible_len();
        let start = range.start.offset.min(length);
        let end = range.end.offset.min(length).max(start);
        let text = block.read(cx).display_text()[start..end].to_string();
        let mut graphemes = 0usize;
        let mut walk = 0usize;
        while walk < text.len() {
            walk = next_grapheme_start(&text, walk).unwrap_or(text.len());
            graphemes += 1;
        }
        let replaced = ch.to_string().repeat(graphemes.max(1));
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
            block.replace_text_in_visible_range(start..end, &replaced, Some(0..0), false, cx);
        });
        self.mark_dirty(cx);
        self.vim_enter_mode(VimMode::Normal, cx);
        let _ = window;
    }

    /// R 模式逐字符覆写。
    pub(super) fn vim_replace_overtype(
        &mut self,
        ch: char,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.vim_active_block() else { return };
        let (offset, end, original) = {
            let block = block.read(cx);
            let text = block.display_text();
            let offset = block.cursor_offset();
            let line = block.cursor_line_display_range();
            if offset >= line.end.saturating_sub(1) && offset >= text.len() {
                return;
            }
            let end = next_grapheme_start(text, offset).unwrap_or(text.len()).min(line.end.max(offset + 1));
            (offset, end, text[offset..end.min(text.len())].to_string())
        };
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
            block.replace_text_in_visible_range(
                offset..end,
                &ch.to_string(),
                Some(1..1),
                false,
                cx,
            );
        });
        self.mark_dirty(cx);
        self.vim.replace_originals.push((offset, original));
        let _ = window;
    }

    /// R 模式退格：在会话记录内还原原字符。
    pub(super) fn vim_replace_restore(&mut self, cx: &mut Context<Self>) {
        let Some(block) = self.vim_active_block() else { return };
        let offset = block.read(cx).cursor_offset();
        while let Some((recorded_offset, original)) = self.vim.replace_originals.last().cloned() {
            if recorded_offset >= offset {
                let _ = self.vim.replace_originals.pop();
                continue;
            }
            let restored = original.clone();
            let at = recorded_offset;
            let after = next_grapheme_start(&original, 0)
                .map(|len| at + len)
                .unwrap_or(at + original.len());
            block.update(cx, |block, cx| {
                block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
                block.replace_text_in_visible_range(at..after.max(at + 1), &restored, Some(0..0), false, cx);
            });
            Self::reset_block_cursor(&block, at, cx);
            return;
        }
    }

    pub(super) fn vim_replace_session_newline(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vim_insert_newline(window, cx);
        self.vim.replace_originals.clear();
    }

    pub(super) fn vim_finish_replace_session(&mut self, cx: &mut Context<Self>) {
        self.vim.replace_originals.clear();
        let _ = cx;
    }

    pub(super) fn vim_enter_replace_inner(&mut self, cx: &mut Context<Self>) {
        self.vim.replace_originals.clear();
        self.vim_enter_mode(VimMode::Replace, cx);
    }

    // ---- 粘贴 ----

    pub(super) fn vim_paste_inner(
        &mut self,
        register: Option<char>,
        after: bool,
        count: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let register = register.unwrap_or('"');
        let use_system = crate::config::EditorSettings::vim(cx).use_system_clipboard;
        let Some(content) = self.vim.registers.read(register, cx, use_system) else {
            return;
        };
        let content = RegisterContent {
            text: content.text.repeat(count.max(1) as usize),
            linewise: content.linewise,
        };
        if content.linewise || content.text.contains('\n') {
            self.vim_paste_linewise(&content, after, window, cx);
        } else {
            let Some(block) = self.vim_active_block() else { return };
            let offset = block.read(cx).cursor_offset();
            let at = if after { next_grapheme_start(block.read(cx).display_text(), offset).unwrap_or(offset) } else { offset };
            Self::reset_block_cursor(&block, at, cx);
            self.vim_insert_raw_text(&content.text, window, cx);
        }
    }

    /// 行级粘贴：p 在当前块下方逐行开块，P 在上方开块（复用换行 + 落字）。
    fn vim_paste_linewise(
        &mut self,
        content: &RegisterContent,
        after: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lines: Vec<String> = content
            .text
            .split('\n')
            .map(|line| line.to_string())
            .collect();
        let lines = if content.linewise && lines.last().is_some_and(|line| line.is_empty()) {
            lines[..lines.len() - 1].to_vec()
        } else {
            lines
        };
        if lines.is_empty() {
            return;
        }
        if !after {
            // P：先在当前块上方开一个空块。
            if let Some(block) = self.vim_active_block() {
                Self::reset_block_cursor(&block, 0, cx);
                block.update(cx, |block, cx| {
                    block.on_newline(&Newline, window, cx);
                });
            }
        }
        for (index, line) in lines.iter().enumerate() {
            // 逐行：在当前块尾开新块并落字。
            if let Some(block) = self.vim_active_block() {
                if index > 0 || after {
                    let length = block.read(cx).visible_len();
                    Self::reset_block_cursor(&block, length, cx);
                    block.update(cx, |block, cx| {
                        block.on_newline(&Newline, window, cx);
                    });
                }
            }
            if !line.is_empty() {
                self.vim_insert_raw_text(line, window, cx);
            }
        }
    }

    // ---- J 合并行 ----

    pub(super) fn vim_join_inner(
        &mut self,
        count: u32,
        keep_space: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for _ in 0..count.max(1).saturating_sub(1).max(1) {
            let entities = self.vim_navigation_entities(cx);
            let Some(cursor) = self.vim_current_position(&entities, cx) else { return };
            let block = entities[cursor.block_index].clone();
            let text_len = block.read(cx).visible_len();
            let line = block.read(cx).cursor_line_display_range();
            if line.end < text_len {
                // 块内：删掉行界 '\n' 与下一行前导空白，按 keep_space 决定是否补一空格。
                let display = block.read(cx).display_text().to_string();
                let newline_at = line.end;
                let mut next_non_ws = newline_at + 1;
                while next_non_ws < display.len() {
                    let stop = next_grapheme_start(&display, next_non_ws).unwrap_or(display.len());
                    if !display[next_non_ws..stop].chars().all(|ch| ch.is_whitespace()) {
                        break;
                    }
                    next_non_ws = stop;
                }
                let prev_char = display[..newline_at].chars().next_back();
                let next_char = display[next_non_ws.min(display.len())..].chars().next();
                let needs_space = keep_space
                    && prev_char.is_some_and(|ch| !ch.is_whitespace())
                    && next_char.is_some_and(|ch| !ch.is_whitespace());
                let replacement = if needs_space { " ".to_string() } else { String::new() };
                block.update(cx, |block, cx| {
                    block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
                    block.replace_text_in_visible_range(
                        newline_at..next_non_ws,
                        &replacement,
                        Some(replacement.len()..replacement.len()),
                        false,
                        cx,
                    );
                });
                self.mark_dirty(cx);
            } else {
                // 块尾：与下一块合并（正向删除在块尾 = 合并事件）。
                if cursor.block_index + 1 >= entities.len() {
                    return;
                }
                Self::reset_block_cursor(&block, text_len, cx);
                block.update(cx, |block, cx| {
                    block.on_delete(&Delete, window, cx);
                });
            }
        }
    }

    // ---- 标记与跳转表 ----

    pub(super) fn vim_set_mark_inner(&mut self, mark: char, cx: &mut Context<Self>) {
        let entities = self.vim_navigation_entities(cx);
        let Some(cursor) = self.vim_current_position(&entities, cx) else { return };
        let Some(entity) = entities.get(cursor.block_index) else { return };
        let identity = self.buffer.identity();
        self.vim
            .marks
            .entry(identity)
            .or_default()
            .insert(mark, (entity.entity_id(), cursor.offset));
    }

    pub(super) fn vim_goto_mark_inner(
        &mut self,
        mark: char,
        linewise: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let identity = self.buffer.identity();
        let resolved: Option<(gpui::EntityId, usize)> = match mark {
            '<' | '>' => self.vim.last_visual.and_then(|(start, end, _)| {
                let position = if mark == '<' { start } else { end };
                let entities = self.vim_navigation_entities(cx);
                entities
                    .get(position.block_index)
                    .map(|entity| (entity.entity_id(), position.offset))
            }),
            _ => self
                .vim
                .marks
                .get(&identity)
                .and_then(|marks| marks.get(&mark).cloned()),
        };
        let Some((entity_id, offset)) = resolved else {
            return;
        };
        let entities = self.vim_navigation_entities(cx);
        let Some(index) = entities
            .iter()
            .position(|entity| entity.entity_id() == entity_id)
        else {
            return;
        };
        let entities_now = entities.clone();
        let Some(current) = self.vim_current_position(&entities_now, cx) else { return };
        self.vim_push_jumplist(
            entities_now.get(current.block_index).map(|entity| entity.entity_id()),
            current.offset,
            cx,
        );
        let mut target = VimPosition { block_index: index, offset };
        if linewise {
            let stream = VimDocStream::new(&entities, cx);
            let line = stream.hard_line_range(index, offset);
            let text = stream.text_for_line(index);
            let mut first_non_blank = line.start;
            while first_non_blank < line.end {
                let Some(next) = next_grapheme_start(text, first_non_blank) else { break };
                if !text[first_non_blank..next].chars().all(|ch| ch.is_whitespace()) {
                    break;
                }
                first_non_blank = next;
            }
            target.offset = first_non_blank;
        }
        self.vim_set_cursor(&entities, target, cx);
        let _ = window;
    }

    pub(super) fn vim_jumplist_navigate_inner(
        &mut self,
        backward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let identity = self.buffer.identity();
        // 目标下标：向后是 jump_index-1（同位则再减一），向前是 jump_index+1。
        let target_index = if backward {
            if self.vim.jump_index > 0 && self.vim.jump_index >= self.vim.jumplist.len() {
                self.vim.jumplist.len().saturating_sub(1)
            } else {
                let Some(previous) = self.vim.jump_index.checked_sub(1) else {
                    return;
                };
                previous
            }
        } else {
            let next = self.vim.jump_index + 1;
            if next >= self.vim.jumplist.len() {
                return;
            }
            next
        };
        let Some(entry) = self.vim.jumplist.get(target_index).cloned() else {
            return;
        };
        if entry.0 != identity {
            return;
        }
        self.vim.jump_index = target_index;
        let entities = self.vim_navigation_entities(cx);
        let Some(index) = entities
            .iter()
            .position(|entity| entity.entity_id() == entry.1)
        else {
            return;
        };
        self.vim_set_cursor(
            &entities,
            VimPosition { block_index: index, offset: entry.2 },
            cx,
        );
        let _ = window;
    }

    // ---- 插入会话 ----

    pub(super) fn vim_enter_insert_inner(
        &mut self,
        placement: InsertPlacement,
        count: u32,
        entry_keys: &[VimKey],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entities = self.vim_navigation_entities(cx);
        let cursor = self.vim_current_position(&entities, cx);
        if let Some(cursor) = cursor {
            let stream = VimDocStream::new(&entities, cx);
            let block = entities[cursor.block_index].clone();
            match placement {
                InsertPlacement::AtCursor => {}
                InsertPlacement::AfterCursor => {
                    let text_len = block.read(cx).visible_len();
                    let line = stream.hard_line_range(cursor.block_index, cursor.offset);
                    let at = if cursor.offset >= line.end.saturating_sub(1) && cursor.offset >= text_len {
                        text_len
                    } else {
                        next_grapheme_start(block.read(cx).display_text(), cursor.offset)
                            .unwrap_or(text_len)
                    };
                    Self::reset_block_cursor(&block, at, cx);
                }
                InsertPlacement::LineStart | InsertPlacement::FirstColumn => {
                    let line = stream.hard_line_range(cursor.block_index, cursor.offset);
                    let at = if placement == InsertPlacement::FirstColumn {
                        line.start
                    } else {
                        let text = stream.text_for_line(cursor.block_index);
                        let mut offset = line.start;
                        while offset < line.end {
                            let Some(next) = next_grapheme_start(text, offset) else { break };
                            if !text[offset..next].chars().all(|ch| ch.is_whitespace()) {
                                break;
                            }
                            offset = next;
                        }
                        offset
                    };
                    Self::reset_block_cursor(&block, at, cx);
                }
                InsertPlacement::LineEnd => {
                    let line = stream.hard_line_range(cursor.block_index, cursor.offset);
                    Self::reset_block_cursor(&block, line.end.min(block.read(cx).visible_len()), cx);
                }
                InsertPlacement::Below | InsertPlacement::Above => {
                    let line = stream.hard_line_range(cursor.block_index, cursor.offset);
                    let at = if placement == InsertPlacement::Below {
                        line.end.min(block.read(cx).visible_len())
                    } else {
                        line.start
                    };
                    Self::reset_block_cursor(&block, at, cx);
                    block.update(cx, |block, cx| {
                        block.on_newline(&Newline, window, cx);
                    });
                }
                InsertPlacement::LastInsertPosition => {
                    let identity = self.buffer.identity();
                    if let Some((mark_entity, mark_offset)) = self
                        .vim
                        .last_insert_position
                        .filter(|(saved_identity, _, _)| *saved_identity == identity)
                        .map(|(_, mark_entity, mark_offset)| (mark_entity, mark_offset))
                    {
                        let entities = self.vim_navigation_entities(cx);
                        if let Some(index) = entities
                            .iter()
                            .position(|entity| entity.entity_id() == mark_entity)
                        {
                            self.vim_set_cursor(
                                &entities,
                                VimPosition { block_index: index, offset: mark_offset },
                                cx,
                            );
                        }
                    }
                }
            }
        }
        self.vim.insert_typed.clear();
        self.vim.insert_entry_keys = entry_keys.to_vec();
        self.vim.insert_repeat_count = count;
        self.vim_enter_mode(VimMode::Insert, cx);
    }

    pub(super) fn vim_finish_insert_session(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 块状可视 I/A 的多行复制。
        if let Some(targets) = self.vim.block_visual_insert.take() {
            let typed: String = self
                .vim
                .insert_typed
                .iter()
                .filter_map(|key| match key {
                    VimKey::Char(ch) => Some(*ch),
                    _ => None,
                })
                .collect();
            if !typed.is_empty() {
                for (entity_id, range) in targets {
                    if let Some(block) = self.focusable_entity_by_id(entity_id) {
                        block.update(cx, |block, cx| {
                            block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
                            block.replace_text_in_visible_range(
                                range.start..range.start,
                                &typed,
                                None,
                                false,
                                cx,
                            );
                        });
                    }
                }
                self.mark_dirty(cx);
            }
        }
        // 记录 `.` 素材与 gi 位置。
        let entities = self.vim_navigation_entities(cx);
        if let Some(cursor) = self.vim_current_position(&entities, cx) {
            if let Some(entity) = entities.get(cursor.block_index) {
                self.vim.last_insert_position =
                    Some((self.buffer.identity(), entity.entity_id(), cursor.offset));
            }
        }
        if !self.vim.replaying && !self.vim.insert_entry_keys.is_empty() {
            self.vim.dot = Some(DotRecord {
                entry_keys: self.vim.insert_entry_keys.clone(),
                insert_keys: self.vim.insert_typed.clone(),
            });
        }
        // 3o / 3O：再补 count-1 次。
        let repeat = self.vim.insert_repeat_count;
        let entry_keys = self.vim.insert_entry_keys.clone();
        self.vim.insert_repeat_count = 0;
        if repeat > 1 && !self.vim.replaying {
            let record = DotRecord {
                entry_keys,
                insert_keys: self.vim.insert_typed.clone(),
            };
            for _ in 1..repeat {
                self.vim_replay_record(&record, window, cx);
            }
        }
        self.vim.insert_typed.clear();
        self.vim.insert_entry_keys.clear();
    }

    pub(super) fn vim_cursor_left_on_escape(&mut self, cx: &mut Context<Self>) {
        let Some(block) = self.vim_active_block() else { return };
        let offset = block.read(cx).cursor_offset();
        if offset > 0 {
            let display = block.read(cx).display_text();
            let target = prev_grapheme_start(display, offset).unwrap_or(0);
            Self::reset_block_cursor(&block, target, cx);
        }
    }

    pub(super) fn vim_insert_raw_text(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.vim_active_block() else { return };
        block.update(cx, |block, cx| {
            block.replace_text_in_range(None, text, window, cx);
        });
    }

    pub(super) fn vim_insert_from_register(
        &mut self,
        register: char,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let use_system = crate::config::EditorSettings::vim(cx).use_system_clipboard;
        let Some(content) = self.vim.registers.read(register, cx, use_system) else {
            return;
        };
        if content.linewise || content.text.contains('\n') {
            self.vim_paste_linewise(&content, false, window, cx);
        } else {
            self.vim_insert_raw_text(&content.text, window, cx);
        }
    }

    pub(super) fn vim_insert_newline(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.vim_active_block() else { return };
        block.update(cx, |block, cx| {
            block.on_newline(&Newline, window, cx);
        });
    }

    pub(super) fn vim_insert_backspace(&mut self, cx: &mut Context<Self>) {
        let Some(block) = self.vim_active_block() else { return };
        let offset = block.read(cx).cursor_offset();
        if offset > 0 {
            let display = block.read(cx).display_text();
            let start = prev_grapheme_start(display, offset).unwrap_or(0);
            block.update(cx, |block, cx| {
                block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
                block.replace_text_in_visible_range(start..offset, "", Some(0..0), false, cx);
            });
        }
    }

    pub(super) fn vim_delete_insert_to_hard_line_start(&mut self, cx: &mut Context<Self>) {
        let Some(block) = self.vim_active_block() else { return };
        let (start, offset) = {
            let block = block.read(cx);
            (block.cursor_line_display_range().start, block.cursor_offset())
        };
        if offset > start {
            block.update(cx, |block, cx| {
                block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
                block.replace_text_in_visible_range(start..offset, "", Some(0..0), false, cx);
            });
        }
    }

    pub(super) fn vim_delete_insert_word_back(&mut self, cx: &mut Context<Self>) {
        let Some(block) = self.vim_active_block() else { return };
        let offset = block.read(cx).cursor_offset();
        let start = block.read(cx).previous_word_start(offset);
        if start < offset {
            block.update(cx, |block, cx| {
                block.prepare_undo_capture(block.text_edit_undo_kind(), cx);
                block.replace_text_in_visible_range(start..offset, "", Some(0..0), false, cx);
            });
        }
    }
}

// ---- 模块门面（vim.rs 执行器调用）----

pub(super) fn vim_apply_operator(
    editor: &mut Editor,
    operator: Operator,
    target: OperatorTarget,
    count: u32,
    register: Option<char>,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_apply_operator_inner(operator, target, count, register, window, cx);
}

pub(super) fn vim_enter_insert(
    editor: &mut Editor,
    placement: InsertPlacement,
    count: u32,
    entry_keys: &[VimKey],
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_enter_insert_inner(placement, count, entry_keys, window, cx);
}

pub(super) fn vim_enter_replace(editor: &mut Editor, cx: &mut Context<Editor>) {
    editor.vim_enter_replace_inner(cx);
}

pub(super) fn vim_replace_char(
    editor: &mut Editor,
    ch: char,
    count: u32,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_replace_char_inner(ch, count, window, cx);
}

pub(super) fn vim_visual_replace_char(
    editor: &mut Editor,
    ch: char,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_visual_replace_char_inner(ch, window, cx);
}

pub(super) fn vim_paste(
    editor: &mut Editor,
    register: Option<char>,
    after: bool,
    count: u32,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_paste_inner(register, after, count, window, cx);
}

pub(super) fn vim_join(
    editor: &mut Editor,
    count: u32,
    keep_space: bool,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_join_inner(count, keep_space, window, cx);
}

pub(super) fn vim_set_mark(editor: &mut Editor, mark: char, cx: &mut Context<Editor>) {
    editor.vim_set_mark_inner(mark, cx);
}

pub(super) fn vim_goto_mark(
    editor: &mut Editor,
    mark: char,
    linewise: bool,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_goto_mark_inner(mark, linewise, window, cx);
}

pub(super) fn vim_jumplist_navigate(
    editor: &mut Editor,
    backward: bool,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_jumplist_navigate_inner(backward, window, cx);
}
