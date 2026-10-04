//! 文档文本缓冲区：编辑器里**唯一的内容事实源**。
//!
//! 背景见 `docs/plans/2026-10-02-buffer-as-source-of-truth-refactor.md`：块树是本
//! 结构的投影，每个块持有一段指向这里的字节区间（[`Anchor`]）；保存写的就是这里，
//! 绝不从块树重新生成 markdown——那是「打开不编辑、保存被改写」的根因。
//!
//! 文本按 [`MAX_CHUNK_BYTES`] 分块存放，一次编辑只重建它真正碰到的那几个块，
//! 不在整篇文本上做 memmove，所以按键成本与文档大小近乎无关。

#[cfg(test)]
mod tests;

mod file_shape;

pub(crate) use file_shape::FileShape;

use std::ops::Range;
use std::sync::Arc;

/// 单个文本块的字节上限。10 MiB 文档约 2560 块。
const MAX_CHUNK_BYTES: usize = 4096;

/// 一段连续文本。永不跨字符边界切分（UTF-8 字节序列完整性是硬要求）。
#[derive(Clone)]
struct Chunk {
    text: String,
    /// 本块里每个 `'\n'` 的**块内**字节偏移，升序。行数就是它的长度，
    /// 「这个偏移落在第几行」「这一行从哪儿起」都在这张短表里二分——行号换算
    /// 于是只问索引，一次都不读正文。块上限 [`MAX_CHUNK_BYTES`] = 4096 字节，
    /// 偏移塞得进 `u16`，代价是每行两个字节（10 MiB 那份 58.5 万行 = 1.2MB）。
    newlines: Vec<u16>,
}

impl Chunk {
    fn byte_len(&self) -> usize {
        self.text.len()
    }

    fn line_count(&self) -> usize {
        self.newlines.len()
    }

    fn new(text: String) -> Self {
        let newlines = newline_offsets(&text);
        Self { text, newlines }
    }

    /// 在块内 `local` 处切开：左半留给本块，右半交出去。换行表跟着切，
    /// 不重读任何字节。
    fn split_at(&mut self, local: usize) -> Chunk {
        debug_assert!(
            self.text.is_char_boundary(local),
            "切分点 {local} 落在多字节字符中间"
        );
        let right = self.text.split_off(local);
        let keep = self.newlines.partition_point(|at| (*at as usize) < local);
        let mut right_newlines = self.newlines.split_off(keep);
        for at in &mut right_newlines {
            *at = (*at as usize - local) as u16;
        }
        Self {
            text: right,
            newlines: right_newlines,
        }
    }

    /// 块内这个偏移之前有几个换行（=它落在块内的第几行）。
    fn lines_before(&self, local: usize) -> usize {
        self.newlines.partition_point(|at| (*at as usize) < local)
    }

    /// 块内第 `nth` 个换行符（1 基）的偏移；没有就按「块尾」算。
    fn newline(&self, nth: usize) -> usize {
        match self.newlines.get(nth - 1) {
            Some(at) => *at as usize,
            None => self.byte_len(),
        }
    }

    /// 块内最后一个换行符的偏移。
    fn last_newline(&self) -> Option<usize> {
        self.newlines.last().map(|at| *at as usize)
    }
}

/// 指向缓冲区内某个位置的锚点：随编辑自动平移，本身只是个槽位编号。
///
/// 块树的 span 全用它表示，所以「第 k 块的起止字节」不需要任何反推换算：
/// 编辑落在别处时它自己跟着移动，编辑落在本块时由该块重新取锚。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Anchor {
    slot: usize,
}

/// 一次已经落地的编辑，自带逆操作。撤销栈只要把这些逆操作按反序重放，
/// 不需要再存任何全文快照。
#[derive(Clone, Debug)]
pub(crate) struct AppliedEdit {
    /// 被换掉的原文；撤销时放回去的就是它。
    pub(crate) removed: String,
    /// 新文本落地后的字节区间；重做时把这段换回 `removed`。
    pub(crate) new_range: Range<usize>,
}

