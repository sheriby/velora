//! vim 动作解析：把 `Motion` 变成光标落点。
//!
//! 位置口径：`VimPosition` 是「导航块列表下标 + 块内显示偏移」。导航块列表
//! 指过滤掉 `renders_nothing` 之后的可见块；跨块扫描（词动作、句子、括号
//! 匹配）都在这个流的显示文本上做，不碰缓冲区映射（那是查找替换才付得起
//! 的整篇成本）。只有 gg/G/{行号} 需要块 → 缓冲行号换算，用 `source_span_of`
//! 的区间表算，不逐块读实体。

use gpui::{App, Context, Window};

use crate::components::{Block, BlockKind};
use crate::editor::{Editor, ScrollPlacement};

use super::{
    FindChar, ScreenCursorKind, ScrollKind, ViewportAlign, VimMode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    Left,
    Right,
    Up,
    Down,
    HardLineStart,
    FirstNonBlank,
    HardLineEnd,
    LastNonBlank,
    WordForward { big: bool },
    WordBackward { big: bool },
    WordEnd { big: bool },
    WordEndBackward { big: bool },
    DocStart,
    DocEnd,
    GoToLine(u32),
    FindChar { ch: char, reverse: bool, till: bool },
    MatchPair,
    ColumnNo(u32),
    ScreenLine(ScreenCursorKind),
    ParagraphBackward,
    ParagraphForward,
    SentenceBackward,
    SentenceForward,
    ForwardChars,
    BackwardChars,
}

impl Motion {
    /// 操作符作用时的区间属性：排他（不含终点）、包含（含终点字符）、行级。
    pub(super) fn kind(self) -> MotionKind {
        match self {
            Motion::WordEnd { .. }
            | Motion::WordEndBackward { .. }
            | Motion::FindChar { till: false, .. }
            | Motion::LastNonBlank
            | Motion::HardLineEnd => MotionKind::Inclusive,
            Motion::Down | Motion::Up => MotionKind::LineWise,
            Motion::MatchPair => MotionKind::Inclusive,
            _ => MotionKind::Exclusive,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MotionKind {
    Exclusive,
    Inclusive,
    LineWise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VimPosition {
    /// 导航块列表（过滤 `renders_nothing` 后）的下标。
    pub block_index: usize,
    pub offset: usize,
}

/// vim 词分类：0 空白，1 词字符（字母数字下划线与 Unicode 字母），2 标点。
fn word_class(grapheme: &str) -> u8 {
    let Some(ch) = grapheme.chars().next() else {
        return 0;
    };
    if ch.is_whitespace() {
        0
    } else if ch.is_alphanumeric() || ch == '_' {
        1
    } else {
        2
    }
}

/// 词分类的公共入口（文本对象解析在 operate.rs，判据仍只此一处）。
pub(super) fn word_class_public(grapheme: &str) -> u8 {
    word_class(grapheme)
}

/// 大词（W/B/E）分类：只有空白与非空白两类。
fn big_word_class(grapheme: &str) -> u8 {
    if grapheme.chars().next().is_some_and(|ch| ch.is_whitespace()) {
        0
    } else {
        1
    }
}

/// 导航块流：跨块的字素步进与读取。构造时过滤 `renders_nothing`。
pub(super) struct VimDocStream<'a> {
    entities: &'a [gpui::Entity<Block>],
    cx: &'a App,
}

impl<'a> VimDocStream<'a> {
    pub(super) fn new(entities: &'a [gpui::Entity<Block>], cx: &'a App) -> Self {
        Self { entities, cx }
    }

    fn block(&self, index: usize) -> Option<&'a gpui::Entity<Block>> {
        self.entities.get(index)
    }

    /// 该块是否为空段落（vim 把空行当独立的词停点）。
    fn block_is_empty(&self, index: usize) -> bool {
        self.block(index)
            .map(|entity| entity.read(self.cx).display_text().is_empty())
            .unwrap_or(true)
    }

    fn text(&self, index: usize) -> &'a str {
        self.block(index)
            .map(|entity| entity.read(self.cx).display_text())
            .unwrap_or("")
    }

    pub(super) fn next_grapheme(&self, position: VimPosition) -> Option<VimPosition> {
        let text = self.text(position.block_index);
        if let Some(next) = next_grapheme_start(text, position.offset) {
            if next < text.len() {
                return Some(VimPosition { block_index: position.block_index, offset: next });
            }
        }
        // 块尾（或空块）→ 下一导航块起点。
        if position.block_index + 1 < self.entities.len() {
            Some(VimPosition { block_index: position.block_index + 1, offset: 0 })
        } else {
            None
        }
    }

    pub(super) fn prev_grapheme(&self, position: VimPosition) -> Option<VimPosition> {
        if position.offset > 0 {
            let text = self.text(position.block_index);
            let offset = prev_grapheme_start(text, position.offset)?;
            return Some(VimPosition { block_index: position.block_index, offset });
        }
        if position.block_index == 0 {
            return None;
        }
        let previous = position.block_index - 1;
        if self.block_is_empty(previous) {
            // 空块：停在其起点（vim 的 b 停在空行上）。
            return Some(VimPosition { block_index: previous, offset: 0 });
        }
        let text = self.text(previous);
        let length = text.len();
        let offset = prev_grapheme_start(text, length).unwrap_or(0);
        Some(VimPosition { block_index: previous, offset })
    }

    /// 位置处的字素；块尾（offset == len）返回 None（视为流边界）。
    pub(super) fn grapheme_at(&self, position: VimPosition) -> Option<&'a str> {
        let text = self.text(position.block_index);
        let start = position.offset;
        if start >= text.len() {
            return None;
        }
        let end = next_grapheme_start(text, start)?;
        Some(&text[start..end])
    }

