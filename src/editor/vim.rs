//! vim 模式核心：按键 token、模式状态机、序列解析与分发。
//!
//! 分层：本文件负责「把按键流变成命令」；动作解析在 `motion.rs`，操作符与
//! 寄存器在 `operate.rs`，可视模式在 `visual.rs`，`:` 命令与 `/` 搜索在
//! `command.rs`。位置统一用 `VimPosition`（可见块序号 + 块内显示偏移）；只有
//! 行级操作（dd/yy/:s/标记）升到缓冲区字节偏移，落地走
//! `apply_selection_snapshot_in_current_mode`。
//!
//! 性能约定：`vim_enabled == false` 时 `vim_key_down` 第一步就返回；开启后
//! 每键只有一次序列匹配与局部文本扫描，不做整篇序列化或整棵重排。

pub(crate) mod command;
pub(crate) mod motion;
pub(crate) mod operate;
pub(crate) mod visual;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use gpui::{App, Context, Keystroke, Task, Window};

use crate::components::Block;
use crate::config::preferences::{VimKeymapAction, VimKeymapMode};
use crate::editor::Editor;

use motion::{Motion, VimPosition};
use operate::{Operator, TextObject};

/// vim 模式集合。`Insert` 即普通编辑器语义（按键直通 IME）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VimMode {
    Normal,
    Insert,
    Replace,
    Visual(VisualKind),
}

impl VimMode {
    /// 块状光标：普通/可视/替换画块，插入画竖条。
    fn uses_block_caret(self) -> bool {
        !matches!(self, VimMode::Insert)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VisualKind {
    Char,
    Line,
    Block,
}

/// surround 的定界符对：open/close 是实际插入的文本（开括号按 vim-surround
/// 惯例带内空格），open_char/close_char 用于查找已有配对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SurroundPair {
    pub open: &'static str,
    pub close: &'static str,
    pub open_char: char,
    pub close_char: char,
}

impl SurroundPair {
    fn from_delimiter(ch: char) -> Option<Self> {
        Some(match ch {
            '"' => Self { open: "\"", close: "\"", open_char: '"', close_char: '"' },
            '\'' => Self { open: "'", close: "'", open_char: '\'', close_char: '\'' },
            '`' => Self { open: "`", close: "`", open_char: '`', close_char: '`' },
            '(' => Self { open: "( ", close: " )", open_char: '(', close_char: ')' },
            ')' | 'b' => Self { open: "(", close: ")", open_char: '(', close_char: ')' },
            '[' => Self { open: "[ ", close: " ]", open_char: '[', close_char: ']' },
            ']' => Self { open: "[", close: "]", open_char: '[', close_char: ']' },
            '{' => Self { open: "{ ", close: " }", open_char: '{', close_char: '}' },
            '}' | 'B' => Self { open: "{", close: "}", open_char: '{', close_char: '}' },
            '<' => Self { open: "< ", close: " >", open_char: '<', close_char: '>' },
            '>' => Self { open: "<", close: ">", open_char: '<', close_char: '>' },
            _ => return None,
        })
    }
}

/// `J`/`gJ` 要并的行数：无计数并相邻两行，`3J` 并三行（两次并入）。
fn vim_join_line_count(count: u32) -> u32 {
    if count > 0 { count } else { 2 }
}

/// 拦截层里的最小按键表示。带 cmd/alt/function 修饰的组合一开始就放行，
/// 不进本类型；ctrl 只认无 shift 的单字母。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VimKey {
    Char(char),
    Ctrl(char),
    Esc,
    Enter,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

impl VimKey {
    pub(super) fn from_keystroke(keystroke: &Keystroke) -> Option<Self> {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.alt || modifiers.function {
            return None;
        }
        let key = keystroke.key.as_str();
        if modifiers.control {
            if modifiers.shift {
                return None;
            }
            // Ctrl-[ 就是终端里的 Esc（0x1B）；不在这里认，下面的 Ctrl('[')
            // 分支全是死代码。
            if key == "[" {
                return Some(VimKey::Esc);
            }
            let ch = key.chars().next()?;
            if ch.is_ascii_lowercase() {
                return Some(VimKey::Ctrl(ch));
            }
            return None;
        }
        // 字母与标点信 key_char（带大小写，与键盘布局一致）；功能键信 key。
        // 本快照里 key_char 是 Option<String>，只收单字符。
        if let Some(text) = keystroke.key_char.as_deref() {
            let mut characters = text.chars();
            if let (Some(ch), None) = (characters.next(), characters.next()) {
                if !ch.is_control() && (ch.is_ascii_graphic() || ch == ' ') {
                    return Some(VimKey::Char(ch));
                }
            }
        }
        match key {
            "escape" => Some(VimKey::Esc),
            "enter" => Some(VimKey::Enter),
            "tab" => Some(VimKey::Tab),
            "backspace" => Some(VimKey::Backspace),
            "delete" => Some(VimKey::Delete),
            "left" => Some(VimKey::Left),
            "right" => Some(VimKey::Right),
            "up" => Some(VimKey::Up),
            "down" => Some(VimKey::Down),
            "home" => Some(VimKey::Home),
            "end" => Some(VimKey::End),
            "pageup" => Some(VimKey::PageUp),
            "pagedown" => Some(VimKey::PageDown),
            // 部分布局里标点无 key_char：退化用键名。
            other if other.len() == 1 && other.chars().all(|c| c.is_ascii_graphic()) => {
                Some(VimKey::Char(other.chars().next()?))
            }
            _ => None,
        }
    }

    fn as_char(self) -> Option<char> {
        match self {
            VimKey::Char(ch) => Some(ch),
            VimKey::Ctrl(ch) => Some(ch),
            VimKey::Esc => Some('\x1b'),
            VimKey::Enter => Some('\n'),
            VimKey::Tab => Some('\t'),
            VimKey::Backspace => Some('\x7f'),
            _ => None,
        }
    }

