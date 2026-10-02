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
    /// 本块内 `'\n'` 的个数，行号换算靠它，不必重扫文本。
    newlines: usize,
}

impl Chunk {
    fn byte_len(&self) -> usize {
        self.text.len()
    }

    fn new(text: String) -> Self {
        let newlines = count_newlines(&text);
        Self { text, newlines }
    }

    fn split_at(&mut self, local: usize) -> Chunk {
        debug_assert!(
            self.text.is_char_boundary(local),
            "切分点 {local} 落在多字节字符中间"
        );
        let right = self.text.split_off(local);
        self.newlines = count_newlines(&self.text);
        Chunk::new(right)
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
    /// 锚点槽位 → 当前绝对字节偏移；`None` 是空槽（可回收）。
    anchors: Vec<Option<usize>>,
    free_anchor_slots: Vec<usize>,
    /// 这个缓冲区对应的文件形状；非文件来源（粘贴片段、恢复快照）为 `None`。
    shape: Option<FileShape>,
    /// 打开时的原始字节。只在整个缓冲区一次编辑都没落过的时候有效——
    /// 那时「保存」可以是把这份字节原样写回去，一个字节都不必重新生成。
    pristine: Option<Arc<[u8]>>,
}

impl TextBuffer {
    /// 以给定文本建一个缓冲区。切分只按字节预算走，内容一字不改。
    pub(crate) fn from_text(text: &str) -> Self {
        Self {
            chunks: chunkify(text),
            anchors: Vec::new(),
            free_anchor_slots: Vec::new(),
            shape: None,
            pristine: None,
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
    pub(crate) fn matches_text(&self, text: &str) -> bool {
        if self.byte_len() != text.len() {
            return false;
        }
        let mut offset = 0usize;
        for chunk in &self.chunks {
            let len = chunk.byte_len();
            if text[offset..offset + len] != chunk.text[..] {
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
        self.chunks.iter().map(Chunk::byte_len).sum()
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
        self.chunks.iter().map(|chunk| chunk.newlines).sum::<usize>() + 1
    }

    /// 第 `line` 行首个字节的偏移。`line` 越界时返回文末偏移。
    pub(crate) fn line_start(&self, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        let mut base = 0usize;
        let mut seen = 0usize;
        for chunk in &self.chunks {
            if line <= seen + chunk.newlines {
                return base + nth_newline(&chunk.text, line - seen) + 1;
            }
            seen += chunk.newlines;
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
            .map(|earlier| earlier.newlines)
            .sum::<usize>();
        preceding + count_newlines(&chunk.text[..local])
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
        self.shift_anchors(&range, text.len());

        // 只在两个端点处切块，端点之间的整块被移除，新文本单独成分块。
        // 顺序要紧：切分起点会插入一个块，终点必须在切完之后重新算。
        let start = self.split_at(range.start);
        let end = self.split_at(range.end);
        self.chunks.splice(start..end, chunkify(text));

        AppliedEdit {
            removed,
            new_range: range.start..range.start + text.len(),
        }
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

/// 块内第 `nth` 个换行符的字节偏移（1 基）。找不到时返回文本长度。
fn nth_newline(text: &str, nth: usize) -> usize {
    let mut seen = 0usize;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            seen += 1;
            if seen == nth {
                return index;
            }
        }
    }
    text.len()
}

fn count_newlines(text: &str) -> usize {
    text.bytes().filter(|byte| *byte == b'\n').count()
}