    fn class_at(&self, position: VimPosition, big: bool) -> u8 {
        match self.grapheme_at(position) {
            Some(grapheme) => {
                if big {
                    big_word_class(grapheme)
                } else {
                    word_class(grapheme)
                }
            }
            None => 0,
        }
    }

    /// 词分类公共入口（operate.rs 的 cw 特例判定用）。
    pub(super) fn class_at_public(&self, position: VimPosition, big: bool) -> u8 {
        self.class_at(position, big)
    }

    /// w：下一个词首（空块算一个独立停点）。
    pub(super) fn word_forward(&self, position: VimPosition, big: bool) -> Option<VimPosition> {
        let mut current = position;
        // 跳过当前词身（若在词中）。
        let start_class = self.class_at(current, big);
        if start_class != 0 {
            loop {
                let Some(next) = self.next_grapheme(current) else {
                    return None;
                };
                if self.class_at(next, big) != start_class {
                    current = next;
                    break;
                }
                current = next;
            }
        }
        // 跳过空白；空块即停。
        loop {
            if self.at_block_start(current) && self.block_is_empty(current.block_index) {
                return Some(current);
            }
            if self.class_at(current, big) != 0 {
                return Some(current);
            }
            let Some(next) = self.next_grapheme(current) else {
                return None;
            };
            current = next;
        }
    }

    /// b：当前/前一个词首。
    pub(super) fn word_backward(&self, position: VimPosition, big: bool) -> Option<VimPosition> {
        let current = position;
        let mut previous = self.prev_grapheme(current)?;
        // 若当前在词中（前一词素同类），先退到词首。
        let current_class = self.class_at(current, big);
        if current_class != 0 && self.class_at(previous, big) == current_class {
            loop {
                let Some(before) = self.prev_grapheme(previous) else {
                    return Some(previous);
                };
                if self.class_at(before, big) != current_class {
                    return Some(previous);
                }
                previous = before;
            }
        }
        // 越过空白，落在非空白词素上；空块本身是停点，不能把它当空白越过。
        while self.class_at(previous, big) == 0 {
            if self.at_block_start(previous) && self.block_is_empty(previous.block_index) {
                return Some(previous);
            }
            let Some(before) = self.prev_grapheme(previous) else {
                return Some(previous);
            };
            previous = before;
        }
        // 再退到该词的词首。
        let class = self.class_at(previous, big);
        loop {
            let Some(before) = self.prev_grapheme(previous) else {
                return Some(previous);
            };
            if self.class_at(before, big) != class {
                return Some(previous);
            }
            previous = before;
        }
    }

    /// e：下一个词尾（落在最后一词素上）。
    pub(super) fn word_end(&self, position: VimPosition, big: bool) -> Option<VimPosition> {
        let mut current = position;
        let Some(mut next) = self.next_grapheme(current) else {
            return None;
        };
        // 空块即词尾停点。
        if self.at_block_start(next) && self.block_is_empty(next.block_index) {
            return Some(next);
        }
        while self.class_at(next, big) == 0 {
            current = next;
            let Some(further) = self.next_grapheme(current) else {
                return None;
            };
            next = further;
            if self.at_block_start(next) && self.block_is_empty(next.block_index) {
                return Some(next);
            }
        }
        // 沿同类词素推进，落在最后一个上。
        let class = self.class_at(next, big);
        current = next;
        loop {
            let Some(further) = self.next_grapheme(current) else {
                return Some(current);
            };
            if self.at_block_start(further)
                && further.block_index != current.block_index
                && self.block_is_empty(further.block_index)
            {
                return Some(current);
            }
            if self.class_at(further, big) != class {
                return Some(current);
            }
            current = further;
        }
    }

    /// ge：前一个词尾。
    pub(super) fn word_end_backward(
        &self,
        position: VimPosition,
        big: bool,
    ) -> Option<VimPosition> {
        let mut previous = self.prev_grapheme(position)?;
        while self.class_at(previous, big) == 0 {
            // 与 b 同一条判据：空块是独立的 e/ge 停点。
            if self.at_block_start(previous) && self.block_is_empty(previous.block_index) {
                return Some(previous);
            }
            let Some(before) = self.prev_grapheme(previous) else {
                return Some(previous);
            };
            previous = before;
        }
        // previous 现在是前一词的最后一词素，即词尾。
        Some(previous)
    }

    fn at_block_start(&self, position: VimPosition) -> bool {
        position.offset == 0
    }