    fn as_register_name(self) -> Option<char> {
        match self.as_char()? {
            ch if ch.is_ascii_alphanumeric() => Some(ch),
            ch if matches!(ch, '+' | '-' | '_' | '"' | ':' | '.' | '%') => Some(ch),
            _ => None,
        }
    }
}

/// 单次 f/F/t/T 的完整参数，`;` 与 `,` 重放用。
#[derive(Clone, Copy, Debug)]
pub(super) struct FindChar {
    pub ch: char,
    pub reverse: bool,
    pub till: bool,
}

/// 可视操作的重放形状：字素步数（Char）或硬行数减一（Line）。
#[derive(Clone, Copy, Debug)]
pub(super) struct VisualRepeat {
    pub kind: VisualKind,
    pub forward: bool,
    pub distance: usize,
}

/// `.` 重放的一条变更记录：进入插入前的命令键 + 插入会话里敲过的键。
#[derive(Clone, Debug, Default)]
pub(super) struct DotRecord {
    pub entry_keys: Vec<VimKey>,
    pub insert_keys: Vec<VimKey>,
    /// 可视操作按选区形状重放；普通操作为空。
    pub visual: Option<VisualRepeat>,
}

#[derive(Clone, Debug)]
pub(super) struct VimSearchState {
    pub query: String,
    pub reverse: bool,
    pub whole_word: bool,
}

/// vim 会话状态，挂在 `Editor` 实体上（分屏各自一份会话）。
pub(crate) struct VimState {
    pub(super) mode: VimMode,
    /// 普通/可视模式下未归约的按键序列。
    pub(super) pending: Vec<VimKey>,
    pub(super) visual_anchor: Option<VimPosition>,
    /// 可视模式的焦点（选区排他端点之前的那一格）。块光标的 `cursor_offset`
    /// 是选区焦点，可视渲染会把它写成排他端点；两层口径分开存。
    pub(super) visual_focus: Option<VimPosition>,
    /// 块状可视的锚点列（硬行内字素序号，口径见 visual.rs）。
    pub(super) visual_block_anchor_column: Option<usize>,
    pub(super) last_visual: Option<(VimPosition, VimPosition, VisualKind)>,
    pub(super) registers: operate::VimRegisters,
    /// 宏寄存器（与文本寄存器分开存：q 录的是键序列不是文本）。
    pub(super) macro_registers: HashMap<char, Vec<VimKey>>,
    /// 标记：缓冲区身份号 → (标记名 → (块实体号, 显示偏移))。
    pub(super) marks: HashMap<u64, HashMap<char, (gpui::EntityId, usize)>>,
    pub(super) jumplist: Vec<(u64, gpui::EntityId, usize)>,
    pub(super) jump_index: usize,
    pub(super) last_find: Option<FindChar>,
    pub(super) last_search: Option<VimSearchState>,
    pub(super) hlsearch: bool,
    pub(super) recording: Option<(char, Vec<VimKey>)>,
    pub(super) last_macro_register: Option<char>,
    pub(super) dot: Option<DotRecord>,
    /// 等待映射补全的缓冲键（insert 模式为字符；normal/visual 模式复用本字段）。
    pub(super) mapping_pending: Vec<VimKey>,
    pub(super) mapping_epoch: u64,
    pub(super) mapping_timeout_task: Option<Task<()>>,
    /// 本插入会话已敲过的键（`.` 重放素材；IME 组合输入不在其中，已知限制）。
    pub(super) insert_typed: Vec<VimKey>,
    /// 上一次插入会话实际落下的文本（插入模式 Ctrl-a 重插）。
    pub(super) last_insert_text: String,
    /// 插入模式 Ctrl-v 之后等待原样插入的下一键。
    pub(super) insert_literal_pending: bool,
    /// 可视操作进入的插入会话，Esc 时把选区形状并进 `.` 素材。
    pub(super) insert_visual_repeat: Option<VisualRepeat>,
    /// 触发本插入会话的命令键（`3o` 的重放素材）。
    pub(super) insert_entry_keys: Vec<VimKey>,
    pub(super) insert_repeat_count: u32,
    /// ctrl-o 单次普通命令结束后回插入模式。
    pub(super) insert_one_shot_normal: bool,
    /// insert 里 ctrl-r 等待寄存器名。
    pub(super) insert_awaiting_register: bool,
    /// 上次离开插入时的位置（gi 用：(缓冲区身份号, 块实体号, 显示偏移)）。
    pub(super) last_insert_position: Option<(u64, gpui::EntityId, usize)>,
    /// R 模式会话的原字符栈（退格还原用）。
    pub(super) replace_originals: Vec<(usize, String)>,
    /// 块状可视 I/A 的多行插入点（会话结束时复制到各行）。
    pub(super) block_visual_insert: Option<Vec<(gpui::EntityId, std::ops::Range<usize>)>>,
    /// `:` / `/` 命令行浮层。
    pub(super) cmdline: Option<command::VimCommandLine>,
    /// `:` / `/` 命令行浮层的 pending 标记（render 期落地，见 command.rs）。
    pub(super) pending_cmdline: Option<command::CmdlineKind>,
    /// 宏/`.` 重放进行中（防自递归与重复记录）。
    pub(super) replaying: bool,
    /// 正在重放的宏寄存器，按调用顺序入栈；同名再入即 Vim 的 E223。
    pub(super) macro_stack: Vec<char>,
    /// `:` 与 `/` 的历史（Vim 也分开存）：命令行里 ↑/↓ 回溯。
    pub(super) command_history: Vec<String>,
    pub(super) search_history: Vec<String>,
    /// 正在回溯到第几条；None 表示还没开始翻。
    pub(super) history_index: Option<usize>,
}

impl Default for VimState {
    fn default() -> Self {
        Self {
            mode: VimMode::Insert,
            pending: Vec::new(),
            visual_anchor: None,
            visual_focus: None,
            visual_block_anchor_column: None,
            last_visual: None,
            registers: operate::VimRegisters::default(),
            macro_registers: HashMap::new(),
            marks: HashMap::new(),
            jumplist: Vec::new(),
            jump_index: 0,
            last_find: None,
            last_search: None,
            hlsearch: true,
            recording: None,
            last_macro_register: None,
            dot: None,
            mapping_pending: Vec::new(),
            mapping_epoch: 0,
            mapping_timeout_task: None,
            insert_typed: Vec::new(),
            last_insert_text: String::new(),
            insert_literal_pending: false,
            insert_visual_repeat: None,
            insert_entry_keys: Vec::new(),
            insert_repeat_count: 0,
            insert_one_shot_normal: false,
            insert_awaiting_register: false,
            last_insert_position: None,
            replace_originals: Vec::new(),
            block_visual_insert: None,
            cmdline: None,
            pending_cmdline: None,
            replaying: false,
            macro_stack: Vec::new(),
            command_history: Vec::new(),
            search_history: Vec::new(),
            history_index: None,
        }
    }
}

/// 解析完成的一条命令。
#[derive(Clone, Debug)]
pub(super) struct VimCommand {
    /// 0 表示未给计数（执行层按 1 处理）。
    pub count: u32,
    pub register: Option<char>,
    pub action: VimAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VimAction {
    Move(Motion),
    /// 操作符 + 目标。`CurrentLines` 是 dd/yy/cc 的行版，`Selection` 是可视
    /// 选区版（行/块语义由当前可视种类决定）。
    Operate(Operator, OperatorTarget),
    /// 可视模式里把选区扩到文本对象。
    VisualObject(TextObject),    /// 可视模式 r{char}：整段选区换成同一字符。
    VisualReplaceChar(char),
    /// ys{motion}{delim}：给目标区间加环绕定界符。
    SurroundAdd { target: OperatorTarget, pair: SurroundPair },
    /// ds{delim}：删除光标所在配对。
    SurroundDelete(SurroundPair),
    /// cs{old}{new}：替换光标所在配对的定界符。
    SurroundChange { old: SurroundPair, new: SurroundPair },
    /// 可视模式 S{delim}：给选区加环绕定界符。
    VisualSurround(SurroundPair),
    /// gn/gN：选中下一/上一处搜索命中。
    SelectSearchMatch { reverse: bool },
    EnterInsert(InsertPlacement),
    EnterReplaceMode,
    ReplaceChar(char),
    /// `~`：交换光标处 count 个字符的大小写并右移。
    SwapCaseAtCursor,
    /// ctrl-a / ctrl-x：光标处（或其后最近的）十进制数加/减 count。
    Increment(i64),
    Paste { after: bool },
    JoinLines { count: u32, keep_space: bool },
    Undo,
    Redo,
    Dot,
    RepeatFind { reverse: bool },
    SetMark(char),
    GotoMark { mark: char, linewise: bool },
    ToggleRecording(char),
    StopRecording,
    ReplayMacro(char),
    ReplayLastMacro,
    Scroll(ScrollKind),
    /// zz/z./z<CR> 居中，zt/zb 顶/底。
    ViewportAlign(ViewportAlign),
    Search { reverse: bool },
    SearchWord { reverse: bool, whole_word: bool },
    SearchNext { reverse: bool },
    OpenCmdline(command::CmdlineKind),
    ToggleVisual(VisualKind),
    /// v 在字符可视里再按 v、V 在行可视里再按 V：退出。
    ExitVisual,
    ExchangeVisualAnchor,
    SwapVisualBlockColumn,
    ReselectVisual,
    JumplistBack,
    JumplistForward,
    EnterNormalMode,
    /// ZZ：保存并关标签；ZQ：不保存关标签。
    SaveAndClose,
    CloseWithoutSaving,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OperatorTarget {
    Motion(Motion, u32),
    TextObject(TextObject),
    CurrentLines,
    Selection,
    /// 下一/上一处搜索命中（gn/gN 作为操作符目标）。
    SearchMatch { reverse: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InsertPlacement {
    AtCursor,
    AfterCursor,
    LineStart,
    LineEnd,
    Below,
    Above,
    /// gI：硬行第一列。
    FirstColumn,
    /// gi：上次离开插入的位置。
    LastInsertPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollKind {
    HalfPageDown,
    HalfPageUp,
    LineDown,
    LineUp,
    PageDown,
    PageUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScreenCursorKind {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewportAlign {
    Center,
    Top,
    Bottom,
}

#[derive(Clone, Debug)]
pub(super) enum ParseOutcome {
    NeedMore,
    Invalid,
    Execute(VimCommand),
}

enum MappingMatch {
    Fire(VimKeymapAction),
    Pending,
    None,
}

impl Editor {
    /// vim 总入口（render 里的按键拦截器调用）。返回 true 表示吞掉该键。
    pub(super) fn vim_key_down(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.vim_enabled {
            return false;
        }
        if self.vim.cmdline.is_some() {
            // 命令行浮层：Enter 提交、Esc 关闭（拦截层先于输入框拿到键）。
            return command::vim_cmdline_key_down(self, keystroke, window, cx);
        }
        if keystroke.modifiers.platform || keystroke.modifiers.alt {
            return false;
        }
        let Some(block) = self.vim_focused_block(window, cx) else {
            return false;
        };
        let focused_block = block.read(cx);
        let language_focused = focused_block.code_language_focus_handle.is_focused(window)
            || focused_block
                .code_language_focus_handle
                .contains_focused(window, cx);
        if language_focused {
            return false;
        }
        let Some(key) = VimKey::from_keystroke(keystroke) else {
            return false;
        };
        self.vim_route_key(key, window, cx)
    }

    /// 按当前模式分发一个 vim 键（宏与 `.` 重放共用同一入口）。
    fn vim_route_key(&mut self, key: VimKey, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.vim.mode {
            VimMode::Insert => self.vim_insert_key(key, window, cx),
            VimMode::Replace => self.vim_replace_key(key, window, cx),
            VimMode::Normal | VimMode::Visual(_) => self.vim_modal_key(key, window, cx),
        }
    }

    /// 焦点是否落在本文档的可编辑块上（含表格单元格与代码语言框）。
    pub(super) fn vim_focused_block(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<gpui::Entity<Block>> {
        let is_focused = |block: &gpui::Entity<Block>| {
            let block = block.read(cx);
            block.focus_handle.is_focused(window)
                || block.focus_handle.contains_focused(window, cx)
                || block.code_language_focus_handle.is_focused(window)
                || block.code_language_focus_handle.contains_focused(window, cx)
        };

        if let Some(block) = self
            .active_entity_id
            .and_then(|entity_id| self.focusable_entity_by_id(entity_id))
            .filter(|block| is_focused(block))
        {
            return Some(block);
        }
        for binding in self.table_cells.values() {
            if is_focused(&binding.cell) {
                return Some(binding.cell.clone());
            }
        }
        self.document
            .visible_blocks()
            .iter()
            .find_map(|visible| is_focused(&visible.entity).then(|| visible.entity.clone()))
    }

    // ---- 模式切换 ----

    pub(super) fn vim_enter_mode(&mut self, mode: VimMode, cx: &mut Context<Self>) {
        self.vim.mode = mode;
        self.vim.pending.clear();
        self.vim.mapping_pending.clear();
        self.vim.insert_one_shot_normal = false;
        self.sync_vim_block_carets(cx);
        cx.notify();
    }

    /// 把当前模式的光标形状（块/竖条）与文本输入开关广播到可见块与表格单元格。
    ///
    /// 单元格是独立实体，不在 `visible_blocks` 里；漏掉它们，普通模式的
    /// 可打印字符会经系统文本输入直接落进格子（绕过 vim 按键拦截）。
    /// 当前模式要求的块内光标形状与系统文本输入开关。
    ///
    /// 广播（`sync_vim_block_carets`）与新建表格单元格共用这一处判据：
    /// 单元格实体在文档载入后才由运行时重建，默认值是可接受输入，必须
    /// 在创建时套用当前模式，不能指望之后某次广播补上。
    pub(super) fn vim_caret_state(&self) -> (bool, bool, bool) {
        (
            self.vim_enabled && self.vim.mode.uses_block_caret(),
            !self.vim_enabled || self.vim.mode == VimMode::Insert,
            self.vim_enabled
                && matches!(self.vim.mode, VimMode::Normal | VimMode::Replace),
        )
    }

    pub(super) fn sync_vim_block_carets(&mut self, cx: &mut Context<Self>) {
        let (block_caret, accepts_text, mouse_moves_caret) = self.vim_caret_state();
        let visible = self.document.visible_blocks().to_vec();
        let cells = self
            .table_cells
            .values()
            .map(|binding| binding.cell.clone())
            .collect::<Vec<_>>();
        let entities = visible
            .iter()
            .map(|entry| &entry.entity)
            .chain(cells.iter());
        for entity in entities {
            let (caret, accepts, mouse) = {
                let block = entity.read(cx);
                (
                    block.vim_block_caret,
                    block.vim_accepts_text_input,
                    block.vim_mouse_moves_caret,
                )
            };
            if caret != block_caret || accepts != accepts_text || mouse != mouse_moves_caret {
                entity.update(cx, |block, cx| {
                    block.vim_block_caret = block_caret;
                    block.vim_accepts_text_input = accepts_text;
                    block.vim_mouse_moves_caret = mouse_moves_caret;
                    cx.notify();
                });
            }
        }
    }

    /// 开关 vim 模式（偏好或命令触发）。关闭时回到普通编辑器语义。
    pub(crate) fn vim_set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.vim_enabled == enabled {
            return;
        }
        self.vim_enabled = enabled;
        if enabled {
            self.vim_enter_mode(VimMode::Normal, cx);
        } else {
            self.vim.mapping_pending.clear();
            self.vim.pending.clear();
            if self.vim.cmdline.take().is_some() {
                self.restore_focus_after_overlay(cx);
            }
            self.vim_enter_mode(VimMode::Insert, cx);
        }
    }

    /// 命令面板/菜单入口：切换 vim 模式并落盘（dispatch.rs 路由到这里）。
    pub(crate) fn toggle_vim_mode(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let enable = !crate::config::EditorSettings::vim(cx).enable;
        crate::config::EditorSettings::set_vim_enabled(cx, enable);
        // 本编辑器经 observe_global 回调同步；立刻刷一次兜底。
        self.vim_set_enabled(enable, cx);
    }

    // ---- insert 模式 ----

    fn vim_insert_key(
        &mut self,
        key: VimKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.vim.insert_literal_pending {
            self.vim.insert_literal_pending = false;
            if !self.vim.replaying {
                self.vim_record_insert_typed(key);
            }
            match key {
                VimKey::Char(ch) => {
                    self.vim_insert_raw_text(&ch.to_string(), window, cx);
                }
                VimKey::Tab => self.vim_insert_raw_text("\t", window, cx),
                VimKey::Enter => self.vim_insert_newline(window, cx),
                _ => {}
            }
            return true;
        }
        if self.vim.insert_awaiting_register {
            self.vim.insert_awaiting_register = false;
            if let Some(reg) = key.as_register_name() {
                self.vim_insert_from_register(reg, window, cx);
            }
            return true;
        }
        // 映射等待中的键：能继续拼出插入映射才吞；拼不下去就先把已缓冲的
        // 键按普通输入落下，再重新处理当前键。
        if !self.vim.mapping_pending.is_empty() {
            let buffered = self.vim.mapping_pending.clone();
            let mut sequence = buffered.clone();
            sequence.push(key);
            return match self
                .vim_match_mapping_sequence_mode(VimKeymapMode::Insert, sequence, cx)
            {
                MappingMatch::Fire(action) => {
                    self.vim.mapping_pending.clear();
                    self.vim.mapping_timeout_task = None;
                    self.vim_execute_mapping_action(action, window, cx);
                    true
                }
                MappingMatch::Pending => {
                    self.vim.mapping_pending.push(key);
                    self.vim_arm_mapping_timeout(cx);
                    true
                }
                MappingMatch::None => {
                    self.vim.mapping_pending = buffered;
                    self.vim_flush_mapping_pending(window, cx);
                    self.vim_insert_key(key, window, cx)
                }
            };
        }
        match key {
            VimKey::Esc | VimKey::Ctrl('c') => {
                self.vim_finish_insert_session(window, cx);
                self.vim_cursor_left_on_escape(cx);
                self.vim_enter_mode(VimMode::Normal, cx);
                true
            }
            VimKey::Ctrl('o') => {
                // 先切模式再立标记：vim_enter_mode 会清掉单次命令标记。
                self.vim_enter_mode(VimMode::Normal, cx);
                self.vim.insert_one_shot_normal = true;
                true
            }
            VimKey::Ctrl('r') => {
                self.vim.insert_awaiting_register = true;
                true
            }
            VimKey::Ctrl('v') | VimKey::Ctrl('q') => {
                if !self.vim.replaying {
                    self.vim_record_insert_typed(key);
                }
                self.vim.insert_literal_pending = true;
                true
            }
            VimKey::Ctrl('a') => {
                let text = self.vim.last_insert_text.clone();
                if !text.is_empty() {
                    if !self.vim.replaying {
                        self.vim_record_insert_typed(key);
                    }
                    self.vim_insert_raw_text(&text, window, cx);
                }
                true
            }
            VimKey::Ctrl('u') => {
                self.vim_delete_insert_to_hard_line_start(cx);
                true
            }
            VimKey::Ctrl('w') => {
                self.vim_delete_insert_word_back(cx);
                true
            }
            // 方向键照常移动（应用键位直通），但打断映射等待。
            VimKey::Left | VimKey::Right | VimKey::Up | VimKey::Down | VimKey::Home
            | VimKey::End | VimKey::PageUp | VimKey::PageDown => {
                self.vim_flush_mapping_pending(window, cx);
                false
            }
            VimKey::Char(_) => match self.vim_match_insert_mapping(key, cx) {
                MappingMatch::Fire(action) => {
                    self.vim_execute_mapping_action(action, window, cx);
                    true
                }
                MappingMatch::Pending => {
                    // 首键只启动等待、不落字；下一键由入口处的序列匹配接着拼。
                    self.vim.mapping_pending = vec![key];
                    self.vim_arm_mapping_timeout(cx);
                    true
                }
                MappingMatch::None => {
                    self.vim_flush_mapping_pending(window, cx);
                    if self.vim.replaying {
                        // 重放里的字符不走 IME，直接落字。
                        if let VimKey::Char(ch) = key {
                            let text = ch.to_string();
                            self.vim_insert_raw_text(&text, window, cx);
                        }
                        true
                    } else {
                        self.vim_record_insert_typed(key);
                        false
                    }
                }
            },
            other => {
                self.vim_flush_mapping_pending(window, cx);
                if self.vim.replaying {
                    self.vim_replay_insert_special(other, window, cx)
                } else {
                    self.vim_record_insert_typed(other);
                    false
                }
            }
        }
    }

    /// 映射等待超时：把缓冲的键按原语义补发（vim 的 timeoutlen）。
    pub(super) fn vim_mapping_timeout(
        &mut self,
        epoch: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vim.mapping_epoch != epoch {
            return;
        }
        self.vim_flush_mapping_pending(window, cx);
    }

    fn vim_flush_mapping_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.vim.mapping_timeout_task = None;
        self.vim.mapping_epoch = self.vim.mapping_epoch.wrapping_add(1);
        let pending = std::mem::take(&mut self.vim.mapping_pending);
        if pending.is_empty() {
            return;
        }
        match self.vim.mode {
            VimMode::Insert => {
                let text: String = pending
                    .iter()
                    .filter_map(|key| match key {
                        VimKey::Char(ch) => Some(*ch),
                        _ => None,
                    })
                    .collect();
                if !text.is_empty() {
                    self.vim_insert_raw_text(&text, window, cx);
                    for key in &pending {
                        self.vim_record_insert_typed(*key);
                    }
                }
            }
            // 普通/可视模式：缓冲键按无映射路径重发。
            _ => {
                for key in pending {
                    self.vim_modal_resolve_key(key, window, cx);
                }
            }
        }
    }

    fn vim_arm_mapping_timeout(&mut self, cx: &mut Context<Self>) {
        self.vim.mapping_epoch = self.vim.mapping_epoch.wrapping_add(1);
        let epoch = self.vim.mapping_epoch;
        let timeout_ms = crate::config::EditorSettings::vim(cx).timeout_ms.max(50);
        let timeout = std::time::Duration::from_millis(u64::from(timeout_ms));
        let window_handle = self.window_handle;
        self.vim.mapping_timeout_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(timeout).await;
            let Some(window_handle) = window_handle else {
                return;
            };
            let _ = window_handle.update(cx, |_view, window, cx| {
                let _ = this.update(cx, |editor, cx| {
                    editor.vim_mapping_timeout(epoch, window, cx);
                });
            });
        }));
    }

    fn vim_record_insert_typed(&mut self, key: VimKey) {
        self.vim.insert_typed.push(key);
        if self.vim.insert_typed.len() > 4096 {
            // 防失控：超长会话放弃 `. 重放素材（正常输入不会到这个量级）。
            self.vim.insert_typed.clear();
            self.vim.insert_entry_keys.clear();
        }
    }

    /// 宏重放里遇到非字符插入键（回车/退格等）时的等价操作。
    fn vim_replay_insert_special(
        &mut self,
        key: VimKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match key {
            VimKey::Enter => {
                self.vim_insert_newline(window, cx);
                true
            }
            VimKey::Backspace => {
                self.vim_insert_backspace(cx);
                true
            }
            VimKey::Tab => {
                self.vim_insert_raw_text("\t", window, cx);
                true
            }
            _ => true,
        }
    }

    // ---- replace 模式 ----

    fn vim_replace_key(
        &mut self,
        key: VimKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match key {
            VimKey::Esc | VimKey::Ctrl('c') => {
                self.vim_finish_replace_session(cx);
                self.vim_enter_mode(VimMode::Normal, cx);
                true
            }
            VimKey::Char(ch) => {
                self.vim_replace_overtype(ch, window, cx);
                true
            }
            VimKey::Backspace => {
                self.vim_replace_restore(cx);
                true
            }
            VimKey::Enter => {
                self.vim_replace_session_newline(window, cx);
                true
            }
            // 方向键移动照常（vim 的 R 会话跨移动保留）。
            VimKey::Left | VimKey::Right | VimKey::Up | VimKey::Down | VimKey::Home
            | VimKey::End => false,
            _ => true,
        }
    }

    // ---- 普通/可视共用序列机 ----

    fn vim_modal_key(&mut self, key: VimKey, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // 映射等待中的键：能继续拼出映射才吞，否则先补发缓冲再走常规解析。
        if !self.vim.mapping_pending.is_empty() {
            let mut sequence = self.vim.mapping_pending.clone();
            sequence.push(key);
            return match self.vim_match_mapping_sequence(sequence.clone(), cx) {
                MappingMatch::Fire(action) => {
                    self.vim.mapping_pending.clear();
                    self.vim.mapping_timeout_task = None;
                    self.vim_execute_mapping_action(action, window, cx);
                    true
                }
                MappingMatch::Pending => {
                    self.vim.mapping_pending = sequence;
                    self.vim_arm_mapping_timeout(cx);
                    true
                }
                MappingMatch::None => {
                    self.vim_flush_mapping_pending(window, cx);
                    self.vim_modal_key_inner(key, window, cx)
                }
            };
        }
        self.vim_modal_key_inner(key, window, cx)
    }

    /// 序列机本体：录宏、试映射、归约、执行。
    fn vim_modal_key_inner(
        &mut self,
        key: VimKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // 录宏时 q 先于记录与序列机判停：q 是停止键，不属于宏内容。
        if self.vim.recording.is_some()
            && key == VimKey::Char('q')
            && self.vim.pending.is_empty()
        {
            self.vim_execute(
                VimCommand { count: 0, register: None, action: VimAction::StopRecording },
                &[],
                window,
                cx,
            );
            return true;
        }
        if self.vim.recording.is_some() && !self.vim.replaying {
            if let Some((_, keys)) = &mut self.vim.recording {
                keys.push(key);
            }
        }

        // 序列起始处先试用户映射。
        if self.vim.pending.is_empty() {
            let mode = if matches!(self.vim.mode, VimMode::Visual(_)) {
                VimKeymapMode::Visual
            } else {
                VimKeymapMode::Normal
            };
            match self.vim_match_mapping(mode, key, cx) {
                MappingMatch::Fire(action) => {
                    self.vim_execute_mapping_action(action, window, cx);
                    return true;
                }
                MappingMatch::Pending => {
                    self.vim.mapping_pending = vec![key];
                    self.vim_arm_mapping_timeout(cx);
                    return true;
                }
                MappingMatch::None => {}
            }
        }

        self.vim_modal_resolve_key(key, window, cx)
    }

    /// 只走序列机，不再试用户映射。映射前缀超时后重发缓冲键必须走这里，
    /// 否则单键又会命中同一条映射的前缀，再次挂起等待。
    fn vim_modal_resolve_key(&mut self, key: VimKey, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.vim.pending.push(key);
        // 护栏：合法序列可以到 12 键（"a9999d9999w = 寄存器 2 键 + 两个 4 位
        // 计数 + 操作符 + 动作），留一点余量到 16；解析器自己会在不可能成为
        // 合法前缀时给出 Invalid。
        if self.vim.pending.len() > 16 {
            self.vim.pending.clear();
            return true;
        }
        let pending = self.vim.pending.clone();
        let outcome = if matches!(self.vim.mode, VimMode::Visual(_)) {
            self.vim_parse_visual_sequence(&pending)
        } else {
            self.vim_parse_normal_sequence(&pending)
        };
        match outcome {
            ParseOutcome::NeedMore => true,
            ParseOutcome::Invalid => {
                self.vim.pending.clear();
                true
            }
            ParseOutcome::Execute(command) => {
                let entry_keys = std::mem::take(&mut self.vim.pending);
                self.vim_execute(command, &entry_keys, window, cx);
                true
            }
        }
    }

    /// 行级可视的 J/gJ：返回选区起点与覆盖的硬行数（至少 1）。
    fn vim_visual_linewise_join_target(
        &self,
        cx: &App,
    ) -> Option<(operate::VimRange, u32)> {
        let entities = self.vim_navigation_entities(cx);
        let range = visual::vim_selection_as_range(self, &entities, cx)?;
        if !range.linewise {
            return None;
        }
        let stream = motion::VimDocStream::new(&entities, cx);
        let mut position = range.start;
        let mut lines = 0u32;
        while (position.block_index, position.offset)
            < (range.end.block_index, range.end.offset)
        {
            lines += 1;
            let line = stream.hard_line_range(position.block_index, position.offset);
            let text_len = stream.text_for_line(position.block_index).len();
            position = if line.end < text_len {
                motion::VimPosition {
                    block_index: position.block_index,
                    offset: line.end + 1,
                }
            } else if position.block_index + 1 < entities.len() {
                motion::VimPosition {
                    block_index: position.block_index + 1,
                    offset: 0,
                }
            } else {
                break;
            };
        }
        Some((range, lines.max(1)))
    }

    // ---- 用户映射 ----

    fn vim_match_mapping(
        &self,
        mode: VimKeymapMode,
        key: VimKey,
        cx: &App,
    ) -> MappingMatch {
        self.vim_match_mapping_sequence_mode(mode, vec![key], cx)
    }

    fn vim_match_mapping_sequence(&self, sequence: Vec<VimKey>, cx: &App) -> MappingMatch {
        let mode = if matches!(self.vim.mode, VimMode::Visual(_)) {
            VimKeymapMode::Visual
        } else {
            VimKeymapMode::Normal
        };
        self.vim_match_mapping_sequence_mode(mode, sequence, cx)
    }

    fn vim_match_insert_mapping(&mut self, key: VimKey, cx: &App) -> MappingMatch {
        self.vim_match_mapping_sequence_mode(VimKeymapMode::Insert, vec![key], cx)
    }

    fn vim_match_mapping_sequence_mode(
        &self,
        mode: VimKeymapMode,
        sequence: Vec<VimKey>,
        cx: &App,
    ) -> MappingMatch {
        let preferences = crate::config::EditorSettings::vim(cx);
        let mut best: Option<(usize, usize)> = None;
        for (index, entry) in preferences.keymaps.iter().enumerate() {
            if entry.mode != mode {
                continue;
            }
            let mapped: Vec<VimKey> = entry.keys.chars().map(VimKey::Char).collect();
            if mapped.len() < sequence.len() {
                continue;
            }
            if mapped[..sequence.len()] == sequence[..] {
                match best {
                    Some((length, _)) if length >= mapped.len() => {}
                    _ => best = Some((mapped.len(), index)),
                }
            }
        }
        match best {
            Some((length, index)) if length == sequence.len() => {
                MappingMatch::Fire(preferences.keymaps[index].action.clone())
            }
            Some(_) => MappingMatch::Pending,
            None => MappingMatch::None,
        }
    }

    fn vim_execute_mapping_action(
        &mut self,
        action: VimKeymapAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let vim_action = match action {
            VimKeymapAction::Esc => VimAction::EnterNormalMode,
            VimKeymapAction::Insert => VimAction::EnterInsert(InsertPlacement::AtCursor),
            VimKeymapAction::Append => VimAction::EnterInsert(InsertPlacement::AfterCursor),
            VimKeymapAction::Visual => VimAction::ToggleVisual(VisualKind::Char),
            VimKeymapAction::VisualLine => VimAction::ToggleVisual(VisualKind::Line),
            VimKeymapAction::Command(id) => {
                self.vim_execute_app_command(&id, window, cx);
                return;
            }
        };
        self.vim_execute(
            VimCommand { count: 0, register: None, action: vim_action },
            &[],
            window,
            cx,
        );
    }

    /// 映射到命令注册表的应用命令。派发会更新同一个编辑器实体，必须延后到
    /// 当前按键处理结束之后，避免重入 update。
    pub(super) fn vim_execute_app_command(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {        let Some(command) = crate::commands::command_by_id(id) else {
            return;
        };
        let id = id.to_string();
        let action = command.boxed_action();
        let target = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.defer(move |cx| {
            if let Err(error) = window_handle.update(cx, |_view, window, cx| {
                crate::app_menu::dispatch_menu_action_for_editor(
                    action.as_ref(),
                    &target,
                    window,
                    cx,
                );
            }) {
                eprintln!("vim 映射命令 {id} 派发失败：{error}");
            }
        });
    }

    // ---- 执行器 ----

    fn vim_execute(
        &mut self,
        command: VimCommand,
        entry_keys: &[VimKey],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let VimCommand { count, register, action } = command;
        let is_change = matches!(
            action,
            VimAction::Operate(Operator::Delete | Operator::Change | Operator::Indent
                | Operator::Outdent | Operator::Lowercase | Operator::Uppercase
                | Operator::SwapCase | Operator::ReplaceWithRegister | Operator::ToggleComment, _)
                | VimAction::ReplaceChar(_)
                | VimAction::VisualReplaceChar(_)
                | VimAction::SurroundAdd { .. }
                | VimAction::SurroundDelete(_)
                | VimAction::SurroundChange { .. }
                | VimAction::VisualSurround(_)
                | VimAction::Paste { .. }
                | VimAction::JoinLines { .. }
                | VimAction::Increment(_)
                | VimAction::SwapCaseAtCursor
                | VimAction::EnterInsert(_)
        );
        // 只读（阅读模式）下编辑命令整体不执行：正文本来就被块级只读守卫拒掉，
        // 但寄存器与系统剪贴板会被「删掉的内容」覆盖，光标和模式也照命令走，
        // 用户以为删成了。yank 不在 is_change 里，只读下仍可复制。
        if is_change && self.is_read_only() {
            cx.notify();
            return;
        }
        // 可视粘贴的入口键（p/P）在普通模式下无法重放；视觉重放素材单独记录。
        let visual_paste = matches!(action, VimAction::Paste { .. })
            && matches!(self.vim.mode, VimMode::Visual(_));
        let was_visual = matches!(self.vim.mode, VimMode::Visual(_));
        let visual_repeat = if was_visual {
            self.vim_capture_visual_repeat(cx)
        } else {
            None
        };
        // 撤销分组在编辑器层同步建立：块级 PrepareUndo 是排队事件，vim 直接改
        // 文本时事件还没到，撤销栈里就一条记录也没有。
        let capture_undo = is_change
            && !matches!(action, VimAction::EnterInsert(_) | VimAction::EnterReplaceMode);
        if capture_undo {
            self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        }
        match action {
            VimAction::Move(motion) => {
                let repeat = count.max(1);
                if matches!(self.vim.mode, VimMode::Visual(_)) {
                    visual::vim_extend_with_motion(self, motion, repeat, window, cx);
                } else {
                    motion::vim_move(self, motion, repeat, window, cx);
                }
            }
            VimAction::Operate(operator, target) => {
                operate::vim_apply_operator(self, operator, target, count, register, window, cx);
                // c 类操作符的插入会话要带上入口键，Esc 时才能把「删到哪里 +
                // 输入了什么」一起交给 `.`。可视 c 还要带上选区形状。
                if operator == Operator::Change
                    && self.vim.mode == VimMode::Insert
                    && self.vim.insert_entry_keys.is_empty()
                {
                    if was_visual {
                        if let Some(repeat) = visual_repeat {
                            self.vim.insert_entry_keys = entry_keys.to_vec();
                            self.vim.insert_visual_repeat = Some(repeat);
                        }
                    } else {
                        self.vim.insert_entry_keys = entry_keys.to_vec();
                    }
                }
            }
            VimAction::VisualObject(object) => {
                visual::vim_extend_to_object(self, object, window, cx);
            }
            VimAction::VisualReplaceChar(ch) => {
                operate::vim_visual_replace_char(self, ch, window, cx);
            }
            VimAction::SurroundAdd { target, pair } => {
                operate::vim_surround_add(self, target, pair, cx);
            }
            VimAction::SurroundDelete(pair) => {
                operate::vim_surround_delete(self, pair, window, cx);
            }
            VimAction::SurroundChange { old, new } => {
                operate::vim_surround_change(self, old, new, cx);
            }
            VimAction::VisualSurround(pair) => {
                operate::vim_visual_surround(self, pair, cx);
            }
            VimAction::SelectSearchMatch { reverse } => {
                command::vim_select_search_match(self, reverse, cx);
            }
            VimAction::EnterInsert(placement) => {
                operate::vim_enter_insert(self, placement, count, entry_keys, window, cx);
            }
            VimAction::EnterReplaceMode => operate::vim_enter_replace(self, cx),
            VimAction::ReplaceChar(ch) => operate::vim_replace_char(self, ch, count, window, cx),
            VimAction::SwapCaseAtCursor => {
                operate::vim_swap_case_at_cursor(self, count, cx);
            }
            VimAction::Increment(delta) => {
                operate::vim_increment(self, delta, count, cx);
            }
            VimAction::Paste { after } => {
                if visual_paste {
                    operate::vim_visual_paste(self, register, count, window, cx);
                } else {
                    operate::vim_paste(self, register, after, count, window, cx);
                }
            }
            VimAction::JoinLines { count: join_count, keep_space } => {
                let mut join_count = join_count.max(1);
                // 行级可视的 J/gJ 要把选区覆盖的全部行并成一行：解析阶段只拿到
                // 按键前缀的 count（默认 2），这里按选区行数补上，并从首行开始。
                if was_visual
                    && matches!(self.vim.mode, VimMode::Visual(VisualKind::Line))
                    && let Some((range, lines)) = self.vim_visual_linewise_join_target(cx)
                {
                    visual::vim_exit_visual(self, cx);
                    let entities = self.vim_navigation_entities(cx);
                    self.vim_set_cursor(&entities, range.start, cx);
                    join_count = lines;
                }
                operate::vim_join(self, join_count, keep_space, window, cx);
            }
            VimAction::Undo => self.undo_document(cx),
            VimAction::Redo => self.redo_document(cx),
            VimAction::Dot => {
                let Some(record) = self.vim.dot.clone() else {
                    return;
                };
                let times = count.max(1);
                for _ in 0..times {
                    self.vim_replay_record(&record, window, cx);
                }
            }
            VimAction::RepeatFind { reverse } => {
                motion::vim_repeat_find(self, reverse, count, window, cx);
            }
            VimAction::SetMark(mark) => operate::vim_set_mark(self, mark, cx),
            VimAction::GotoMark { mark, linewise } => {
                operate::vim_goto_mark(self, mark, linewise, window, cx);
            }
            VimAction::ToggleRecording(register_name) => {
                self.vim.recording = Some((register_name, Vec::new()));
            }
            VimAction::StopRecording => {
                if let Some((register_name, keys)) = self.vim.recording.take() {
                    if !keys.is_empty() {
                        self.vim.macro_registers.insert(register_name, keys);
                        self.vim.last_macro_register = Some(register_name);
                    }
                }
            }
            VimAction::ReplayMacro(register_name) => {
                self.vim_replay_macro(register_name, count, window, cx);
            }
            VimAction::ReplayLastMacro => {
                if let Some(register_name) = self.vim.last_macro_register {
                    self.vim_replay_macro(register_name, count, window, cx);
                }
            }
            VimAction::Scroll(kind) => motion::vim_scroll(self, kind, count, window, cx),
            VimAction::ViewportAlign(align) => motion::vim_viewport_align(self, align, cx),
            VimAction::Search { reverse } => command::vim_request_cmdline(
                self,
                command::CmdlineKind::Search { reverse },
                cx,
            ),
            VimAction::SearchWord { reverse, whole_word } => {
                command::vim_search_word(self, reverse, whole_word, window, cx);
            }
            VimAction::SearchNext { reverse } => command::vim_search_next(self, reverse, cx),
            VimAction::OpenCmdline(kind) => command::vim_request_cmdline(self, kind, cx),
            VimAction::ToggleVisual(kind) => visual::vim_toggle_visual(self, kind, cx),
            VimAction::ExitVisual => visual::vim_exit_visual(self, cx),
            VimAction::ExchangeVisualAnchor => visual::vim_exchange_anchor(self, cx),
            VimAction::SwapVisualBlockColumn => visual::vim_swap_block_column(self, cx),
            VimAction::ReselectVisual => visual::vim_reselect(self, window, cx),
            VimAction::JumplistBack => operate::vim_jumplist_navigate(self, true, window, cx),
            VimAction::JumplistForward => operate::vim_jumplist_navigate(self, false, window, cx),
            VimAction::EnterNormalMode => {
                if matches!(self.vim.mode, VimMode::Visual(_)) {
                    visual::vim_exit_visual(self, cx);
                } else if self.vim.mode == VimMode::Replace {
                    self.vim_finish_replace_session(cx);
                }
                self.vim_enter_mode(VimMode::Normal, cx);
            }
            VimAction::SaveAndClose => command::vim_save_and_close(self, window, cx),
            VimAction::CloseWithoutSaving => command::vim_close_discarding(self, window, cx),
        }

        // ctrl-o：单条普通命令执行完回插入；命令自己切了模式则以命令为准。
        if self.vim.insert_one_shot_normal {
            let back_to_insert = self.vim.mode == VimMode::Normal;
            self.vim.insert_one_shot_normal = false;
            if back_to_insert {
                self.vim_enter_mode(VimMode::Insert, cx);
            }
        }

        // 修改操作符把插入会话并进同一组，等 Esc 收尾时再落账。目标解析失败的
        // Change 没进插入模式（mode 仍是 Normal）：必须当场收组，否则下一笔
        // 编辑并进这个空组，选区快照还停留在失败命令的时刻。
        let change_entered_insert = matches!(action, VimAction::Operate(Operator::Change, _))
            && self.vim.mode == VimMode::Insert;
        if capture_undo && !change_entered_insert {
            self.finalize_pending_undo_capture(cx);
        }

        // `.` 素材：变更命令记下入口键（插入会话的键在 esc 时补记）；
        // 失败的 Change 什么都没改，不记。
        let change_failed = matches!(action, VimAction::Operate(Operator::Change, _))
            && self.vim.mode != VimMode::Insert;
        if is_change && !self.vim.replaying && !change_failed {
            if was_visual && visual_repeat.is_none() {
                // 块状可视暂时没有可重建的形状，宁可清空也不要留下待解析操作符。
                self.vim.dot = None;
            } else if !entry_keys.is_empty() {
                self.vim.dot = Some(DotRecord {
                    entry_keys: entry_keys.to_vec(),
                    insert_keys: Vec::new(),
                    visual: visual_repeat,
                });
            }
        }
    }

    fn vim_replay_macro(
        &mut self,
        register_name: char,
        count: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(keys) = self.vim.macro_registers.get(&register_name).cloned() else {
            return;
        };
        if self.vim.macro_stack.contains(&register_name) {
            return;
        }
        self.vim.last_macro_register = Some(register_name);
        let times = count.max(1);
        let mut guard_counter = 0usize;
        self.vim.replaying = true;
        self.vim.macro_stack.push(register_name);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for _ in 0..times {
                for key in &keys {
                    guard_counter += 1;
                    if guard_counter > 100_000 {
                        return;
                    }
                    self.vim_route_key(*key, window, cx);
                }
            }
        }));
        self.vim.replaying = false;
        self.vim.macro_stack.retain(|entry| *entry != register_name);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    /// `.` 与 `3o` 的重放：入口命令 → 插入会话 → 退出插入。
    pub(super) fn vim_replay_record(&mut self, record: &DotRecord, window: &mut Window, cx: &mut Context<Self>) {
        self.vim.replaying = true;
        // 与 vim_replay_macro 同规：重放途中 panic 也必须把 replaying 复位，
        // 否则此后所有按键都按重放路径走、不再记 dot/宏。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(repeat) = record.visual {
                self.vim_replay_visual_repeat(repeat, window, cx);
            }
            for key in &record.entry_keys {
                self.vim_route_key(*key, window, cx);
            }
            for key in &record.insert_keys {
                self.vim_route_key(*key, window, cx);
            }
            if self.vim.mode == VimMode::Insert {
                self.vim_route_key(VimKey::Esc, window, cx);
            }
        }));
        self.vim.replaying = false;
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    /// 记录当前可视选区的形状，`.` 重放时按同样的字素数/行数重建。
    fn vim_capture_visual_repeat(&self, cx: &App) -> Option<VisualRepeat> {
        let kind = match self.vim.mode {
            VimMode::Visual(kind) => kind,
            _ => return None,
        };
        if kind == VisualKind::Block {
            return None;
        }
        let anchor = self.vim.visual_anchor?;
        let entities = self.vim_navigation_entities(cx);
        let focus = self.vim_current_position(&entities, cx)?;
        let forward = (anchor.block_index, anchor.offset) <= (focus.block_index, focus.offset);
        let stream = motion::VimDocStream::new(&entities, cx);
        match kind {
            VisualKind::Char => {
                let mut current = anchor;
                let mut distance = 0usize;
                while current != focus {
                    let step = if forward {
                        stream.next_grapheme(current)
                    } else {
                        stream.prev_grapheme(current)
                    };
                    let Some(next) = step else {
                        break;
                    };
                    current = next;
                    distance += 1;
                    if distance > 1_000_000 {
                        break;
                    }
                }
                Some(VisualRepeat { kind, forward, distance })
            }
            VisualKind::Line => {
                let (first, last) = if forward { (anchor, focus) } else { (focus, anchor) };
                let mut current = first;
                let mut line_count = 1usize;
                while current != last {
                    let line = stream.hard_line_range(current.block_index, current.offset);
                    let text_len = stream.text_for_line(current.block_index).len();
                    let next = if line.end < text_len {
                        VimPosition {
                            block_index: current.block_index,
                            offset: line.end + 1,
                        }
                    } else if current.block_index + 1 < entities.len() {
                        VimPosition {
                            block_index: current.block_index + 1,
                            offset: 0,
                        }
                    } else {
                        break;
                    };
                    if (next.block_index, next.offset) > (last.block_index, last.offset) {
                        break;
                    }
                    current = next;
                    line_count += 1;
                    if line_count > 1_000_000 {
                        break;
                    }
                }
                Some(VisualRepeat {
                    kind,
                    forward,
                    distance: line_count.saturating_sub(1),
                })
            }
            VisualKind::Block => None,
        }
    }