/// 文档文本。所有编辑最终都落成一次 [`TextBuffer::edit`]。
#[derive(Clone)]
pub(crate) struct TextBuffer {
    chunks: Vec<Chunk>,
    /// 全文字节数。逐块加起来是 O(文本块数)，而 `byte_len` 在按根块走一圈的循环里
    /// 会被调到 O(根块数 × 文本块数)（10 MiB 文档一次按键 6 秒就是这么来的），
    /// 所以它必须是 O(1)：只有 [`edit`](Self::edit) 会动内容，跟着它记就行。
    total_bytes: usize,
    /// 锚点槽位 → 当前绝对字节偏移；`None` 是空槽（可回收）。
    anchors: Vec<Option<usize>>,
    free_anchor_slots: Vec<usize>,
    /// 这个缓冲区对应的文件形状；非文件来源（粘贴片段、恢复快照）为 `None`。
    shape: Option<FileShape>,
    /// 打开时的原始字节。只在整个缓冲区一次编辑都没落过的时候有效——
    /// 那时「保存」可以是把这份字节原样写回去，一个字节都不必重新生成。
    pristine: Option<Arc<[u8]>>,
    /// 自上次取走以来被改过的字节范围（保守：历次编辑的最小起点到最大终点）。
    /// 读侧的增量视图（文档大纲）用它判断「哪些块的字节真的动过」：没动过的块照用
    /// 自己上次的摘要，不必每按一个键就把整篇重扫一遍。
    dirty: Option<Range<usize>>,
    /// 为了建「这块里有几个换行、都落在哪儿」那张索引，读了多少字节正文。只有建块
    /// 与插入新文本会读（按那份文本自己的长度计），**查询行号一律不许读正文**——
    /// 问的是每块那张换行偏移表。以前查询侧每批量换算都要沿块把正文数一遍（实测一个
    /// 196KB 的夹具读 196119 字节 = 整篇一次，10 MiB 一次大纲同步因此约 5ms）；闸门
    /// `asking_for_line_numbers_does_not_read_the_text` 钉住查询读 0 字节，
    /// `rebuilding_the_line_index_costs_only_the_edited_bytes` 钉住维护成本只跟着
    /// 改动走、不跟着文档长。
    line_probe_bytes: std::cell::Cell<usize>,
}

impl TextBuffer {
    /// 以给定文本建一个缓冲区。切分只按字节预算走，内容一字不改。
    ///
    /// 整个内容都算「刚改过的」：派生数据（大纲这类增量视图）于是第一次一定重算，
    /// 不会因为「一个字节都没编辑过」而留着上一份文档的结果。
    pub(crate) fn from_text(text: &str) -> Self {
        let chunks = chunkify(text);
        let probed = text.len();
        Self {
            chunks,
            total_bytes: text.len(),
            anchors: Vec::new(),
            free_anchor_slots: Vec::new(),
            shape: None,
            pristine: None,
            dirty: (!text.is_empty()).then_some(0..text.len()),
            line_probe_bytes: std::cell::Cell::new(probed),
        }
    }

    /// 记下这个缓冲区的文件来源：打开时读到的原始字节 + 由它判定的形状。
    pub(crate) fn set_file_origin(&mut self, raw: Vec<u8>, shape: FileShape) {
        self.shape = Some(shape);
        self.pristine = Some(raw.into());
    }

    /// 一次编辑都没落过时为真；此时 [`file_bytes`](Self::file_bytes) 就是原字节。
    pub(crate) fn is_pristine(&self) -> bool {
        self.pristine.is_some()
    }

    /// 保存要写的字节：未编辑过就是打开时的原始字节，否则按形状重新编码。
    pub(crate) fn file_bytes(&self) -> Vec<u8> {
        match (self.pristine.as_ref(), self.shape) {
            (Some(raw), _) => raw.to_vec(),
            (None, Some(shape)) => shape.encode(&self.text()),
            (None, None) => self.text().into_bytes(),
        }
    }

    /// 与另一个缓冲区内容相同吗？逐块比字节，不复制任何一方。
    pub(crate) fn same_content(&self, other: &Self) -> bool {
        self.byte_len() == other.byte_len()
            && self
                .chunks
                .iter()
                .zip(other.chunks.iter())
                .all(|(mine, theirs)| mine.text == theirs.text)
    }