    /// 从位置起在硬行内正向找字符。返回落点（till 时停在目标前一字素）。
    pub(super) fn find_char_forward(
        &self,
        position: VimPosition,
        ch: char,
        till: bool,
    ) -> Option<VimPosition> {
        let line = self.hard_line_range(position.block_index, position.offset);
        let text = self.text(position.block_index);
        let mut offset = position.offset;
        let mut landing: Option<usize> = None;
        while let Some(next) = next_grapheme_start(text, offset) {
            if next >= line.end {
                break;
            }
            let end = next_grapheme_start(text, next).unwrap_or(line.end);
            if text[next..end].contains(ch) {
                landing = Some(if till {
                    // 停在目标前一字素。
                    offset
                } else {
                    next
                });
                break;
            }
            offset = next;
        }
        landing.map(|offset| VimPosition { block_index: position.block_index, offset })
    }

    /// 反向找字符。
    pub(super) fn find_char_backward(
        &self,
        position: VimPosition,
        ch: char,
        till: bool,
    ) -> Option<VimPosition> {
        let line = self.hard_line_range(position.block_index, position.offset);
        let text = self.text(position.block_index);
        let mut offset = position.offset.min(line.end);
        let mut landing: Option<usize> = None;
        while offset > line.start {
            let Some(prev) = prev_grapheme_start(text, offset) else {
                break;
            };
            let end = offset.min(line.end);
            if prev < end && text[prev..end].contains(ch) {
                landing = Some(if till {
                    // 反向 till：停在目标后一字素。
                    (offset).min(line.end)
                } else {
                    prev
                });
                break;
            }
            offset = prev;
        }
        landing.map(|offset| VimPosition { block_index: position.block_index, offset })
    }

    /// 光标所在硬行的显示范围（判据与 shared_hard_line_ranges 同源：按 '\n' 切）。
    pub(super) fn hard_line_range(
        &self,
        block_index: usize,
        offset: usize,
    ) -> std::ops::Range<usize> {
        let Some(entity) = self.block(block_index) else {
            return 0..0;
        };
        let block = entity.read(self.cx);
        if block.cursor_offset() == offset {
            return block.cursor_line_display_range();
        }
        let text = block.display_text();
        let mut start = 0usize;
        let mut end = text.len();
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                if index < offset {
                    start = index + 1;
                } else {
                    end = index;
                    break;
                }
            }
        }
        start..end
    }

    /// 括号匹配：当前位置是括号则配对；否则硬行内向后找第一个括号再配对。
    /// 跨块沿导航流扫描，配对计数嵌套。
    pub(super) fn match_pair(&self, position: VimPosition) -> Option<VimPosition> {
        let (open, close) = match self.grapheme_at(position).and_then(|grapheme| {
            grapheme.chars().next()
        }) {
            Some('(') => ('(', ')'),
            Some(')') => (')', '('),
            Some('[') => ('[', ']'),
            Some(']') => (']', '['),
            Some('{') => ('{', '}'),
            Some('}') => ('}', '{'),
            _ => {
                // 行内找下一个括号。
                let line = self.hard_line_range(position.block_index, position.offset);
                let text = self.text(position.block_index);
                let mut offset = position.offset;
                while let Some(next) = next_grapheme_start(text, offset) {
                    if next >= line.end {
                        break;
                    }
                    let end = next_grapheme_start(text, next)?;
                    let ch = text[next..end].chars().next()?;
                    if matches!(ch, '(' | ')' | '[' | ']' | '{' | '}') {
                        return self.match_pair(VimPosition {
                            block_index: position.block_index,
                            offset: next,
                        });
                    }
                    offset = next;
                }
                return None;
            }
        };
        let forward = matches!(open, '(' | '[' | '{');
        let mut depth = 0usize;
        let mut current = position;
        loop {
            let grapheme = self.grapheme_at(current)?;
            let ch = grapheme.chars().next()?;
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(current);
                }
            }
            current = if forward {
                self.next_grapheme(current)?
            } else {
                self.prev_grapheme(current)?
            };
        }
    }
}

/// 字素工具：起点处字素的后继起点（越界返回 None）。
///
/// 必须按字素簇而不是字符边界步进：`👨‍👩‍👧` 是 6 个 code point 用 ZWJ 连成的
/// 一个簇，按 char 走会把一个人删成半截；`e\u{0301}` 同理。
pub(super) fn next_grapheme_start(text: &str, offset: usize) -> Option<usize> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut start = offset.min(text.len());
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut clusters = text[start..].grapheme_indices(true);
    clusters.next()?;
    // 与按字符步进的老口径同一约定：最后一个字素之后是「文本末尾」，只有 offset
    // 已在末尾时才返回 None；调用方靠这个区分「块内末字」与「无后继」。
    Some(clusters.next().map_or(text.len(), |(index, _)| start + index))
}

/// 起点处字素的前驱起点。
pub(super) fn prev_grapheme_start(text: &str, offset: usize) -> Option<usize> {
    use unicode_segmentation::UnicodeSegmentation;
    let end = offset.min(text.len());
    if end == 0 {
        return None;
    }
    // 簇最长也就几十字节（ZWJ 家庭、区域指示符成对）：回退一个窗口，再正向切到
    // 窗口内最后一个簇的起点——那正是 offset 之前（或含 offset）的那个字素。
    let mut window = end.saturating_sub(128);
    while window < end && !text.is_char_boundary(window) {
        window += 1;
    }
    let mut start = None;
    for (index, _) in text[window..end].grapheme_indices(true) {
        start = Some(window + index);
    }
    start
}