    fn vim_replay_visual_repeat(
        &mut self,
        repeat: VisualRepeat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        visual::vim_toggle_visual(self, repeat.kind, cx);
        if repeat.distance == 0 {
            return;
        }
        let motion = match repeat.kind {
            VisualKind::Char => {
                if repeat.forward {
                    Motion::ForwardChars
                } else {
                    Motion::BackwardChars
                }
            }
            VisualKind::Line => {
                if repeat.forward {
                    Motion::Down
                } else {
                    Motion::Up
                }
            }
            VisualKind::Block => return,
        };
        let count = repeat.distance.min(u32::MAX as usize) as u32;
        visual::vim_extend_with_motion(self, motion, count, window, cx);
    }

    // ---- 序列解析（普通模式）----

    fn vim_parse_normal_sequence(&self, keys: &[VimKey]) -> ParseOutcome {
        let (count_a, index) = Self::vim_parse_count(keys, 0);
        let (register, index) = Self::vim_parse_register(keys, index);
        let (count_b, index) = Self::vim_parse_count(keys, index);
        let count = if count_a == 0 {
            count_b
        } else if count_b == 0 {
            count_a
        } else {
            count_a.saturating_mul(count_b)
        };
        if index >= keys.len() {
            return ParseOutcome::NeedMore;
        }
        let head = keys[index];
        let rest = &keys[index + 1..];

        let action = match head {
            VimKey::Char('d') | VimKey::Char('c') | VimKey::Char('y') => {
                let operator = match head {
                    VimKey::Char('d') => Operator::Delete,
                    VimKey::Char('c') => Operator::Change,
                    _ => Operator::Yank,
                };
                if rest.first() == Some(&VimKey::Char('s')) {
                    return match head {
                        VimKey::Char('y') => Self::vim_parse_surround_add(&rest[1..], count),
                        VimKey::Char('d') => Self::vim_parse_surround_delete(&rest[1..]),
                        _ => Self::vim_parse_surround_change(&rest[1..]),
                    };
                }
                if rest.is_empty() {
                    return ParseOutcome::NeedMore;
                }
                if rest[0] == head {
                    let (extra, _) = Self::vim_parse_count(&rest[1..], 0);
                    let lines = count.max(1).max(extra.max(1));
                    return ParseOutcome::Execute(VimCommand {
                        count: lines,
                        register,
                        action: VimAction::Operate(operator, OperatorTarget::CurrentLines),
                    });
                }
                if let Some(lines) = Self::vim_linewise_repeat_count(head, rest, count) {
                    return ParseOutcome::Execute(VimCommand {
                        count: lines,
                        register,
                        action: VimAction::Operate(operator, OperatorTarget::CurrentLines),
                    });
                }
                return Self::vim_parse_operator_target(operator, rest, count, register);
            }
            VimKey::Char('>') | VimKey::Char('<') => {
                let operator = if head == VimKey::Char('>') {
                    Operator::Indent
                } else {
                    Operator::Outdent
                };
                if rest.is_empty() {
                    return ParseOutcome::NeedMore;
                }
                if rest[0] == head {
                    return ParseOutcome::Execute(VimCommand {
                        count: count.max(1),
                        register,
                        action: VimAction::Operate(operator, OperatorTarget::CurrentLines),
                    });
                }
                if let Some(lines) = Self::vim_linewise_repeat_count(head, rest, count) {
                    return ParseOutcome::Execute(VimCommand {
                        count: lines,
                        register,
                        action: VimAction::Operate(operator, OperatorTarget::CurrentLines),
                    });
                }
                return Self::vim_parse_operator_target(operator, rest, count, register);
            }
            VimKey::Char('g') => {
                if rest.first() == Some(&VimKey::Char('c')) {
                    if rest.get(1) == Some(&VimKey::Char('c')) {
                        return ParseOutcome::Execute(VimCommand {
                            count: count.max(1),
                            register,
                            action: VimAction::Operate(
                                Operator::ToggleComment,
                                OperatorTarget::CurrentLines,
                            ),
                        });
                    }
                    return Self::vim_parse_operator_target(
                        Operator::ToggleComment,
                        &rest[1..],
                        count,
                        register,
                    );
                }
                if rest.first() == Some(&VimKey::Char('r')) {
                    if rest.get(1) == Some(&VimKey::Char('r')) {
                        return ParseOutcome::Execute(VimCommand {
                            count: count.max(1),
                            register,
                            action: VimAction::Operate(
                                Operator::ReplaceWithRegister,
                                OperatorTarget::CurrentLines,
                            ),
                        });
                    }
                    return Self::vim_parse_operator_target(
                        Operator::ReplaceWithRegister,
                        &rest[1..],
                        count,
                        register,
                    );
                }
                return Self::vim_parse_g_sequence(rest, count, register);
            }
            VimKey::Char('z') => return Self::vim_parse_z_sequence(rest),
            VimKey::Char('f') | VimKey::Char('F') | VimKey::Char('t') | VimKey::Char('T') => {
                let Some(ch) = rest.first().and_then(|key| key.as_char()) else {
                    return ParseOutcome::NeedMore;
                };
                VimAction::Move(Motion::FindChar {
                    ch,
                    reverse: matches!(head, VimKey::Char('F') | VimKey::Char('T')),
                    till: matches!(head, VimKey::Char('t') | VimKey::Char('T')),
                })
            }
            VimKey::Char('r') => {
                let Some(ch) = rest.first().and_then(|key| key.as_char()) else {
                    return ParseOutcome::NeedMore;
                };
                VimAction::ReplaceChar(ch)
            }
            VimKey::Char('~') => VimAction::SwapCaseAtCursor,
            VimKey::Char('m') => {
                let Some(mark) = rest.first().and_then(|key| key.as_char()) else {
                    return ParseOutcome::NeedMore;
                };
                if !mark.is_ascii_alphabetic() {
                    return ParseOutcome::Invalid;
                }
                VimAction::SetMark(mark)
            }
            VimKey::Char('`') | VimKey::Char('\'') => {
                let Some(mark) = rest.first() else {
                    return ParseOutcome::NeedMore;
                };
                match mark {
                    VimKey::Char('<') | VimKey::Char('>') => VimAction::GotoMark {
                        mark: mark.as_char().unwrap_or('<'),
                        linewise: head == VimKey::Char('\''),
                    },
                    VimKey::Char(mark_char) if mark_char.is_ascii_alphabetic() => {
                        VimAction::GotoMark {
                            mark: *mark_char,
                            linewise: head == VimKey::Char('\''),
                        }
                    }
                    _ => return ParseOutcome::Invalid,
                }
            }
            VimKey::Char('q') => {
                let Some(register_name) = rest.first().and_then(|key| key.as_register_name()) else {
                    return ParseOutcome::NeedMore;
                };
                if !register_name.is_ascii_alphabetic() {
                    return ParseOutcome::Invalid;
                }
                VimAction::ToggleRecording(register_name)
            }
            VimKey::Char('@') => {
                if rest.first() == Some(&VimKey::Char('@')) {
                    VimAction::ReplayLastMacro
                } else if let Some(register_name) =
                    rest.first().and_then(|key| key.as_register_name())
                {
                    if register_name.is_ascii_alphanumeric() {
                    VimAction::ReplayMacro(register_name)
                    } else {
                        return ParseOutcome::Invalid;
                    }
                } else {
                    return ParseOutcome::NeedMore;
                }
            }
            VimKey::Char('i') => VimAction::EnterInsert(InsertPlacement::AtCursor),
            VimKey::Char('I') => VimAction::EnterInsert(InsertPlacement::LineStart),
            VimKey::Char('a') => VimAction::EnterInsert(InsertPlacement::AfterCursor),
            VimKey::Char('A') => VimAction::EnterInsert(InsertPlacement::LineEnd),
            VimKey::Char('o') => VimAction::EnterInsert(InsertPlacement::Below),
            VimKey::Char('O') => VimAction::EnterInsert(InsertPlacement::Above),
            VimKey::Char('v') => VimAction::ToggleVisual(VisualKind::Char),
            VimKey::Char('V') => VimAction::ToggleVisual(VisualKind::Line),
            VimKey::Ctrl('q') | VimKey::Ctrl('v') => {
                VimAction::ToggleVisual(VisualKind::Block)
            }
            VimKey::Char('R') => VimAction::EnterReplaceMode,
            VimKey::Char('x') => VimAction::Operate(
                Operator::Delete,
                OperatorTarget::Motion(Motion::ForwardChars, 1),
            ),
            VimKey::Char('X') => VimAction::Operate(
                Operator::Delete,
                OperatorTarget::Motion(Motion::BackwardChars, 1),
            ),
            VimKey::Char('s') => VimAction::Operate(
                Operator::Change,
                OperatorTarget::Motion(Motion::ForwardChars, 1),
            ),
            VimKey::Char('S') => {
                VimAction::Operate(Operator::Change, OperatorTarget::CurrentLines)
            }
            VimKey::Char('D') => VimAction::Operate(
                Operator::Delete,
                OperatorTarget::Motion(Motion::HardLineEnd, 1),
            ),
            VimKey::Char('C') => VimAction::Operate(
                Operator::Change,
                OperatorTarget::Motion(Motion::HardLineEnd, 1),
            ),
            VimKey::Char('Y') => VimAction::Operate(Operator::Yank, OperatorTarget::CurrentLines),
            VimKey::Char('p') => VimAction::Paste { after: true },
            VimKey::Char('P') => VimAction::Paste { after: false },
            VimKey::Char('J') => VimAction::JoinLines {
                count: vim_join_line_count(count),
                keep_space: true,
            },
            VimKey::Char('u') => VimAction::Undo,
            VimKey::Ctrl('r') => VimAction::Redo,
            VimKey::Char('.') => VimAction::Dot,
            VimKey::Char(';') => VimAction::RepeatFind { reverse: false },
            VimKey::Char(',') => VimAction::RepeatFind { reverse: true },
            VimKey::Char('/') => VimAction::Search { reverse: false },
            VimKey::Char('?') => VimAction::Search { reverse: true },
            VimKey::Char('n') => VimAction::SearchNext { reverse: false },
            VimKey::Char('N') => VimAction::SearchNext { reverse: true },
            VimKey::Char('*') => VimAction::SearchWord { reverse: false, whole_word: true },
            VimKey::Char('#') => VimAction::SearchWord { reverse: true, whole_word: true },
            VimKey::Char(':') => VimAction::OpenCmdline(command::CmdlineKind::Command),
            VimKey::Char('%') => VimAction::Move(Motion::MatchPair),
            VimKey::Char('|') => VimAction::Move(Motion::ColumnNo(count.max(1))),
            VimKey::Esc => VimAction::EnterNormalMode,
            VimKey::Char('Z') => {
                match rest.first() {
                    Some(VimKey::Char('Z')) => VimAction::SaveAndClose,
                    Some(VimKey::Char('Q')) => VimAction::CloseWithoutSaving,
                    Some(_) => return ParseOutcome::Invalid,
                    None => return ParseOutcome::NeedMore,
                }
            }
            other => return Self::vim_motion_action(other, count),
        };
        ParseOutcome::Execute(VimCommand { count, register, action })
    }