    /// 内容与给定文本逐字节相同吗？不复制自己，用来跳过「其实没变的整篇重投影」。
    ///
    /// 按字节比，不按字符切片：块边界只对缓冲区自己的字符对齐，比较另一份内容时同样的
    /// 偏移可能落在某个多字节字符中间，`text[a..b]` 会直接 panic。
    pub(crate) fn matches_text(&self, text: &str) -> bool {
        if self.byte_len() != text.len() {
            return false;
        }
        let bytes = text.as_bytes();
        let mut offset = 0usize;
        for chunk in &self.chunks {
            let len = chunk.byte_len();
            if &bytes[offset..offset + len] != chunk.text.as_bytes() {
                return false;
            }
            offset += len;
        }
        true
    }

    /// 缓冲区当前全文。
    pub(crate) fn text(&self) -> String {
        let mut out = String::with_capacity(self.byte_len());
        for chunk in &self.chunks {
            out.push_str(&chunk.text);
        }
        out
    }

    pub(crate) fn byte_len(&self) -> usize {
        self.total_bytes
    }

    /// 取 `range` 指向的字节区间。
    pub(crate) fn slice(&self, range: Range<usize>) -> String {
        let mut out = String::with_capacity(range.end.saturating_sub(range.start));
        let mut base = 0usize;
        for chunk in &self.chunks {
            let end = base + chunk.byte_len();
            if range.start < end && range.end > base {
                let from = range.start.max(base) - base;
                let to = range.end.min(end) - base;
                out.push_str(&chunk.text[from..to]);
            }
            base = end;
        }
        out
    }

    /// 取两个锚点之间的文本——这就是「块 span 的原文」。
    pub(crate) fn slice_span(&self, range: Range<Anchor>) -> String {
        self.slice(self.resolve(range.start)..self.resolve(range.end))
    }

    /// 把当前字节偏移绑成一个跨编辑稳定的锚点。用完要 [`free_anchor`](Self::free_anchor)。
    pub(crate) fn anchor_at(&mut self, offset: usize) -> Anchor {
        let offset = offset.min(self.byte_len());
        match self.free_anchor_slots.pop() {
            Some(slot) => {
                self.anchors[slot] = Some(offset);
                Anchor { slot }
            }
            None => {
                self.anchors.push(Some(offset));
                Anchor {
                    slot: self.anchors.len() - 1,
                }
            }
        }
    }

    /// 释放锚点槽位（块被删除时调用）。释放后再解析该锚点是编程错误。
    pub(crate) fn free_anchor(&mut self, anchor: Anchor) {
        if let Some(slot) = self.anchors.get_mut(anchor.slot) {
            if std::mem::replace(slot, None).is_some() {
                self.free_anchor_slots.push(anchor.slot);
            }
        }
    }

    /// 解析锚点为当前偏移。
    pub(crate) fn resolve(&self, anchor: Anchor) -> usize {
        self.anchors[anchor.slot]
            .expect("解析了一个已释放的锚点")
            .min(self.byte_len())
    }

    /// 行数按 `split('\n')` 计：`"a\n"` 是 2 行（第二行为空），空文档是 1 行。
    /// 与编辑器状态栏、搜索结果的行号口径一致。
    pub(crate) fn line_count(&self) -> usize {
        self.chunks.iter().map(Chunk::line_count).sum::<usize>() + 1
    }

    /// 取走「建/维护换行索引读了多少字节」并清零。查询行号不该让它动一下。
    pub(crate) fn take_line_probe_bytes(&self) -> usize {
        self.line_probe_bytes.replace(0)
    }

    fn probed(&self, bytes: usize) {
        self.line_probe_bytes.set(self.line_probe_bytes.get() + bytes);
    }

    /// 第 `line` 行首个字节的偏移。`line` 越界时返回文末偏移。
    pub(crate) fn line_start(&self, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        let mut base = 0usize;
        let mut seen = 0usize;
        for chunk in &self.chunks {
            if line <= seen + chunk.line_count() {
                return base + chunk.newline(line - seen) + 1;
            }
            seen += chunk.line_count();
            base += chunk.byte_len();
        }
        self.byte_len()
    }