impl Editor {
    /// 导航块列表（过滤 renders_nothing，与可见快照同源）。
    pub(super) fn vim_navigation_entities(&self, cx: &App) -> Vec<gpui::Entity<Block>> {
        self.document
            .visible_blocks()
            .iter()
            .filter(|visible| !visible.entity.read(cx).renders_nothing())
            .map(|visible| visible.entity.clone())
            .collect()
    }

    /// 文本动作的作用域：光标落在表格单元格里时缩成这一格。
    ///
    /// 单元格是独立实体，不在导航块列表里；不缩作用域，格内动作既找不到
    /// 光标位置，操作符还可能按文档下标误伤别的块。缩成单格后所有行内
    /// 动作与操作符都只落在格内文字上，跨块删除/合并不会发生。
    pub(super) fn vim_text_scope_entities(&self) -> Option<Vec<gpui::Entity<Block>>> {
        let binding = self
            .active_entity_id
            .and_then(|entity_id| self.table_cell_binding(entity_id))?;
        Some(vec![binding.cell.clone()])
    }

    /// 当前光标位置（导航块坐标）。
    pub(super) fn vim_current_position(
        &self,
        entities: &[gpui::Entity<Block>],
        cx: &App,
    ) -> Option<VimPosition> {
        if matches!(self.vim.mode, VimMode::Visual(_))
            && let Some(focus) = self.vim.visual_focus
        {
            return Some(focus);
        }
        let active_id = self.active_entity_id?;
        let index = entities
            .iter()
            .position(|entity| entity.entity_id() == active_id)?;
        let offset = entities[index].read(cx).cursor_offset();
        Some(VimPosition { block_index: index, offset })
    }

    /// 普通模式落点：清跨块选区，块内直落或跨块聚焦。
    pub(super) fn vim_set_cursor(
        &mut self,
        entities: &[gpui::Entity<Block>],
        position: VimPosition,
        cx: &mut Context<Self>,
    ) {
        let Some(entity) = entities.get(position.block_index) else {
            return;
        };
        if matches!(self.vim.mode, VimMode::Visual(_)) {
            self.vim.visual_focus = Some(position);
        }
        self.clear_cross_block_selection(cx);
        if Some(entity.entity_id()) == self.active_entity_id {
            entity.update(cx, |block, cx| {
                block.move_to(position.offset, cx);
            });
        } else {
            Self::reset_block_cursor(entity, position.offset, cx);
            self.focus_block(entity.entity_id());
        }
        // 视口跟着光标走：块内移动也要能滚，未挂载的跳转目标由 Reveal
        // 的估算路径接近（普通的聚焦只挂 IntoView，不会主动拉视口）。
        self.vim_request_reveal();
    }

    /// 请求把当前活动实体（可以是表格单元格）滚进视口。
    pub(super) fn vim_request_reveal(&mut self) {
        self.pending_scroll_active_block_into_view = true;
        self.pending_scroll_placement = ScrollPlacement::Reveal;
    }