    /// 纯动作键（移动/滚动/跳转表）：普通与可视解析共用。
    fn vim_motion_action(head: VimKey, count: u32) -> ParseOutcome {
        let action = match head {
            VimKey::Char('h') | VimKey::Left | VimKey::Backspace => VimAction::Move(Motion::Left),
            VimKey::Char('l') | VimKey::Right | VimKey::Char(' ') => VimAction::Move(Motion::Right),
            VimKey::Char('j') | VimKey::Down | VimKey::Enter => VimAction::Move(Motion::Down),
            VimKey::Char('k') | VimKey::Up => VimAction::Move(Motion::Up),
            VimKey::Char('G') => {
                if count > 0 {
                    VimAction::Move(Motion::GoToLine(count))
                } else {
                    VimAction::Move(Motion::DocEnd)
                }
            }
            VimKey::Char('0') | VimKey::Home => VimAction::Move(Motion::HardLineStart),
            VimKey::Char('$') | VimKey::End => VimAction::Move(Motion::HardLineEnd),
            VimKey::Char('^') => VimAction::Move(Motion::FirstNonBlank),
            VimKey::Char('w') => VimAction::Move(Motion::WordForward { big: false }),
            VimKey::Char('W') => VimAction::Move(Motion::WordForward { big: true }),
            VimKey::Char('b') => VimAction::Move(Motion::WordBackward { big: false }),
            VimKey::Char('B') => VimAction::Move(Motion::WordBackward { big: true }),
            VimKey::Char('e') => VimAction::Move(Motion::WordEnd { big: false }),
            VimKey::Char('E') => VimAction::Move(Motion::WordEnd { big: true }),
            VimKey::Char('{') => VimAction::Move(Motion::ParagraphBackward),
            VimKey::Char('}') => VimAction::Move(Motion::ParagraphForward),
            VimKey::Char('(') => VimAction::Move(Motion::SentenceBackward),
            VimKey::Char(')') => VimAction::Move(Motion::SentenceForward),
            VimKey::Char('H') => VimAction::Move(Motion::ScreenLine(ScreenCursorKind::Top)),
            VimKey::Char('M') => VimAction::Move(Motion::ScreenLine(ScreenCursorKind::Middle)),
            VimKey::Char('L') => VimAction::Move(Motion::ScreenLine(ScreenCursorKind::Bottom)),
            VimKey::Ctrl('d') => VimAction::Scroll(ScrollKind::HalfPageDown),
            VimKey::Ctrl('u') => VimAction::Scroll(ScrollKind::HalfPageUp),
            VimKey::Ctrl('f') => VimAction::Scroll(ScrollKind::PageDown),
            VimKey::Ctrl('b') => VimAction::Scroll(ScrollKind::PageUp),
            VimKey::Ctrl('a') => VimAction::Increment(1),
            VimKey::Ctrl('x') => VimAction::Increment(-1),
            VimKey::Ctrl('e') => VimAction::Scroll(ScrollKind::LineDown),
            VimKey::Ctrl('y') => VimAction::Scroll(ScrollKind::LineUp),
            VimKey::Ctrl('o') => VimAction::JumplistBack,
            VimKey::Ctrl('i') => VimAction::JumplistForward,
            _ => return ParseOutcome::Invalid,
        };
        ParseOutcome::Execute(VimCommand { count, register: None, action })
    }