    /// 第 `line` 行（0 基）的字节区间，**不含**行尾换行符。越界钳到文末。
    ///
    /// 读取侧（搜索跳转、大纲点击、行列号）都从这里把行号换成字节，
    /// 于是它们说的坐标天然是文件坐标。
    pub(crate) fn line_range(&self, line: usize) -> std::ops::Range<usize> {
        let start = self.line_start(line);
        let mut end = self.line_start(line + 1).min(self.byte_len());
        if end > start && self.byte_at(end - 1) == Some(b'\n') {
            end -= 1;
        }
        start..end.max(start)
    }

    /// 该偏移处的字节；越界返回 `None`（多字节字符内部的字节照样能取到）。
    pub(crate) fn byte_at(&self, offset: usize) -> Option<u8> {
        let (index, local) = self.locate(offset);
        self.chunks
            .get(index)
            .and_then(|chunk| chunk.text.as_bytes().get(local).copied())
    }

    /// 字节偏移所在行（0 基）。`offset` 等于全文长度时算作末行。
    ///
    /// `offset` 必须是字符边界（搜索命中与光标位置天然是）。
    pub(crate) fn line_of(&self, offset: usize) -> usize {
        let (chunk_index, local) = self.locate(offset);
        let Some(chunk) = self.chunks.get(chunk_index) else {
            return 0;
        };
        debug_assert!(
            chunk.text.is_char_boundary(local),
            "line_of 的偏移落在多字节字符中间：{offset}"
        );
        let preceding = self.chunks[..chunk_index]
            .iter()
            .map(Chunk::line_count)
            .sum::<usize>();
        preceding + chunk.lines_before(local)
    }

    /// 一次遍历把多个**升序**字节偏移的「（所在行, 本行首字节偏移）」一起取回来。
    ///
    /// 单点问 [`line_of`](Self::line_of) 是沿文本块累加换行数的线性活（这里没有行
    /// 索引树），在「逐根块」的循环里调它就变成 O(根块数 × 文本块数)——10 MiB 文档
    /// 一次按键 6 秒就是这么来的。批量问一次只走一遍块。越界的偏移钳到文末。
    ///
    /// 走的是每块那张换行偏移表：一次正文都不读，代价与文档多大无关。
    pub(crate) fn lines_and_line_starts(&self, offsets: &[usize]) -> Vec<(usize, usize)> {
        let mut out = vec![(self.line_count() - 1, self.byte_len()); offsets.len()];
        let mut base = 0usize; // 当前文本块首字节的绝对偏移
        let mut line = 0usize; // 该块首字节所在行
        let mut line_start = 0usize; // 那一行的首字节偏移
        let mut target = 0usize;
        for chunk in &self.chunks {
            let end = base + chunk.byte_len();
            while target < offsets.len() && offsets[target] <= end {
                let local = offsets[target].saturating_sub(base);
                let inside = chunk.lines_before(local);
                let start = if inside == 0 {
                    line_start
                } else {
                    base + chunk.newline(inside) + 1
                };
                out[target] = (line + inside, start);
                target += 1;
            }
            if target >= offsets.len() {
                break;
            }
            if let Some(last) = chunk.last_newline() {
                line_start = base + last + 1;
            }
            line += chunk.line_count();
            base = end;
        }
        out
    }