    /// 主移动入口：普通模式按动作移动。
    pub(super) fn vim_move_by(
        &mut self,
        motion: Motion,
        repeat: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 表格单元格是独立实体，不在导航块列表里：hjkl 在表格内按格移动，
        // 到边缘由单元格导航交还给相邻块；其余动作仍按块导航。
        if let Some(binding) = self
            .active_entity_id
            .and_then(|entity_id| self.table_cell_binding(entity_id))
        {
            let step = repeat.max(1) as i32;
            let handled = match motion {
                Motion::Up => {
                    self.focus_table_cell_vertical_neighbor(
                        &binding.table_block,
                        binding.position,
                        -step,
                        cx,
                    );
                    true
                }
                Motion::Down => {
                    self.focus_table_cell_vertical_neighbor(
                        &binding.table_block,
                        binding.position,
                        step,
                        cx,
                    );
                    true
                }
                Motion::Left => {
                    self.focus_table_cell_horizontal_neighbor(
                        &binding.table_block,
                        binding.position,
                        -step,
                        cx,
                    );
                    true
                }
                Motion::Right => {
                    self.focus_table_cell_horizontal_neighbor(
                        &binding.table_block,
                        binding.position,
                        step,
                        cx,
                    );
                    true
                }
                _ => false,
            };
            if handled {
                self.vim_request_reveal();
                return;
            }
        }
        let entities = self
            .vim_text_scope_entities()
            .unwrap_or_else(|| self.vim_navigation_entities(cx));
        let Some(start) = self.vim_current_position(&entities, cx) else {
            return;
        };
        match motion {
            Motion::Up | Motion::Down => {
                let down = motion == Motion::Down;
                let mut current = start;
                for _ in 0..repeat.max(1) {
                    let Some(next) = self.vim_vertical_step(&entities, current, down, cx) else {
                        break;
                    };
                    current = next;
                }
                // 落到表格容器上时直接进第一格：容器本身没有可编辑文字。
                if entities
                    .get(current.block_index)
                    .is_some_and(|entity| entity.read(cx).kind() == BlockKind::Table)
                {
                    let table = entities[current.block_index].clone();
                    if self.focus_table_entry_cell(&table, down, cx) {
                        self.vim_request_reveal();
                        return;
                    }
                }
                self.vim_set_cursor(&entities, current, cx);
            }
            Motion::DocStart | Motion::DocEnd | Motion::GoToLine(_) => {
                let Some(target) = self.vim_document_line_landing(motion, &entities, cx) else {
                    return;
                };
                self.vim_push_jumplist(entities.get(start.block_index).map(|entity| entity.entity_id()), start.offset, cx);
                self.vim_set_cursor(&entities, target, cx);
            }
            Motion::ScreenLine(which) => {
                let Some(target) = self.vim_screen_line(which, &entities, repeat, cx) else {
                    return;
                };
                self.vim_set_cursor(&entities, target, cx);
            }
            Motion::WordForward { big } => {
                let stream = VimDocStream::new(&entities, cx);
                let mut current = start;
                for _ in 0..repeat.max(1) {
                    let Some(next) = stream.word_forward(current, big) else {
                        break;
                    };
                    current = next;
                }
                self.vim_set_cursor(&entities, current, cx);
            }
            Motion::WordBackward { big } => {
                let stream = VimDocStream::new(&entities, cx);
                let mut current = start;
                for _ in 0..repeat.max(1) {
                    let Some(next) = stream.word_backward(current, big) else {
                        break;
                    };
                    current = next;
                }
                self.vim_set_cursor(&entities, current, cx);
            }
            Motion::WordEnd { big } => {
                let stream = VimDocStream::new(&entities, cx);
                let mut current = start;
                for _ in 0..repeat.max(1) {
                    let Some(next) = stream.word_end(current, big) else {
                        break;
                    };
                    current = next;
                }
                self.vim_set_cursor(&entities, current, cx);
            }
            Motion::WordEndBackward { big } => {
                let stream = VimDocStream::new(&entities, cx);
                let mut current = start;
                for _ in 0..repeat.max(1) {
                    let Some(next) = stream.word_end_backward(current, big) else {
                        break;
                    };
                    current = next;
                }
                self.vim_set_cursor(&entities, current, cx);
            }
            Motion::MatchPair => {
                let stream = VimDocStream::new(&entities, cx);
                let Some(target) = stream.match_pair(start) else {
                    return;
                };
                self.vim_push_jumplist(entities.get(start.block_index).map(|entity| entity.entity_id()), start.offset, cx);
                self.vim_set_cursor(&entities, target, cx);
            }
            Motion::ParagraphForward | Motion::ParagraphBackward => {
                let forward = motion == Motion::ParagraphForward;
                let Some(target) = self.vim_paragraph_landing(&entities, start, forward, cx)
                else {
                    return;
                };
                self.vim_set_cursor(&entities, target, cx);
            }
            Motion::SentenceForward | Motion::SentenceBackward => {
                let forward = motion == Motion::SentenceForward;
                let stream = VimDocStream::new(&entities, cx);
                let mut current = start;
                for _ in 0..repeat.max(1) {
                    let Some(next) = sentence_step(&stream, current, forward) else {
                        break;
                    };
                    current = next;
                }
                self.vim_set_cursor(&entities, current, cx);
            }
            Motion::FindChar { ch, reverse, till } => {
                let stream = VimDocStream::new(&entities, cx);
                let mut current = start;
                for _ in 0..repeat.max(1) {
                    let next = if reverse {
                        stream.find_char_backward(current, ch, till)
                    } else {
                        stream.find_char_forward(current, ch, till)
                    };
                    let Some(next) = next else {
                        break;
                    };
                    current = next;
                }
                self.vim.last_find = Some(FindChar { ch, reverse, till });
                self.vim_set_cursor(&entities, current, cx);
            }
            Motion::ForwardChars | Motion::BackwardChars => {
                let forward = motion == Motion::ForwardChars;
                let stream = VimDocStream::new(&entities, cx);
                let mut current = start;
                for _ in 0..repeat.max(1) {
                    let next = if forward {
                        stream.next_grapheme(current)
                    } else {
                        stream.prev_grapheme(current)
                    };
                    let Some(next) = next else {
                        break;
                    };
                    current = next;
                }
                self.vim_set_cursor(&entities, current, cx);
            }
            Motion::HardLineStart | Motion::FirstNonBlank | Motion::HardLineEnd
            | Motion::LastNonBlank | Motion::Left | Motion::Right | Motion::ColumnNo(_) => {
                self.vim_line_motion(motion, &entities, start, repeat, cx);
            }
        }
        let _ = window;
    }