    /// `g` 前缀族。
    fn vim_parse_g_sequence(
        rest: &[VimKey],
        count: u32,
        register: Option<char>,
    ) -> ParseOutcome {
        let Some(next) = rest.first() else {
            return ParseOutcome::NeedMore;
        };
        let action = match next {
            VimKey::Char('g') => {
                if count > 0 {
                    VimAction::Move(Motion::GoToLine(count))
                } else {
                    VimAction::Move(Motion::DocStart)
                }
            }
            VimKey::Char('0') => VimAction::Move(Motion::HardLineStart),
            VimKey::Char('i') => VimAction::EnterInsert(InsertPlacement::LastInsertPosition),
            VimKey::Char('I') => VimAction::EnterInsert(InsertPlacement::FirstColumn),
            VimKey::Char('J') => VimAction::JoinLines { count: vim_join_line_count(count), keep_space: false },
            VimKey::Char('v') => VimAction::ReselectVisual,
            VimKey::Char('e') => VimAction::Move(Motion::WordEndBackward { big: false }),
            VimKey::Char('E') => VimAction::Move(Motion::WordEndBackward { big: true }),
            VimKey::Char('n') => VimAction::SelectSearchMatch { reverse: false },
            VimKey::Char('N') => VimAction::SelectSearchMatch { reverse: true },
            VimKey::Char('*') => VimAction::SearchWord { reverse: false, whole_word: false },
            VimKey::Char('#') => VimAction::SearchWord { reverse: true, whole_word: false },
            VimKey::Char('_') => VimAction::Move(Motion::LastNonBlank),
            VimKey::Char('u') | VimKey::Char('U') | VimKey::Char('~') => {
                let operator = match next {
                    VimKey::Char('u') => Operator::Lowercase,
                    VimKey::Char('U') => Operator::Uppercase,
                    _ => Operator::SwapCase,
                };
                // 双写（guu/gUU/g~~）= 当前行。
                match rest.get(1) {
                    Some(key) if key == next => VimAction::Operate(
                        operator,
                        OperatorTarget::CurrentLines,
                    ),
                    Some(_) => {
                        return Self::vim_parse_operator_target(operator, &rest[1..], count, register)
                    }
                    None => return ParseOutcome::NeedMore,
                }
            }
            _ => return ParseOutcome::Invalid,
        };
        ParseOutcome::Execute(VimCommand { count, register, action })
    }

