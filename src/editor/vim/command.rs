//! vim 命令行：`:` 命令与 `/` 搜索浮层，及 n/N/*/#/:nohl/:s 的执行。
//!
//! 浮层复用命令面板的输入组件（gpui-kit `InputState`），Enter/Esc 由外层
//! 容器（overlays 渲染处）捕获。搜索走应用自己的文档命中表（`workspace`
//! 的搜索状态 + `find_next_document_match`），高亮与跳转因此与 ⌘F 同源。
//! `:s` 按 Rust regex 语法解释模式（vim 的 `\(` 反向分组建议直接写 `(`
//! 分组），范围替换一次算完后经 `write_back_cross_block_source_edit` 落笔。

use gpui::*;

use gpui_kit::component::{ActiveTheme as _, input::Input};

use crate::editor::Editor;

use super::VimSearchState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmdlineKind {
    /// `:` 普通命令。
    Command,
    /// 可视模式进入的 `:`（预填 `'<,'>`）。
    VisualCommand,
    /// `/` 正向、`?` 反向搜索。
    Search { reverse: bool },
}

/// 命令行浮层状态。
#[derive(Clone)]
pub(crate) struct VimCommandLine {
    pub kind: CmdlineKind,
    pub input: Entity<gpui_kit::component::input::InputState>,
}