    /// 行内动作（h/l/0/^/$/g_/|）：以硬行为界，不跨块。
    fn vim_line_motion(
        &mut self,
        motion: Motion,
        entities: &[gpui::Entity<Block>],
        start: VimPosition,
        repeat: u32,
        cx: &mut Context<Self>,
    ) {
        let stream = VimDocStream::new(entities, cx);
        let line = stream.hard_line_range(start.block_index, start.offset);
        let text = stream.text_for_line(start.block_index);
        let target = match motion {
            Motion::Left => {
                let mut offset = start.offset;
                for _ in 0..repeat.max(1) {
                    if offset <= line.start {
                        break;
                    }
                    offset = prev_grapheme_start(text, offset).unwrap_or(line.start);
                }
                offset
            }
            Motion::Right => {
                let mut offset = start.offset;
                for _ in 0..repeat.max(1) {
                    let Some(next) = next_grapheme_start(text, offset) else {
                        break;
                    };
                    if next >= line.end {
                        break;
                    }
                    offset = next;
                }
                offset
            }
            Motion::HardLineStart => line.start,
            Motion::FirstNonBlank => {
                let mut offset = line.start;
                while offset < line.end {
                    let Some(next) = next_grapheme_start(text, offset) else {
                        break;
                    };
                    if !text[offset..next].chars().all(|ch| ch.is_whitespace()) {
                        break;
                    }
                    offset = next;
                }
                offset
            }
            Motion::HardLineEnd => {
                // vim 的 $ 落在最后一字素上；空行停行首。
                if line.end > line.start {
                    prev_grapheme_start(text, line.end).unwrap_or(line.start)
                } else {
                    line.start
                }
            }
            Motion::LastNonBlank => {
                let mut offset = if line.end > line.start {
                    prev_grapheme_start(text, line.end).unwrap_or(line.start)
                } else {
                    line.start
                };
                while offset > line.start {
                    let Some(prev) = prev_grapheme_start(text, offset) else {
                        break;
                    };
                    if !text[prev..offset].chars().all(|ch| ch.is_whitespace()) {
                        break;
                    }
                    offset = prev;
                }
                offset
            }
            Motion::ColumnNo(column) => {
                let mut offset = line.start;
                let mut index = 1u32;
                while index < column {
                    let Some(next) = next_grapheme_start(text, offset) else {
                        break;
                    };
                    if next >= line.end {
                        break;
                    }
                    offset = next;
                    index += 1;
                }
                offset
            }
            _ => start.offset,
        };
        self.vim_set_cursor(
            entities,
            VimPosition { block_index: start.block_index, offset: target },
            cx,
        );
    }

    /// 上/下一步：块内走视觉行（换行感知），块界走邻块进位。块内移动直接
    /// 复用 `move_cursor_vertically`（换行判据与方向键同源），移动成功读回
    /// 新位置；失败说明到块界，按保存的横坐标进邻块。
    pub(super) fn vim_vertical_step(
        &mut self,
        entities: &[gpui::Entity<Block>],
        current: VimPosition,
        down: bool,
        cx: &mut Context<Self>,
    ) -> Option<VimPosition> {
        let entity = entities.get(current.block_index)?.clone();
        let is_code_block = entity.read(cx).kind().is_code_block();
        let moved = if is_code_block {
            false
        } else {
            entity.update(cx, |block, block_cx| {
                let preferred_x = block.vertical_anchor_x();
                block.move_cursor_vertically(if down { 1 } else { -1 }, preferred_x, block_cx)
            })
        };
        if moved {
            let offset = entity.read(cx).cursor_offset();
            return Some(VimPosition { block_index: current.block_index, offset });
        }
        // 代码块与没有文本布局的块按硬行移动：代码块的布局行距会让一次 j
        // 跨过多个源码行。
        let text = entity.read(cx).display_text().to_string();
        let line = VimDocStream::new(entities, cx)
            .hard_line_range(current.block_index, current.offset);
        if down {
            if line.end < text.len() {
                return Some(VimPosition {
                    block_index: current.block_index,
                    offset: line.end + 1,
                });
            }
        } else if line.start > 0 {
            let offset = text
                .get(..line.start - 1)
                .and_then(|before| before.rfind('\n'))
                .map_or(0, |index| index + 1);
            return Some(VimPosition {
                block_index: current.block_index,
                offset,
            });
        }
        let next_index = current.block_index.checked_add_signed(if down { 1 } else { -1 })?;
        if next_index >= entities.len() {
            return None;
        }
        let preferred_x = entity.read(cx).vertical_anchor_x();
        let offset = entities[next_index]
            .read(cx)
            .entry_offset_for_vertical_focus(!down, Some(preferred_x));
        Some(VimPosition { block_index: next_index, offset })
    }

    /// gg/G/{n}G 的落点：目标块 + 块内硬行行首的非空白。
    pub(super) fn vim_document_line_landing(
        &self,
        motion: Motion,
        entities: &[gpui::Entity<Block>],
        cx: &App,
    ) -> Option<VimPosition> {
        let target_block = match motion {
            Motion::DocStart => 0,
            Motion::DocEnd => entities.len().checked_sub(1)?,
            Motion::GoToLine(line) => {
                self.vim_block_for_buffer_line(u64::from(line.max(1)), entities)?
            }
            _ => return None,
        };
        let entity = entities.get(target_block)?;
        let block = entity.read(cx);
        let text = block.display_text();
        let offset = text
            .char_indices()
            .find(|(_, ch)| !ch.is_whitespace())
            .map(|(index, _)| index)
            .unwrap_or(0);
        Some(VimPosition { block_index: target_block, offset })
    }