    fn vim_parse_z_sequence(rest: &[VimKey]) -> ParseOutcome {
        let Some(next) = rest.first() else {
            return ParseOutcome::NeedMore;
        };
        let action = match next {
            VimKey::Char('z') | VimKey::Char('.') | VimKey::Enter => {
                VimAction::ViewportAlign(ViewportAlign::Center)
            }
            VimKey::Char('t') => VimAction::ViewportAlign(ViewportAlign::Top),
            VimKey::Char('b') | VimKey::Char('-') => VimAction::ViewportAlign(ViewportAlign::Bottom),
            _ => return ParseOutcome::Invalid,
        };
        ParseOutcome::Execute(VimCommand { count: 0, register: None, action })
    }

    /// 操作符后接目标（计数/文本对象/动作）。
    fn vim_parse_operator_target(
        operator: Operator,
        rest: &[VimKey],
        count: u32,
        register: Option<char>,
    ) -> ParseOutcome {
        let (tail_count, tail_index) = Self::vim_parse_count(rest, 0);
        let rest = &rest[tail_index..];
        let total = count.max(1).saturating_mul(tail_count.max(1));
        // total 恒 ≥1；G/gg 要靠「有没有显式计数」区分文末/文首与 {count}G。
        let explicit_count = count > 0 || tail_count > 0;
        let Some(next) = rest.first().copied() else {
            return ParseOutcome::NeedMore;
        };
        // 文本对象 i/a 系列。
        if matches!(next, VimKey::Char('i') | VimKey::Char('a')) {
            if rest.len() < 2 {
                return ParseOutcome::NeedMore;
            }
            if let Some(object) = TextObject::parse(next == VimKey::Char('i'), rest[1].as_char()) {
                return ParseOutcome::Execute(VimCommand {
                    count: total,
                    register,
                    action: VimAction::Operate(operator, OperatorTarget::TextObject(object)),
                });
            }
            // i/a 后的对象不认识：比如 `dia` 里的 `a` 不是对象也不是动作 → 无效。
            return ParseOutcome::Invalid;
        }
        if next == VimKey::Char('g') {
            match rest.get(1) {
                Some(VimKey::Char('n')) | Some(VimKey::Char('N')) => {
                    return ParseOutcome::Execute(VimCommand {
                        count: 0,
                        register,
                        action: VimAction::Operate(
                            operator,
                            OperatorTarget::SearchMatch {
                                reverse: rest.get(1) == Some(&VimKey::Char('N')),
                            },
                        ),
                    });
                }
                _ => {}
            }
        }
        let (motion, _) = match Self::vim_parse_motion_sequence(rest, total, explicit_count) {
            Ok(parsed) => parsed,
            Err(outcome) => return outcome,
        };
        ParseOutcome::Execute(VimCommand {
            count: 0,
            register,
            action: VimAction::Operate(operator, OperatorTarget::Motion(motion, total)),
        })
    }

