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

/// `.` 重放的一条变更记录：进入插入前的命令键 + 插入会话里敲过的键。
#[derive(Clone, Debug, Default)]
pub(super) struct DotRecord {
    pub entry_keys: Vec<VimKey>,
    pub insert_keys: Vec<VimKey>,
}

#[derive(Clone, Debug)]
pub(super) struct VimSearchState {
    pub query: String,
    pub reverse: bool,
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
    VisualObject(TextObject),
    /// 可视模式 r{char}：整段选区换成同一字符。
    VisualReplaceChar(char),
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
    SearchWord { reverse: bool },
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
        if self.vim_focused_block(window, cx).is_none() {
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
                || block.code_language_focus_handle.is_focused(window)
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

    /// 把当前模式的光标形状（块/竖条）广播到可见块。
    pub(super) fn sync_vim_block_carets(&mut self, cx: &mut Context<Self>) {
        if !self.vim_enabled {
            return;
        }
        let block_caret = self.vim.mode.uses_block_caret();
        let visible = self.document.visible_blocks().to_vec();
        for entry in &visible {
            let entity = &entry.entity;
            if entity.read(cx).vim_block_caret != block_caret {
                entity.update(cx, |block, cx| {
                    block.vim_block_caret = block_caret;
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
            VimKey::Esc | VimKey::Ctrl('[') | VimKey::Ctrl('c') => {
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
                    self.vim_modal_key_inner(key, window, cx);
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
            VimKey::Esc | VimKey::Ctrl('[') | VimKey::Ctrl('c') => {
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

        self.vim.pending.push(key);
        // 护栏：最长的合法序列（"a12d3fo）也远到不了 10 键。
        if self.vim.pending.len() > 10 {
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
        let mut best: Option<(usize, VimKeymapAction)> = None;
        for entry in &preferences.keymaps {
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
                    _ => best = Some((mapped.len(), entry.action)),
                }
            }
        }
        match best {
            Some((length, action)) if length == sequence.len() => MappingMatch::Fire(action),
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
        };
        self.vim_execute(
            VimCommand { count: 0, register: None, action: vim_action },
            &[],
            window,
            cx,
        );
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
                | Operator::SwapCase, _)
                | VimAction::ReplaceChar(_)
                | VimAction::VisualReplaceChar(_)
                | VimAction::Paste { .. }
                | VimAction::JoinLines { .. }
                | VimAction::Increment(_)
                | VimAction::SwapCaseAtCursor
                | VimAction::EnterInsert(_)
        );
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
            }
            VimAction::VisualObject(object) => {
                visual::vim_extend_to_object(self, object, window, cx);
            }
            VimAction::VisualReplaceChar(ch) => {
                operate::vim_visual_replace_char(self, ch, window, cx);
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
            VimAction::Paste { after } => operate::vim_paste(self, register, after, count, window, cx),
            VimAction::JoinLines { count: join_count, keep_space } => {
                operate::vim_join(self, join_count.max(1), keep_space, window, cx);
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
            VimAction::SearchWord { reverse } => command::vim_search_word(self, reverse, window, cx),
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

        // 修改操作符把插入会话并进同一组，等 Esc 收尾时再落账。
        if capture_undo
            && !matches!(action, VimAction::Operate(Operator::Change, _))
        {
            self.finalize_pending_undo_capture(cx);
        }

        // `.` 素材：变更命令记下入口键（插入会话的键在 esc 时补记）。
        if is_change && !self.vim.replaying && !entry_keys.is_empty() {
            self.vim.dot = Some(DotRecord {
                entry_keys: entry_keys.to_vec(),
                insert_keys: Vec::new(),
            });
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
        self.vim.last_macro_register = Some(register_name);
        let times = count.max(1);
        let mut guard_counter = 0usize;
        self.vim.replaying = true;
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
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    /// `.` 与 `3o` 的重放：入口命令 → 插入会话 → 退出插入。
    pub(super) fn vim_replay_record(&mut self, record: &DotRecord, window: &mut Window, cx: &mut Context<Self>) {
        self.vim.replaying = true;
        for key in &record.entry_keys {
            self.vim_route_key(*key, window, cx);
        }
        for key in &record.insert_keys {
            self.vim_route_key(*key, window, cx);
        }
        if self.vim.mode == VimMode::Insert {
            self.vim_route_key(VimKey::Esc, window, cx);
        }
        self.vim.replaying = false;
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
                return Self::vim_parse_operator_target(operator, rest, count, register);
            }
            VimKey::Char('g') => return Self::vim_parse_g_sequence(rest, count, register),
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
                count: count.max(1),
                keep_space: true,
            },
            VimKey::Char('u') => VimAction::Undo,
            VimKey::Char('U') => VimAction::Redo,
            VimKey::Ctrl('r') => VimAction::Redo,
            VimKey::Char('.') => VimAction::Dot,
            VimKey::Char(';') => VimAction::RepeatFind { reverse: false },
            VimKey::Char(',') => VimAction::RepeatFind { reverse: true },
            VimKey::Char('/') => VimAction::Search { reverse: false },
            VimKey::Char('?') => VimAction::Search { reverse: true },
            VimKey::Char('n') => VimAction::SearchNext { reverse: false },
            VimKey::Char('N') => VimAction::SearchNext { reverse: true },
            VimKey::Char('*') => VimAction::SearchWord { reverse: false },
            VimKey::Char('#') => VimAction::SearchWord { reverse: true },
            VimKey::Char(':') => VimAction::OpenCmdline(command::CmdlineKind::Command),
            VimKey::Char('%') => VimAction::Move(Motion::MatchPair),
            VimKey::Char('|') => VimAction::Move(Motion::ColumnNo(count.max(1))),
            VimKey::Esc | VimKey::Ctrl('[') => VimAction::EnterNormalMode,
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
            VimKey::Char('i') => VimAction::EnterInsert(InsertPlacement::LastInsertPosition),
            VimKey::Char('I') => VimAction::EnterInsert(InsertPlacement::FirstColumn),
            VimKey::Char('J') => VimAction::JoinLines { count: 2, keep_space: false },
            VimKey::Char('v') => VimAction::ReselectVisual,
            VimKey::Char('e') => VimAction::Move(Motion::WordEndBackward { big: false }),
            VimKey::Char('E') => VimAction::Move(Motion::WordEndBackward { big: true }),
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
        let motion = match next {
            VimKey::Char('w') => Motion::WordForward { big: false },
            VimKey::Char('W') => Motion::WordForward { big: true },
            VimKey::Char('b') => Motion::WordBackward { big: false },
            VimKey::Char('B') => Motion::WordBackward { big: true },
            VimKey::Char('e') => Motion::WordEnd { big: false },
            VimKey::Char('E') => Motion::WordEnd { big: true },
            VimKey::Char('h') | VimKey::Left => Motion::Left,
            VimKey::Char('l') | VimKey::Right | VimKey::Char(' ') => Motion::Right,
            VimKey::Char('j') | VimKey::Down => Motion::Down,
            VimKey::Char('k') | VimKey::Up => Motion::Up,
            VimKey::Char('0') | VimKey::Home => Motion::HardLineStart,
            VimKey::Char('$') | VimKey::End => Motion::HardLineEnd,
            VimKey::Char('^') => Motion::FirstNonBlank,
            VimKey::Char('_') => Motion::LastNonBlank,
            VimKey::Char('{') => Motion::ParagraphBackward,
            VimKey::Char('}') => Motion::ParagraphForward,
            VimKey::Char('(') => Motion::SentenceBackward,
            VimKey::Char(')') => Motion::SentenceForward,
            VimKey::Char('%') => Motion::MatchPair,
            VimKey::Char('|') => Motion::ColumnNo(total),
            VimKey::Char('g') => {
                return match rest.get(1) {
                    Some(VimKey::Char('g')) => ParseOutcome::Execute(VimCommand {
                        count: total,
                        register,
                        action: VimAction::Operate(operator, OperatorTarget::CurrentLines),
                    }),
                    Some(VimKey::Char('_')) => ParseOutcome::Execute(VimCommand {
                        count: 0,
                        register,
                        action: VimAction::Operate(
                            operator,
                            OperatorTarget::Motion(Motion::LastNonBlank, 1),
                        ),
                    }),
                    Some(_) => ParseOutcome::Invalid,
                    None => ParseOutcome::NeedMore,
                };
            }
            VimKey::Char('f') | VimKey::Char('F') | VimKey::Char('t') | VimKey::Char('T') => {
                let Some(ch) = rest.get(1).and_then(|key| key.as_char()) else {
                    return ParseOutcome::NeedMore;
                };
                return ParseOutcome::Execute(VimCommand {
                    count: 0,
                    register,
                    action: VimAction::Operate(
                        operator,
                        OperatorTarget::Motion(
                            Motion::FindChar {
                                ch,
                                reverse: matches!(next, VimKey::Char('F') | VimKey::Char('T')),
                                till: matches!(next, VimKey::Char('t') | VimKey::Char('T')),
                            },
                            total,
                        ),
                    ),
                });
            }
            _ => return ParseOutcome::Invalid,
        };
        ParseOutcome::Execute(VimCommand {
            count: 0,
            register,
            action: VimAction::Operate(operator, OperatorTarget::Motion(motion, total)),
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
            VimKey::Ctrl('q') => {
                if kind == VisualKind::Block {
                    VimAction::ExitVisual
                } else {
                    VimAction::ToggleVisual(VisualKind::Block)
                }
            }
            VimKey::Esc | VimKey::Ctrl('[') => VimAction::EnterNormalMode,
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
            VimKey::Char('C') | VimKey::Char('S') => {
                VimAction::Operate(Operator::Change, OperatorTarget::CurrentLines)
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
            VimKey::Char('J') => VimAction::JoinLines { count: 2, keep_space: true },
            VimKey::Char('g') => {
                return match rest.first() {
                    Some(VimKey::Char('J')) => ParseOutcome::Execute(VimCommand {
                        count,
                        register,
                        action: VimAction::JoinLines { count: 2, keep_space: false },
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