    /// 把 `range` 指向的字节区间换成 `text`：区间的左邻与右邻字节一字不动。
    ///
    /// 这是全编辑器唯一的文本写入口——块树、撤销、保存都必须经过它，
    /// 这样「未被编辑的字节不会被改写」才是结构性质而不是约定。
    pub(crate) fn edit(&mut self, range: Range<usize>, text: &str) -> AppliedEdit {
        let total = self.byte_len();
        assert!(
            range.start <= range.end && range.end <= total,
            "编辑区间越界：{range:?}，缓冲区长度 {total}"
        );
        debug_assert!(
            self.is_char_boundary(range.start) && self.is_char_boundary(range.end),
            "编辑端点落在多字节字符中间：{range:?}"
        );

        let removed = self.slice(range.clone());
        self.pristine = None;
        let new_range = range.start..range.start + text.len();
        self.dirty = Some(match self.dirty.take() {
            Some(seen) => seen.start.min(new_range.start)..seen.end.max(new_range.end),
            None => new_range.clone(),
        });
        self.shift_anchors(&range, text.len());

        // 只在两个端点处切块，端点之间的整块被移除，新文本单独成分块。
        // 顺序要紧：切分起点会插入一个块，终点必须在切完之后重新算。
        // 切块只是把两张换行表分开（哪一侧都不重读正文），只有新进来的这段文本
        // 要现数一遍换行——所以一次编辑的索引成本是 O(改动大小)，不是 O(文档大小)。
        let start = self.split_at(range.start);
        let end = self.split_at(range.end);
        self.probed(text.len());
        self.chunks.splice(start..end, chunkify(text));
        self.total_bytes += text.len();
        self.total_bytes -= removed.len();

        AppliedEdit {
            removed,
            new_range,
        }
    }

    /// 取走「被改过的字节范围」并清空；`None` 是「自上次取走以来一个字都没改」。
    /// 谁取走谁负责把这段范围内的派生数据重算一遍——所以取走之后必须真的重算。
    pub(crate) fn take_dirty_region(&mut self) -> Option<Range<usize>> {
        self.dirty.take()
    }

    fn shift_anchors(&mut self, range: &Range<usize>, inserted: usize) {
        let deleted = range.end - range.start;
        let delta = inserted as isize - deleted as isize;
        for offset in self.anchors.iter_mut() {
            let Some(at) = offset else { continue };
            if *at > range.end {
                *at = (*at as isize + delta).max(range.end as isize) as usize;
            } else if *at > range.start {
                // 落在被删文本内部的锚点，钳到编辑起点。
                *at = range.start;
            }
            // 锚点正好等于 range.start：留在原地。
        }
    }

    /// 确保存在一个以 `offset` 开头的块，返回它的下标。
    fn split_at(&mut self, offset: usize) -> usize {
        if offset == self.byte_len() {
            return self.chunks.len();
        }
        let (index, local) = self.locate(offset);
        if local == 0 {
            return index;
        }
        let right = self.chunks[index].split_at(local);
        self.chunks.insert(index + 1, right);
        index + 1
    }

    /// 绝对字节偏移 →（块下标, 块内偏移）。落在块开头的偏移算给右边那块。
    fn locate(&self, offset: usize) -> (usize, usize) {
        let mut base = 0usize;
        for (index, chunk) in self.chunks.iter().enumerate() {
            let len = chunk.byte_len();
            if offset < base + len {
                return (index, offset - base);
            }
            base += len;
        }
        match self.chunks.len() {
            0 => (0, 0),
            n => (n - 1, self.chunks[n - 1].byte_len()),
        }
    }

    pub(crate) fn is_char_boundary(&self, offset: usize) -> bool {
        let (index, local) = self.locate(offset);
        match self.chunks.get(index) {
            Some(chunk) => chunk.text.is_char_boundary(local),
            None => offset == 0,
        }
    }
}

/// 按字节预算切文本，切点只落在字符边界上。
fn chunkify(text: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let cut = char_boundary_floor(rest, MAX_CHUNK_BYTES);
        chunks.push(Chunk::new(rest[..cut].to_string()));
        rest = &rest[cut..];
    }
    chunks
}

/// `wanted` 向左取整到 `text` 的字符边界，保证切分永不切碎多字节字符。
fn char_boundary_floor(text: &str, wanted: usize) -> usize {
    let wanted = wanted.min(text.len());
    if text.is_char_boundary(wanted) {
        return wanted;
    }
    let mut boundary = wanted;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

/// 这段文本里每个换行符的偏移（升序）。块只有 [`MAX_CHUNK_BYTES`] 大，所以偏移
/// 塞得进 `u16`；行号换算全指着这张表，再也不按字节数行。
fn newline_offsets(text: &str) -> Vec<u16> {
    debug_assert!(text.len() <= u16::MAX as usize, "块太大，u16 装不下换行偏移");
    text.bytes()
        .enumerate()
        .filter_map(|(index, byte)| (byte == b'\n').then(|| index as u16))
        .collect()
}