    /// 解析一个动作键序列（`w`、`f{char}`、`gg`、`G` 等），返回动作与消耗
    /// 的键数。`total` 是已合并的计数；`explicit_count` 区分 `G`/`gg` 与
    /// `{count}G`/`{count}gg`。
    fn vim_parse_motion_sequence(
        keys: &[VimKey],
        total: u32,
        explicit_count: bool,
    ) -> Result<(Motion, usize), ParseOutcome> {
        let Some(head) = keys.first().copied() else {
            return Err(ParseOutcome::NeedMore);
        };
        let parsed = match head {
            VimKey::Char('w') => (Motion::WordForward { big: false }, 1),
            VimKey::Char('W') => (Motion::WordForward { big: true }, 1),
            VimKey::Char('b') => (Motion::WordBackward { big: false }, 1),
            VimKey::Char('B') => (Motion::WordBackward { big: true }, 1),
            VimKey::Char('e') => (Motion::WordEnd { big: false }, 1),
            VimKey::Char('E') => (Motion::WordEnd { big: true }, 1),
            VimKey::Char('h') | VimKey::Left => (Motion::Left, 1),
            VimKey::Char('l') | VimKey::Right | VimKey::Char(' ') => (Motion::Right, 1),
            VimKey::Char('j') | VimKey::Down => (Motion::Down, 1),
            VimKey::Char('k') | VimKey::Up => (Motion::Up, 1),
            VimKey::Char('0') | VimKey::Home => (Motion::HardLineStart, 1),
            VimKey::Char('$') | VimKey::End => (Motion::HardLineEnd, 1),
            VimKey::Char('^') => (Motion::FirstNonBlank, 1),
            VimKey::Char('_') => (Motion::LastNonBlank, 1),
            VimKey::Char('G') => (
                if explicit_count {
                    Motion::GoToLine(total)
                } else {
                    Motion::DocEnd
                },
                1,
            ),
            VimKey::Char('{') => (Motion::ParagraphBackward, 1),
            VimKey::Char('}') => (Motion::ParagraphForward, 1),
            VimKey::Char('(') => (Motion::SentenceBackward, 1),
            VimKey::Char(')') => (Motion::SentenceForward, 1),
            VimKey::Char('%') => (Motion::MatchPair, 1),
            VimKey::Char('|') => (Motion::ColumnNo(total), 1),
            VimKey::Char('g') => {
                return match keys.get(1) {
                    Some(VimKey::Char('g')) => Ok((
                        if explicit_count {
                            Motion::GoToLine(total)
                        } else {
                            Motion::DocStart
                        },
                        2,
                    )),
                    Some(VimKey::Char('_')) => Ok((Motion::LastNonBlank, 2)),
                    Some(_) => Err(ParseOutcome::Invalid),
                    None => Err(ParseOutcome::NeedMore),
                };
            }
            VimKey::Char('f') | VimKey::Char('F')
            | VimKey::Char('t') | VimKey::Char('T') => {
                let Some(ch) = keys.get(1).and_then(|key| key.as_char()) else {
                    return Err(ParseOutcome::NeedMore);
                };
                (
                    Motion::FindChar {
                        ch,
                        reverse: matches!(head, VimKey::Char('F') | VimKey::Char('T')),
                        till: matches!(head, VimKey::Char('t') | VimKey::Char('T')),
                    },
                    2,
                )
            }
            _ => return Err(ParseOutcome::Invalid),
        };
        Ok(parsed)
    }