    /// 缓冲行号 → 导航块下标（块内软换行不拆行：近似映射，源码/代码视图精确）。
    fn vim_block_for_buffer_line(&self, line: u64, entities: &[gpui::Entity<Block>]) -> Option<usize> {
        let line = (line.max(1) as usize).saturating_sub(1);
        if line >= self.buffer.line_count() {
            return entities.len().checked_sub(1);
        }
        let offset = self.buffer.line_start(line);
        for (index, entity) in entities.iter().enumerate() {
            if let Some(span) = self.document.source_span_of(entity.entity_id()) {
                if span.start <= offset && offset < span.end {
                    return Some(index);
                }
            }
        }
        None
    }

    /// { / }：段落边界 = 空块（本编辑器的段落即块）。
    pub(super) fn vim_paragraph_landing(
        &self,
        entities: &[gpui::Entity<Block>],
        start: VimPosition,
        forward: bool,
        cx: &App,
    ) -> Option<VimPosition> {
        let is_empty = |index: usize| {
            entities
                .get(index)
                .map(|entity| entity.read(cx).display_text().is_empty())
                .unwrap_or(true)
        };
        if forward {
            let mut index = start.block_index + 1;
            while index < entities.len() {
                if is_empty(index) {
                    return Some(VimPosition { block_index: index, offset: 0 });
                }
                index += 1;
            }
            entities.len().checked_sub(1).map(|index| VimPosition { block_index: index, offset: 0 })
        } else {
            let mut index = start.block_index;
            while index > 0 {
                index -= 1;
                if is_empty(index) {
                    return Some(VimPosition { block_index: index, offset: 0 });
                }
            }
            Some(VimPosition { block_index: 0, offset: 0 })
        }
    }

    /// H/M/L：视口顶/中/底。行级精确几何在行计划里，这里以导航块的可见
    /// 顺序近似（首/末/中），聚焦后由 `ensure_focused_caret_visible` 稳住。
    fn vim_screen_line(
        &self,
        which: ScreenCursorKind,
        entities: &[gpui::Entity<Block>],
        repeat: u32,
        cx: &App,
    ) -> Option<VimPosition> {
        let last = entities.len().checked_sub(1)?;
        // 基准是**视口内可见块**：已挂载块的 last_bounds 与滚动视口求交；
        // 完全没有几何（首帧前）时退回整篇近似，避免 H/M/L 直接失效。
        let viewport = self.scroll_handle.bounds();
        let mut visible: Vec<usize> = entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| {
                entity
                    .read(cx)
                    .last_bounds
                    .filter(|bounds| {
                        bounds.bottom() > viewport.top() && bounds.top() < viewport.bottom()
                    })
                    .map(|_| index)
            })
            .collect();
        if visible.is_empty() {
            visible = (0..=last).collect();
        }
        let count = repeat.max(1) as usize;
        let index = match which {
            ScreenCursorKind::Top => visible[count.saturating_sub(1).min(visible.len() - 1)],
            ScreenCursorKind::Middle => visible[visible.len() / 2],
            ScreenCursorKind::Bottom => {
                visible[visible.len().saturating_sub(count).min(visible.len() - 1)]
            }
        };
        let entity = entities.get(index)?;
        let block = entity.read(cx);
        let text = block.display_text();
        let offset = text
            .char_indices()
            .find(|(_, ch)| !ch.is_whitespace())
            .map(|(index, _)| index)
            .unwrap_or(0);
        Some(VimPosition { block_index: index, offset })
    }

    /// 跳转表压栈（gg/G/%/标记/搜索前调用）。
    /// 跳转前把当前落点压进跳转表。`/`、`?`、`n`、`N`、`*`、`#` 与标记跳转
    /// 共用这一处，不在各调用方重抄一遍取位置的代码。
    pub(super) fn vim_push_jump_before_leaving(&mut self, cx: &mut Context<Self>) {
        let entities = self.vim_navigation_entities(cx);
        let Some(current) = self.vim_current_position(&entities, cx) else {
            return;
        };
        let entity_id = entities
            .get(current.block_index)
            .map(|entity| entity.entity_id());
        self.vim_push_jumplist(entity_id, current.offset, cx);
    }

    pub(super) fn vim_push_jumplist(        &mut self,
        entity_id: Option<gpui::EntityId>,
        offset: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(entity_id) = entity_id else {
            return;
        };
        let identity = self.buffer.identity();
        let entry = (identity, entity_id, offset);
        // 截掉当前下标之后的「未来」，再压栈。
        if self.vim.jump_index < self.vim.jumplist.len() {
            self.vim.jumplist.truncate(self.vim.jump_index);
        }
        if self.vim.jumplist.last() != Some(&entry) {
            self.vim.jumplist.push(entry);
            if self.vim.jumplist.len() > 100 {
                self.vim.jumplist.remove(0);
            }
        }
        self.vim.jump_index = self.vim.jumplist.len();
        let _ = cx;
    }
}