impl Editor {
    /// 打开命令行（`:` / `/`）。
    pub(super) fn vim_open_cmdline_inner(
        &mut self,
        kind: CmdlineKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let mut state = gpui_kit::component::input::InputState::new(window, cx);
            let prefill = match kind {
                CmdlineKind::Command => ":",
                CmdlineKind::VisualCommand => ":'<,'>",
                CmdlineKind::Search { reverse } => if reverse { "?" } else { "/" },
            };
            state.set_value(prefill.to_string(), window, cx);
            state
        });
        let focus = input.read(cx).focus_handle(cx);
        self.vim.cmdline = Some(VimCommandLine { kind, input });
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn vim_close_cmdline_inner(&mut self, cx: &mut Context<Self>) {
        if self.vim.cmdline.take().is_some() {
            self.restore_focus_after_overlay(cx);
            cx.notify();
        }
    }

    /// Enter：提交命令行。
    pub(super) fn vim_commit_cmdline_inner(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.vim.cmdline.clone() else { return };
        let raw = state.input.read(cx).value();
        self.vim_close_cmdline_inner(cx);
        match state.kind {
            CmdlineKind::Search { reverse } => {
                let query = raw.trim_start_matches(['/', '?']).trim().to_string();
                self.vim_execute_search(&query, reverse, cx);
            }
            CmdlineKind::Command | CmdlineKind::VisualCommand => {
                self.vim_execute_command(&raw, window, cx);
            }
        }
    }

    fn vim_execute_search(&mut self, query: &str, reverse: bool, cx: &mut Context<Self>) {
        if query.is_empty() {
            return;
        }
        self.vim.last_search = Some(VimSearchState { query: query.to_string(), reverse });
        self.vim.hlsearch = true;
        self.vim_apply_search_query(&query, cx);
        self.find_next_document_match(reverse, cx);
    }

    fn vim_apply_search_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if self.workspace.search_query != query {
            self.workspace.search_query = query.to_string();
            self.workspace.search_scope =
                crate::editor::workspace::WorkspaceSearchScope::Document;
        }
        self.schedule_workspace_search(cx);
    }

    pub(super) fn vim_search_next_inner(&mut self, opposite: bool, cx: &mut Context<Self>) {
        let Some(search) = self.vim.last_search.clone() else {
            return;
        };
        self.vim_apply_search_query(&search.query, cx);
        let reverse = search.reverse != opposite;
        self.find_next_document_match(reverse, cx);
    }

    pub(super) fn vim_search_word_inner(
        &mut self,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entities = self.vim_navigation_entities(cx);
        let Some(cursor) = self.vim_current_position(&entities, cx) else { return };
        let text = entities
            .get(cursor.block_index)
            .map(|entity| entity.read(cx).display_text().to_string())
            .unwrap_or_default();
        // 光标所在词（vim 词类）：向两侧扩展。
        let class = |offset: usize| -> u8 {
            let ch = text[offset..].chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                0
            } else if ch.is_alphanumeric() || ch == '_' {
                1
            } else {
                2
            }
        };
        if cursor.offset >= text.len() {
            return;
        }
        let current_class = class(cursor.offset);
        if current_class == 0 {
            return;
        }
        let mut start = cursor.offset;
        while start > 0 && text[..start].chars().next_back().is_some_and(|ch| {
            let c = if ch.is_whitespace() { 0 } else if ch.is_alphanumeric() || ch == '_' { 1 } else { 2 };
            c == current_class
        }) {
            start -= text[..start].chars().next_back().map_or(1, |ch| ch.len_utf8());
        }
        let mut end = cursor.offset;
        while end < text.len() {
            let ch = text[end..].chars().next().unwrap_or(' ');
            let c = if ch.is_whitespace() { 0 } else if ch.is_alphanumeric() || ch == '_' { 1 } else { 2 };
            if c != current_class {
                break;
            }
            end += ch.len_utf8();
        }
        let word = text[start..end].to_string();
        if word.is_empty() {
            return;
        }
        let query: String = word
            .chars()
            .flat_map(|ch| {
                if ch.is_alphanumeric() || ch == '_' {
                    vec![ch]
                } else {
                    vec!['\\', ch]
                }
            })
            .collect();
        self.vim_execute_search(&query, reverse, cx);
        // `/` 的首次命中就是第一处；`*`/`#` 要从光标所在词之后算起。命中表导航
        // 以活动区间为准（首次为空 → 第一处），再前进一格即跳过当前词。
        self.find_next_document_match(reverse, cx);
        let _ = window;
    }

    pub(super) fn vim_clear_search_highlight_inner(&mut self, cx: &mut Context<Self>) {
        self.vim.hlsearch = false;
        self.workspace.document_active_range = None;
        self.workspace.document_active_index = None;
        self.sync_document_search_highlights(cx);
        cx.notify();
    }

    // ---- `:` 命令 ----

    fn vim_execute_command(&mut self, raw: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut line = raw.trim().to_string();
        if line.starts_with(':') {
            line.remove(0);
        }
        if line.starts_with("'<,'>") || line.starts_with("'<, '>") {
            line = line.trim_start_matches("'<,'>").trim_start_matches("'<, '>").to_string();
            // 可视范围由当前可视选区决定（若无则退回当前行）。
        }
        let line = line.trim().to_string();
        if line.is_empty() {
            return;
        }
        // 拆范围与命令体。
        let (range, body) = Self::vim_split_cmdline_range(&line);
        let body = body.trim();
        let command = body.split_whitespace().next().unwrap_or("");
        match command {
            "w" | "write" => self.request_save_document(cx),
            "wq" | "x" => {
                self.request_save_document(cx);
                self.vim_close_current_tab(false, window, cx);
            }
            "q" | "quit" => self.vim_close_current_tab(false, window, cx),
            "q!" | "quit!" => self.vim_close_current_tab(true, window, cx),
            "nohl" | "noh" | "nohlsearch" => self.vim_clear_search_highlight_inner(cx),
            body if body.starts_with('s') => {
                self.vim_execute_substitute(range, &body[1..], window, cx);
            }
            _ => {}
        }
    }

    /// 范围前缀（% / N / N,M / . / $）→ (None|Some((start_line, end_line)), 命令体)。
    fn vim_split_cmdline_range(line: &str) -> (Option<(usize, usize)>, &str) {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('%') {
            return (Some((0, usize::MAX)), rest);
        }
        // 找到第一个不是数字/./$/,/空格 的字符处切分。
        let bytes = trimmed.as_bytes();
        let mut split = 0usize;
        let mut seen_digit = false;
        while split < bytes.len() {
            let ch = trimmed[split..].chars().next().unwrap_or(' ');
            if ch.is_ascii_digit() {
                seen_digit = true;
                split += 1;
            } else if (ch == '.' || ch == '$' || ch == ',') && (seen_digit || ch != '.') {
                split += 1;
            } else if ch == '.' && !seen_digit && split == 0 {
                split += 1;
            } else {
                break;
            }
        }
        if split == 0 {
            return (None, trimmed);
        }
        let range_text = trimmed[..split].trim();
        let rest = trimmed[split..].trim_start();
        let parse_bound = |text: &str, last_line: usize| -> Option<usize> {
            let text = text.trim();
            if text.is_empty() || text == "." {
                return None;
            }
            if text == "$" {
                return Some(last_line);
            }
            text.parse::<usize>().ok().map(|line| line.saturating_sub(1))
        };
        // 无逗号时整体是一个界（1,N 支持「,命令」前省略）；这里只支持显式数字。
        if let Some((left, right)) = range_text.split_once(',') {
            // 解析需要行数上限，先返回文本，调用方补齐。
            let parsed = (
                parse_bound(left, usize::MAX).or(Some(0)),
                parse_bound(right, usize::MAX),
            );
            // 占位：真正钳制在 substitute 执行时做。
            return (Some((parsed.0.unwrap_or(0), parsed.1.unwrap_or(0))), rest);
        }
        if let Some(line_no) = parse_bound(range_text, usize::MAX) {
            return (Some((line_no, line_no)), rest);
        }
        (None, trimmed)
    }

    fn vim_close_current_tab(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        if force {
            // q!：直接按干净文档处理，关闭即丢弃。
            self.document_dirty = false;
        }
        let Some(path) = self.file_path.clone() else {
            self.request_close_current_window(window, cx);
            return;
        };
        self.close_workspace_tabs(&[path], window, cx);
    }

    /// `:s/{pat}/{rep}/{flags}`：范围内正则替换，一次成型后走缓冲区写回。
    fn vim_execute_substitute(
        &mut self,
        range: Option<(usize, usize)>,
        body: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(delimiter) = body.chars().next() else { return };
        if delimiter.is_alphanumeric() || delimiter == ' ' {
            return;
        }
        let parts: Vec<&str> = split_escaped(&body[delimiter.len_utf8()..], delimiter);
        let pattern = parts.first().copied().unwrap_or("");
        let replacement = parts.get(1).copied().unwrap_or("");
        let flags = parts.get(2).copied().unwrap_or("");
        let global = flags.contains('g');
        let case_insensitive = flags.contains('i');
        let Ok(regex) = regex::RegexBuilder::new(pattern)
            .case_insensitive(case_insensitive)
            .build()
        else {
            return;
        };
        // vim 的 \1 反向引用翻译成 Rust 的 ${1}。
        let replacement = replacement.replace("\\1", "${1}").replace("\\2", "${2}").replace("\\0", "${0}");

        // 范围 → 缓冲区字节区间。
        let snapshot = self.capture_source_selection_snapshot(cx);
        let buffer_range = match range {
            Some((start_line, end_line)) => {
                let last = self.buffer.line_count().saturating_sub(1);
                let start_line = start_line.min(last);
                let end_line = end_line.max(start_line).min(last);
                let start = self.buffer.line_start(start_line);
                let end = self
                    .buffer
                    .line_range(end_line)
                    .end
                    .min(self.buffer.byte_len());
                start..end
            }
            None => {
                // 无范围 = 当前行（不含行尾换行），不是光标处那个空选区。
                let offset = snapshot.range.start.min(self.buffer.byte_len());
                let line = self.buffer.line_of(offset);
                let line_range = self.buffer.line_range(line);
                let line_text = self.buffer.slice(line_range.clone());
                let length = line_text.trim_end_matches(['\n', '\r']).len();
                line_range.start..line_range.start + length
            }
        };
        if buffer_range.is_empty() && range.is_none() {
            return;
        }
        let source = self.current_document_source(cx);
        let Some(segment) = source.get(buffer_range.clone()) else {
            return;
        };
        let mut replaced = String::with_capacity(segment.len());
        let mut last_match = 0usize;
        let mut count = 0usize;
        for matches in regex.find_iter(segment) {
            if !global && count >= 1 {
                break;
            }
            replaced.push_str(&segment[last_match..matches.start()]);
            let matched = &segment[matches.start()..matches.end()];
            let mut expanded = String::new();
            match regex.captures(matched) {
                Some(captures) => captures.expand(&replacement, &mut expanded),
                None => expanded.push_str(&replacement),
            }
            replaced.push_str(&expanded);
            last_match = matches.end();
            count += 1;
            if matches.end() == matches.start() {
                // 空匹配防死循环：前移一个字符。
                if let Some(next) = segment[last_match..].chars().next() {
                    let len = next.len_utf8();
                    replaced.push_str(&segment[last_match..last_match + len]);
                    last_match += len;
                }
            }
        }
        replaced.push_str(&segment[last_match..]);
        if count == 0 {
            return;
        }
        self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        self.write_back_cross_block_source_edit(buffer_range.clone(), &replaced, cx);
        self.finalize_pending_undo_capture(cx);
        // 光标落回替换段首。
        self.apply_selection_snapshot_in_current_mode(
            &crate::editor::UndoSelectionSnapshot {
                range: buffer_range.start..buffer_range.start,
                reversed: false,
            },
            cx,
        );
        cx.notify();
        let _ = window;
    }
}