    /// `ys{motion|textobject|s}{delim}`：解析环绕目标与定界符。
    fn vim_parse_surround_add(keys: &[VimKey], count: u32) -> ParseOutcome {
        let Some(head) = keys.first().copied() else {
            return ParseOutcome::NeedMore;
        };
        let (target, consumed) = match head {
            VimKey::Char('s') => (OperatorTarget::CurrentLines, 1),
            VimKey::Char('i') | VimKey::Char('a') => {
                let Some(object_key) = keys.get(1).and_then(|key| key.as_char()) else {
                    return ParseOutcome::NeedMore;
                };
                let Some(object) =
                    TextObject::parse(head == VimKey::Char('i'), Some(object_key))
                else {
                    return ParseOutcome::Invalid;
                };
                (OperatorTarget::TextObject(object), 2)
            }
            _ => {
                let (motion, consumed) =
                    match Self::vim_parse_motion_sequence(keys, count.max(1), count > 0) {
                        Ok(parsed) => parsed,
                        Err(outcome) => return outcome,
                    };
                (OperatorTarget::Motion(motion, count.max(1)), consumed)
            }
        };
        let Some(delimiter) = keys.get(consumed).and_then(|key| key.as_char()) else {
            return ParseOutcome::NeedMore;
        };
        let Some(pair) = SurroundPair::from_delimiter(delimiter) else {
            return ParseOutcome::Invalid;
        };
        ParseOutcome::Execute(VimCommand {
            count: 0,
            register: None,
            action: VimAction::SurroundAdd { target, pair },
        })
    }

    fn vim_parse_surround_delete(keys: &[VimKey]) -> ParseOutcome {
        let Some(delimiter) = keys.first().and_then(|key| key.as_char()) else {
            return ParseOutcome::NeedMore;
        };
        let Some(pair) = SurroundPair::from_delimiter(delimiter) else {
            return ParseOutcome::Invalid;
        };
        ParseOutcome::Execute(VimCommand {
            count: 0,
            register: None,
            action: VimAction::SurroundDelete(pair),
        })
    }

    fn vim_parse_surround_change(keys: &[VimKey]) -> ParseOutcome {
        let Some(old_key) = keys.first().and_then(|key| key.as_char()) else {
            return ParseOutcome::NeedMore;
        };
        let Some(new_key) = keys.get(1).and_then(|key| key.as_char()) else {
            return ParseOutcome::NeedMore;
        };
        let Some(old) = SurroundPair::from_delimiter(old_key) else {
            return ParseOutcome::Invalid;
        };
        let Some(new) = SurroundPair::from_delimiter(new_key) else {
            return ParseOutcome::Invalid;
        };
        ParseOutcome::Execute(VimCommand {
            count: 0,
            register: None,
            action: VimAction::SurroundChange { old, new },
        })
    }

    // ---- 序列解析（可视模式）----
    fn vim_parse_visual_sequence(&self, keys: &[VimKey]) -> ParseOutcome {
        let (count, index) = Self::vim_parse_count(keys, 0);
        let (register, index) = Self::vim_parse_register(keys, index);
        if index >= keys.len() {
            return ParseOutcome::NeedMore;
        }
        let head = keys[index];
        let rest = &keys[index + 1..];
        let kind = match self.vim.mode {
            VimMode::Visual(kind) => kind,
            _ => VisualKind::Char,
        };

        let action = match head {
            VimKey::Char('v') => {
                if kind == VisualKind::Char {
                    VimAction::ExitVisual
                } else {
                    VimAction::ToggleVisual(VisualKind::Char)
                }
            }
            VimKey::Char('V') => {
                if kind == VisualKind::Line {
                    VimAction::ExitVisual
                } else {
                    VimAction::ToggleVisual(VisualKind::Line)
                }
            }
            VimKey::Ctrl('q') | VimKey::Ctrl('v') => {
                if kind == VisualKind::Block {
                    VimAction::ExitVisual
                } else {
                    VimAction::ToggleVisual(VisualKind::Block)
                }
            }
            VimKey::Esc => VimAction::EnterNormalMode,
            VimKey::Char('o') => VimAction::ExchangeVisualAnchor,
            VimKey::Char('O') => VimAction::SwapVisualBlockColumn,
            VimKey::Char('d') | VimKey::Char('x') | VimKey::Delete => {
                VimAction::Operate(Operator::Delete, OperatorTarget::Selection)
            }
            VimKey::Char('D') | VimKey::Char('X') => {
                VimAction::Operate(Operator::Delete, OperatorTarget::CurrentLines)
            }
            VimKey::Char('y') => VimAction::Operate(Operator::Yank, OperatorTarget::Selection),
            VimKey::Char('c') | VimKey::Char('s') => {
                VimAction::Operate(Operator::Change, OperatorTarget::Selection)
            }
            VimKey::Char('C') => {
                VimAction::Operate(Operator::Change, OperatorTarget::CurrentLines)
            }
            VimKey::Char('S') => {
                let Some(delimiter) = rest.first().and_then(|key| key.as_char()) else {
                    return ParseOutcome::NeedMore;
                };
                let Some(pair) = SurroundPair::from_delimiter(delimiter) else {
                    return ParseOutcome::Invalid;
                };
                VimAction::VisualSurround(pair)
            }
            VimKey::Char('u') => VimAction::Operate(Operator::Lowercase, OperatorTarget::Selection),
            VimKey::Char('U') => VimAction::Operate(Operator::Uppercase, OperatorTarget::Selection),
            VimKey::Char('~') => VimAction::Operate(Operator::SwapCase, OperatorTarget::Selection),
            VimKey::Char('r') => {
                let Some(ch) = rest.first().and_then(|key| key.as_char()) else {
                    return ParseOutcome::NeedMore;
                };
                VimAction::VisualReplaceChar(ch)
            }
            VimKey::Char('J') => VimAction::JoinLines { count: vim_join_line_count(count), keep_space: true },
            VimKey::Char('g') => {
                let Some(next) = rest.first() else {
                    return ParseOutcome::NeedMore;
                };
                return match next {
                    VimKey::Char('J') => ParseOutcome::Execute(VimCommand {
                        count,
                        register,
                        action: VimAction::JoinLines {
                            count: vim_join_line_count(count),
                            keep_space: false,
                        },
                    }),
                    VimKey::Char('c') => ParseOutcome::Execute(VimCommand {
                        count,
                        register,
                        action: VimAction::Operate(
                            Operator::ToggleComment,
                            OperatorTarget::Selection,
                        ),
                    }),
                    _ => ParseOutcome::Invalid,
                };
            }
            VimKey::Char('>') => VimAction::Operate(Operator::Indent, OperatorTarget::Selection),
            VimKey::Char('<') => VimAction::Operate(Operator::Outdent, OperatorTarget::Selection),
            VimKey::Char('p') | VimKey::Char('P') => VimAction::Paste { after: true },
            VimKey::Char('I') => VimAction::EnterInsert(InsertPlacement::LineStart),
            VimKey::Char('A') => VimAction::EnterInsert(InsertPlacement::LineEnd),
            VimKey::Char('i') | VimKey::Char('a') => {
                if rest.is_empty() {
                    return ParseOutcome::NeedMore;
                }
                let Some(object) =
                    TextObject::parse(head == VimKey::Char('i'), rest[0].as_char())
                else {
                    return ParseOutcome::Invalid;
                };
                VimAction::VisualObject(object)
            }
            VimKey::Char(':') => VimAction::OpenCmdline(command::CmdlineKind::VisualCommand),
            _ => return Self::vim_motion_action(head, count),
        };
        ParseOutcome::Execute(VimCommand { count, register, action })
    }

    // ---- 计数与寄存器前缀 ----

    /// 「操作符 + 计数 + 同一个操作符」是行级：`d2d`、`y3y`、`>2>`。
    /// 返回（行数 = 外层计数与内层计数的较大者，消费到的位置）。
    fn vim_linewise_repeat_count(head: VimKey, rest: &[VimKey], count: u32) -> Option<u32> {
        let (extra, next) = Self::vim_parse_count(rest, 0);
        if next == 0 {
            return None;
        }
        (rest.get(next) == Some(&head)).then(|| extra.max(1).max(count.max(1)))
    }

    fn vim_parse_count(keys: &[VimKey], mut index: usize) -> (u32, usize) {
        // 前导 0 是「行首」动作，不算计数。
        if matches!(keys.get(index), Some(VimKey::Char('0'))) {
            return (0, index);
        }
        let mut value: u32 = 0;
        let mut digits = 0;
        while let Some(VimKey::Char(ch)) = keys.get(index) {
            if let Some(digit) = ch.to_digit(10) {
                value = value.saturating_mul(10).saturating_add(digit).min(9999);
                index += 1;
                digits += 1;
            } else {
                break;
            }
        }
        if digits == 0 {
            return (0, index);
        }
        (value, index)
    }

    fn vim_parse_register(keys: &[VimKey], mut index: usize) -> (Option<char>, usize) {
        if !matches!(keys.get(index), Some(VimKey::Char('"'))) {
            return (None, index);
        }
        index += 1;
        match keys.get(index).and_then(|key| key.as_register_name()) {
            Some(register) => (Some(register), index + 1),
            None => (None, index),
        }
    }
}