/// 句子步进（( / )）：句子结束 = [.!?] 后跟空白/边界；流式扫描。
pub(super) fn sentence_step(
    stream: &VimDocStream<'_>,
    position: VimPosition,
    forward: bool,
) -> Option<VimPosition> {
    let is_terminator = |position: VimPosition| {
        matches!(
            stream
                .grapheme_at(position)
                .and_then(|grapheme| grapheme.chars().next()),
            Some('.' | '!' | '?')
        )
    };
    let mut current = position;
    if forward {
        // 跳过当前残句到终结符，再越过空白落到下一句首。
        loop {
            let grapheme = stream.grapheme_at(current)?;
            let ch = grapheme.chars().next()?;
            if matches!(ch, '.' | '!' | '?') {
                current = stream.next_grapheme(current)?;
                // 连续终结符（`!?`、`…`）要一起越过，`)` 才不停在第二个符号上。
                while is_terminator(current) {
                    current = stream.next_grapheme(current)?;
                }
                break;
            }
            current = stream.next_grapheme(current)?;
        }
        while stream
            .grapheme_at(current)
            .and_then(|grapheme| grapheme.chars().next())
            .is_some_and(|ch| ch.is_whitespace())
        {
            current = stream.next_grapheme(current)?;
        }
        Some(current)
    } else {
        // 退过空白与前一句的句尾标点，落到前一句首。
        let mut previous = stream.prev_grapheme(position)?;
        while stream
            .grapheme_at(previous)
            .and_then(|grapheme| grapheme.chars().next())
            .is_some_and(|ch| ch.is_whitespace())
        {
            let Some(before) = stream.prev_grapheme(previous) else {
                return Some(previous);
            };
            previous = before;
        }
        // 现在在前一句句尾；继续退到句首（越过整句的非空白与非终结符）。
        loop {
            let ch = stream.grapheme_at(previous)?.chars().next()?;
            if matches!(ch, '.' | '!' | '?') {
                // 句尾找到了：还要越过连续终结符与其后的空白，`(` 才落在句首的
                // 第一个非空白上，而不是停在标点后的那个空格。
                let mut landing = stream.next_grapheme(previous)?;
                while is_terminator(landing) {
                    landing = stream.next_grapheme(landing)?;
                }
                while stream
                    .grapheme_at(landing)
                    .and_then(|grapheme| grapheme.chars().next())
                    .is_some_and(|ch| ch.is_whitespace())
                {
                    let Some(next) = stream.next_grapheme(landing) else {
                        break;
                    };
                    landing = next;
                }
                return Some(landing);
            }
            let Some(before) = stream.prev_grapheme(previous) else {
                return Some(previous);
            };
            previous = before;
        }
    }
}

impl VimDocStream<'_> {
    /// 硬行文本（hard_line_range 的伴生读取）。
    pub(super) fn text_for_line(&self, block_index: usize) -> &str {
        self.text(block_index)
    }
}

/// `vim_move` 的模块门面（vim.rs 的执行器调用）。
pub(super) fn vim_move(
    editor: &mut Editor,
    motion: Motion,
    repeat: u32,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_move_by(motion, repeat, window, cx);
}

/// `;` / `,`：重放上一次字符查找。
pub(super) fn vim_repeat_find(
    editor: &mut Editor,
    reverse: bool,
    count: u32,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let Some(find) = editor.vim.last_find else {
        return;
    };
    let motion = Motion::FindChar {
        ch: find.ch,
        reverse: find.reverse != reverse,
        till: find.till,
    };
    vim_move(editor, motion, count, window, cx);
}

/// 滚动（ctrl-d/u/f/b/e/y）。滚动偏移向下为负，与 `scroll_handle.offset()` 一致。
pub(super) fn vim_scroll(
    editor: &mut Editor,
    kind: ScrollKind,
    count: u32,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let repeat = count.max(1) as f32;
    let viewport_height = editor.scroll_handle.bounds().size.height;
    let delta = match kind {
        ScrollKind::HalfPageDown => -(viewport_height / 2.0 * repeat),
        ScrollKind::HalfPageUp => viewport_height / 2.0 * repeat,
        ScrollKind::LineDown => gpui::px(-repeat),
        ScrollKind::LineUp => gpui::px(repeat),
        ScrollKind::PageDown => -(viewport_height * 0.9 * repeat),
        ScrollKind::PageUp => viewport_height * 0.9 * repeat,
    };
    let _ = window;
    editor.scroll_viewport_by(delta, cx);
}

/// zz / zt / zb：光标所在块对齐视口中部、顶部、底部。
pub(super) fn vim_viewport_align(
    editor: &mut Editor,
    align: ViewportAlign,
    cx: &mut Context<Editor>,
) {
    let Some(entity_id) = editor.active_entity_id else {
        return;
    };
    editor.focus_block(entity_id);
    editor.pending_scroll_active_block_into_view = true;
    editor.pending_scroll_placement = match align {
        ViewportAlign::Top => ScrollPlacement::Top,
        ViewportAlign::Bottom => ScrollPlacement::Bottom,
        ViewportAlign::Center => ScrollPlacement::Center,
    };
    cx.notify();
}