/// 按分隔符切分，反斜杠转义的分隔符不算。
fn split_escaped(text: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == delimiter {
            parts.push(&text[start..index]);
            start = index + ch.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

// ---- 模块门面（vim.rs 执行器调用）----

/// 浮层需要 Window 构造输入框：执行器只挂 pending 标记，render 里落地
/// （与公式编辑器同一套「pending + apply」惯例）。
pub(super) fn vim_request_cmdline(editor: &mut Editor, kind: CmdlineKind, cx: &mut Context<Editor>) {
    editor.vim.pending_cmdline = Some(kind);
    cx.notify();
}

/// 命令行浮层（`: ` / `/`）：底部一条输入栏，与 vim 的命令行同位。
/// Enter/Esc 由按键拦截层截获（见 `vim_key_down` 的 cmdline 分支），输入
/// 框自己只管文本。
pub(crate) fn render_vim_cmdline_overlay(
    editor: &mut Editor,
    _window: &mut Window,
    cx: &mut Context<Editor>,
) -> AnyElement {
    let Some(state) = editor.vim.cmdline.clone() else {
        return div().into_any_element();
    };
    let input = state.input.clone();
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .w_full()
        .flex()
        .items_center()
        .px_4()
        .h(px(36.0))
        .bg(cx.theme().background)
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            Input::new(&input)
                .id("vim-cmdline")
                .appearance(false)
                .p_0(),
        )
        .into_any_element()
}

/// render 期落地命令行浮层。
pub(crate) fn apply_pending_vim_cmdline(
    editor: &mut Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let Some(kind) = editor.vim.pending_cmdline.take() else {
        return;
    };
    editor.vim_open_cmdline_inner(kind, window, cx);
}

pub(super) fn vim_search_word(
    editor: &mut Editor,
    reverse: bool,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_search_word_inner(reverse, window, cx);
}

pub(super) fn vim_search_next(
    editor: &mut Editor,
    reverse: bool,
    cx: &mut Context<Editor>,
) {
    editor.vim_search_next_inner(reverse, cx);
}

pub(super) fn vim_save_and_close(
    editor: &mut Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.request_save_document(cx);
    editor.vim_close_current_tab(false, window, cx);
}

pub(super) fn vim_close_discarding(
    editor: &mut Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    editor.vim_close_current_tab(true, window, cx);
}

/// 浮层按键：Enter 提交、Esc 关闭。返回 true 表示已消费。
pub(super) fn vim_cmdline_key_down(
    editor: &mut Editor,
    keystroke: &gpui::Keystroke,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> bool {
    if editor.vim.cmdline.is_none() {
        return false;
    }
    match keystroke.key.as_str() {
        "enter" => {
            editor.vim_commit_cmdline_inner(window, cx);
            true
        }
        "escape" => {
            editor.vim_close_cmdline_inner(cx);
            true
        }
        _ => false,
    }
}
