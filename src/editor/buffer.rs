//! 文档文本缓冲区：编辑器里**唯一的内容事实源**。
//!
//! 块树是本结构的投影，每个块持有一段指向这里的字节区间（span）；保存写的就是
//! 这里，绝不从块树重新生成 markdown——那是「打开不编辑、保存被改写」的根因。
//!
//! 文本按 [`MAX_CHUNK_BYTES`] 分块存放，一次编辑只重建它真正碰到的那几个块，
//! 不在整篇文本上做 memmove，所以按键成本与文档大小近乎无关。

#[cfg(test)]
mod tests;

mod file_shape;

pub(crate) use file_shape::{is_text_prefix, FileShape};
#[cfg(test)]
pub(crate) use file_shape::starts_with_utf16_bom;

use std::ops::Range;
use std::sync::Arc;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

type SharedDirtyRegion = Rc<RefCell<Option<Range<usize>>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BufferChange {
    pub(crate) old_range: Range<usize>,
    pub(crate) new_len: usize,
    pub(crate) revision: u64,
    pub(crate) origin: u64,
    /// 这一笔换进来的文本比换掉的多了几个换行。源码视图的投影块按 512 行切，
    /// 行数一变分块边界就整体挪位，区域重投影只能整篇重建。
    pub(crate) newline_delta: i64,
}

macro_rules! shared_read {
    ($buffer:ident.$method:ident($($argument:expr),*)) => {
        if let Some(shared) = &$buffer.shared {
            return shared.borrow().$method($($argument),*);
        }
    };
}

macro_rules! shared_write {
    ($buffer:ident.$method:ident($($argument:expr),*)) => {
        if let Some(shared) = &$buffer.shared {
            return shared.borrow_mut().$method($($argument),*);
        }
    };
}

/// 给每份新建的缓冲区发一个进程内唯一的身份号。
static NEXT_BUFFER_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

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

/// 一次已经落地的编辑，自带逆操作。撤销栈只要把这些逆操作按反序重放，
/// 不需要再存任何全文快照。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AppliedEdit {
    /// 被换掉的原文；撤销时放回去的就是它。
    pub(crate) removed: String,
    /// 新文本落地后的字节区间；重做时把这段换回 `removed`。
    pub(crate) new_range: Range<usize>,
}

/// 块的**前缀索引**：`byte_starts[i]` 是第 i 块的首字节偏移，`line_prefix[k]` 是前 k 块
/// 里的换行总数（长度 = 块数 + 1，末项就是全篇换行数）。
///
/// 没有它时 `locate` / `line_start` / `line_count` / `slice` 都从第 0 块线性找。10 MiB
/// 一档约 2,560 块，而一次整篇重投影要把每个根块的源码区间都换算一遍：实测一次 ⌘Z
/// 在 10 MiB 档 **62 秒**，采样里绝大多数样本落在这些线性查找上
/// （`TextBuffer::locate` / `line_start` / `slice` + `Chunk::line_count`）。
#[derive(Clone, Default)]
struct ChunkStarts {
    byte_starts: Vec<usize>,
    line_prefix: Vec<usize>,
    /// 查询为了找到一块比较过几次。二分是 O(log 块数)；它随块数线性长就说明退回了
    /// 从第 0 块开始找（闸门 `chunk_lookups_do_not_scan_every_chunk` 盯它）。
    probes: std::cell::Cell<usize>,
}

impl ChunkStarts {
    fn rebuild(chunks: &[Chunk]) -> Self {
        let mut starts = Self {
            byte_starts: Vec::with_capacity(chunks.len()),
            line_prefix: Vec::with_capacity(chunks.len() + 1),
            probes: std::cell::Cell::new(0),
        };
        starts.line_prefix.push(0);
        let mut bytes = 0usize;
        let mut lines = 0usize;
        for chunk in chunks {
            starts.byte_starts.push(bytes);
            bytes += chunk.byte_len();
            lines += chunk.line_count();
            starts.line_prefix.push(lines);
        }
        starts
    }

    /// 从第 `from` 块起重建：编辑只动这一段的偏移与行号，前面的两项都没变。
    fn rebuild_from(&mut self, chunks: &[Chunk], from: usize) {
        let from = from.min(chunks.len());
        self.byte_starts.truncate(from);
        self.line_prefix.truncate(from + 1);
        if self.line_prefix.is_empty() {
            self.line_prefix.push(0);
        }
        // `from - 1` 不会越界：上面的 `truncate(from)` 之后 `byte_starts.len() <= from`，
        // 而 `from <= chunks.len()`（入口已经钳过）——也就是 `from == 0` 时走 `None`
        // 那一支，`from >= 1` 时 `chunks[from - 1]` 一定在界内。
        let mut bytes = match self.byte_starts.last() {
            Some(start) => start + chunks[from - 1].byte_len(),
            None => 0,
        };
        let mut lines = self.line_prefix[self.line_prefix.len() - 1];
        for chunk in &chunks[from..] {
            self.byte_starts.push(bytes);
            bytes += chunk.byte_len();
            lines += chunk.line_count();
            self.line_prefix.push(lines);
        }
    }

    /// 在 `index` 后面插进一个块（切块用它）：两块的前缀各自记一笔，后面的项不用动
    /// ——切块不改内容，总字节与总换行数都不变。
    fn note_split(&mut self, index: usize, left_bytes: usize, left_lines: usize) {
        let byte_start = self.byte_starts.get(index).copied().unwrap_or(0) + left_bytes;
        self.byte_starts.insert(index + 1, byte_start);
        let line_start = self.line_prefix.get(index).copied().unwrap_or(0) + left_lines;
        self.line_prefix.insert(index + 1, line_start);
    }

    /// 第一个 `start >= needle` 的下标；同时记下比较了几次（闸门盯它别退回线性）。
    fn lower_bound(&self, starts: &[usize], needle: usize) -> usize {
        let (mut low, mut high) = (0usize, starts.len());
        while low < high {
            self.probes.set(self.probes.get() + 1);
            let mid = low + (high - low) / 2;
            if starts[mid] < needle {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low
    }

    /// 绝对字节偏移 →（块下标, 块内偏移）。与线性版同口径：落在块开头的偏移算给右边
    /// 那块，越界钳到最后一块的块尾。
    fn locate(&self, chunks: &[Chunk], offset: usize) -> (usize, usize) {
        if chunks.is_empty() {
            return (0, 0);
        }
        let index = self
            .lower_bound(&self.byte_starts, offset.saturating_add(1))
            .saturating_sub(1)
            .min(chunks.len() - 1);
        let local = offset
            .saturating_sub(self.byte_starts[index])
            .min(chunks[index].byte_len());
        (index, local)
    }

    /// 第 `line` 行首个字节的偏移（`line` 越界返回 `total_bytes`）。与线性版的
    /// `line <= seen + count` 等价：找第一块「到它为止的累计换行数 >= line」。
    fn line_start(&self, chunks: &[Chunk], line: usize, total_bytes: usize) -> usize {
        if line == 0 {
            return 0;
        }
        let boundary = self.lower_bound(&self.line_prefix, line);
        if boundary == 0 || boundary > chunks.len() {
            return total_bytes;
        }
        let index = boundary - 1;
        self.byte_starts[index]
            + chunks[index].newline(line - self.line_prefix[index])
            + 1
    }

    /// 取走并清零「查询比较过几次块」。
    #[cfg(test)]
    fn take_probes(&self) -> usize {
        self.probes.replace(0)
    }
}


/// 文档文本。所有编辑最终都落成一次 [`TextBuffer::edit`]。
pub(crate) struct TextBuffer {
    chunks: Vec<Chunk>,
    /// 块的前缀索引：`locate` / `line_start` / `line_count` / `slice` 都靠它，别再从
    /// 第 0 块线性找（见 [`ChunkStarts`]）。
    starts: ChunkStarts,
    /// 全文字节数。逐块加起来是 O(文本块数)，而 `byte_len` 在按根块走一圈的循环里
    /// 会被调到 O(根块数 × 文本块数)（10 MiB 文档一次按键 6 秒就是这么来的），
    /// 所以它必须是 O(1)：只有 [`edit`](Self::edit) 会动内容，跟着它记就行。
    total_bytes: usize,
    /// 这个缓冲区对应的文件形状；非文件来源（粘贴片段、恢复快照）为 `None`。
    shape: Option<FileShape>,
    /// 打开时的原始字节。只在整个缓冲区一次编辑都没落过的时候有效——
    /// 那时「保存」可以是把这份字节原样写回去，一个字节都不必重新生成。
    pristine: Option<Arc<[u8]>>,
    /// 形状不能无损写回时的原始字节，**编辑之后仍然扣着**（见
    /// [`set_file_origin`](Self::set_file_origin) 的最后一道闸）。正常路径恒为
    /// `None`：不可无损表示的编码在读盘漏斗就被拒绝打开了。
    unrepresentable: Option<Arc<[u8]>>,
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
    /// 内容被改过多少次。读侧的派生缓存（搜索命中表）用它做失效判断：
    /// 只有 [`edit`](Self::edit) 会动内容，所以跟着它自增就足够，不需要比较字节。
    revision: u64,
    /// 这份缓冲区的身份号，进程内单调递增、每份唯一。
    ///
    /// 只有 `revision` 不够：**两份从没被编辑过的文档 revision 都是 0**，于是
    /// 「A.md 搜完切到 B.md 搜同一个词」会命中上一份文档的缓存，把 A 的命中
    /// 区间当成 B 的（实测就是搜索跳转后高亮整体消失）。身份号让缓存键跨文档
    /// 也不会撞车。
    identity: u64,
    shared: Option<Rc<RefCell<TextBuffer>>>,
    view_dirty: Option<SharedDirtyRegion>,
    shared_views: Vec<Weak<RefCell<Option<Range<usize>>>>>,
    shared_changes: Vec<BufferChange>,
}

impl Clone for TextBuffer {
    fn clone(&self) -> Self {
        shared_read!(self.clone());
        // 撤销试算依赖独立副本；只有 shared_view 才共享可编辑正文。
        Self {
            chunks: self.chunks.clone(),
            starts: self.starts.clone(),
            total_bytes: self.total_bytes,
            shape: self.shape,
            pristine: self.pristine.clone(),
            unrepresentable: self.unrepresentable.clone(),
            dirty: self.dirty.clone(),
            line_probe_bytes: self.line_probe_bytes.clone(),
            revision: self.revision,
            identity: self.identity,
            shared: None,
            view_dirty: None,
            shared_views: Vec::new(),
            shared_changes: Vec::new(),
        }
    }
}

impl TextBuffer {
    pub(crate) fn shared_view(&mut self) -> Self {
        let shared = if let Some(shared) = self.shared.clone() {
            shared
        } else {
            let content = std::mem::replace(self, Self::from_text(""));
            let shared = Rc::new(RefCell::new(content));
            *self = Self::new_shared_view(shared.clone());
            shared
        };
        Self::new_shared_view(shared)
    }

    fn new_shared_view(shared: Rc<RefCell<Self>>) -> Self {
        let dirty = Rc::new(RefCell::new(Some(0..shared.borrow().byte_len())));
        shared.borrow_mut().shared_views.push(Rc::downgrade(&dirty));
        let mut view = Self::from_text("");
        view.shared = Some(shared);
        view.view_dirty = Some(dirty);
        view
    }

    pub(crate) fn shared_writer_id(&self) -> u64 { self.identity }

    pub(crate) fn shared_changes(&self) -> Vec<BufferChange> {
        shared_read!(self.shared_changes());
        self.shared_changes.clone()
    }

    pub(crate) fn shared_handle(&self) -> Option<Self> {
        self.shared.clone().map(Self::new_shared_view)
    }

    pub(crate) fn clear_shared_changes(&mut self) {
        shared_write!(self.clear_shared_changes());
        self.shared_changes.clear();
    }

    /// 以给定文本建一个缓冲区。切分只按字节预算走，内容一字不改。
    ///
    /// 整个内容都算「刚改过的」：派生数据（大纲这类增量视图）于是第一次一定重算，
    /// 不会因为「一个字节都没编辑过」而留着上一份文档的结果。
    pub(crate) fn from_text(text: &str) -> Self {
        let chunks = chunkify(text);
        let probed = text.len();
        Self {
            starts: ChunkStarts::rebuild(&chunks),
            chunks,
            total_bytes: text.len(),
            shape: None,
            pristine: None,
            unrepresentable: None,
            dirty: (!text.is_empty()).then_some(0..text.len()),
            line_probe_bytes: std::cell::Cell::new(probed),
            revision: 0,
            identity: NEXT_BUFFER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            shared: None,
            view_dirty: None,
            shared_views: Vec::new(),
            shared_changes: Vec::new(),
        }
    }

    /// 记下这个缓冲区的文件来源：打开时读到的原始字节 + 由它判定的形状。
    pub(crate) fn set_file_origin(&mut self, raw: Vec<u8>, shape: FileShape) {
        shared_write!(self.set_file_origin(raw, shape));
        self.shape = Some(shape);
        let shared: Arc<[u8]> = raw.into();
        self.pristine = Some(shared.clone());
        // 最后一道闸：形状不能无损写回时，把原字节扣住不放。编辑之后
        // [`file_bytes`](Self::file_bytes) 仍交回这份字节，于是「保存」这个文档
        // 是一次什么都不写的操作——宁可这次编辑不落盘，也不把 lossy 文本重编码
        // 出来的垃圾盖上用户的文件（报修第 1 条的破坏形状）。
        // 正常入口走不到这里：`encoding::load_document` 在这种编码上直接拒绝打开。
        if !shape.is_lossless() {
            self.unrepresentable = Some(shared);
        }
    }

    pub(crate) fn inherit_file_shape_from(&mut self, previous: &Self) {
        if previous.shared.is_some() {
            self.inherit_file_shape_from(&previous.clone());
            return;
        }
        shared_write!(self.inherit_file_shape_from(previous));
        // 全文替换仍按当前文件形状写回，旧原始字节已不代表替换后的文本。
        self.shape = previous.shape;
        self.pristine = None;
        self.unrepresentable = previous.unrepresentable.clone();
    }

    pub(crate) fn file_shape(&self) -> Option<FileShape> {
        shared_read!(self.file_shape());
        self.shape
    }

    pub(crate) fn restore_file_shape(&mut self, shape: FileShape) {
        shared_write!(self.restore_file_shape(shape));
        self.shape = Some(shape);
        self.pristine = None;
    }

    /// 一次编辑都没落过时为真；此时 [`file_bytes`](Self::file_bytes) 就是原字节。
    /// 只有测试闸门读它——生产侧一律直接要 [`file_bytes`](Self::file_bytes)。
    #[cfg(test)]
    pub(crate) fn is_pristine(&self) -> bool {
        shared_read!(self.is_pristine());
        self.pristine.is_some()
    }

    /// 保存要写的字节：未编辑过就是打开时的原始字节，否则按形状重新编码。
    /// 形状不可无损表示时（见 [`set_file_origin`](Self::set_file_origin)）永远
    /// 是原字节——这种文档根本打不开，走到这一步说明有入口绕过了读盘漏斗。
    pub(crate) fn file_bytes(&self) -> Vec<u8> {
        shared_read!(self.file_bytes());
        if let Some(raw) = self.pristine.as_ref() {
            return raw.to_vec();
        }
        if let Some(raw) = self.unrepresentable.as_ref() {
            return raw.to_vec();
        }
        match self.shape {
            Some(shape) => shape.encode(&self.text()),
            None => self.text().into_bytes(),
        }
    }

    /// 与另一个缓冲区内容相同吗？逐块比字节，不复制任何一方。
    pub(crate) fn same_content(&self, other: &Self) -> bool {
        shared_read!(self.same_content(other));
        if let Some(shared) = &other.shared {
            return self.same_content(&shared.borrow());
        }
        self.byte_len() == other.byte_len()
            && self
                .chunks
                .iter()
                .flat_map(|chunk| chunk.text.bytes())
                .eq(other.chunks.iter().flat_map(|chunk| chunk.text.bytes()))
    }

    /// 内容与给定文本逐字节相同吗？不复制自己，用来跳过「其实没变的整篇重投影」。
    ///
    /// 按字节比，不按字符切片：块边界只对缓冲区自己的字符对齐，比较另一份内容时同样的
    /// 偏移可能落在某个多字节字符中间，`text[a..b]` 会直接 panic。
    pub(crate) fn matches_text(&self, text: &str) -> bool {
        shared_read!(self.matches_text(text));
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
        shared_read!(self.text());
        let mut out = String::with_capacity(self.byte_len());
        for chunk in &self.chunks {
            out.push_str(&chunk.text);
        }
        out
    }

    pub(crate) fn byte_len(&self) -> usize {
        shared_read!(self.byte_len());
        self.total_bytes
    }

    /// 内容版本号：每次 [`edit`](Self::edit) 自增。派生缓存拿它做失效判断。
    pub(crate) fn revision(&self) -> u64 {
        shared_read!(self.revision());
        self.revision
    }

    /// 这份缓冲区的身份号，与内容改了多少次无关。见 `identity` 字段的说明。
    pub(crate) fn identity(&self) -> u64 {
        shared_read!(self.identity());
        self.identity
    }

    /// 取 `range` 指向的字节区间。
    pub(crate) fn slice(&self, range: Range<usize>) -> String {
        shared_read!(self.slice(range));
        let mut out = String::with_capacity(range.end.saturating_sub(range.start));
        if range.start >= range.end || self.chunks.is_empty() {
            return out;
        }
        // 从起点那一块开始，不再从第 0 块扫；越过右端就停。
        let (first, _) = self.starts.locate(&self.chunks, range.start);
        let mut base = self.starts.byte_starts[first];
        for chunk in &self.chunks[first..] {
            let end = base + chunk.byte_len();
            if range.end <= base {
                break;
            }
            if range.start < end {
                let from = range.start.max(base) - base;
                let to = range.end.min(end) - base;
                out.push_str(&chunk.text[from..to]);
            }
            base = end;
        }
        out
    }

    /// 行数按 `split('\n')` 计：`"a\n"` 是 2 行（第二行为空），空文档是 1 行。
    /// 与编辑器状态栏、搜索结果的行号口径一致。
    pub(crate) fn line_count(&self) -> usize {
        shared_read!(self.line_count());
        self.starts.line_prefix.last().copied().unwrap_or(0) + 1
    }

    /// 为了找一块扫过/比较过几个块：二分是 O(log 块数)，线性是 O(块数)。
    /// 只有测试闸门读它（生产没有消费方）。
    #[cfg(test)]
    pub(crate) fn take_chunk_probes(&self) -> usize {
        shared_read!(self.take_chunk_probes());
        self.starts.take_probes()
    }

    /// 取走「建/维护换行索引读了多少字节」并清零。查询行号不该让它动一下。
    /// 只有测试闸门读它（生产没有消费方）。
    #[cfg(test)]
    pub(crate) fn take_line_probe_bytes(&self) -> usize {
        shared_read!(self.take_line_probe_bytes());
        self.line_probe_bytes.replace(0)
    }

    fn probed(&self, bytes: usize) {
        self.line_probe_bytes.set(self.line_probe_bytes.get() + bytes);
    }

    /// 第 `line` 行首个字节的偏移。`line` 越界时返回文末偏移。
    pub(crate) fn line_start(&self, line: usize) -> usize {
        shared_read!(self.line_start(line));
        self.starts.line_start(&self.chunks, line, self.byte_len())
    }

    /// 第 `line` 行（0 基）的字节区间，**不含**行尾换行符。越界钳到文末。
    ///
    /// 读取侧（搜索跳转、大纲点击、行列号）都从这里把行号换成字节，
    /// 于是它们说的坐标天然是文件坐标。
    pub(crate) fn line_range(&self, line: usize) -> std::ops::Range<usize> {
        shared_read!(self.line_range(line));
        let start = self.line_start(line);
        let mut end = self.line_start(line + 1).min(self.byte_len());
        if end > start && self.byte_at(end - 1) == Some(b'\n') {
            end -= 1;
        }
        start..end.max(start)
    }

    /// 该偏移处的字节；越界返回 `None`（多字节字符内部的字节照样能取到）。
    pub(crate) fn byte_at(&self, offset: usize) -> Option<u8> {
        shared_read!(self.byte_at(offset));
        let (index, local) = self.locate(offset);
        self.chunks
            .get(index)
            .and_then(|chunk| chunk.text.as_bytes().get(local).copied())
    }

    /// 字节偏移所在行（0 基）。`offset` 等于全文长度时算作末行。
    ///
    /// `offset` 必须是字符边界（搜索命中与光标位置天然是）。
    pub(crate) fn line_of(&self, offset: usize) -> usize {
        shared_read!(self.line_of(offset));
        let (chunk_index, local) = self.locate(offset);
        let Some(chunk) = self.chunks.get(chunk_index) else {
            return 0;
        };
        debug_assert!(
            chunk.text.is_char_boundary(local),
            "line_of 的偏移落在多字节字符中间：{offset}"
        );
        // 这一块之前有多少个换行，前缀索引里已经记着（`line_prefix[chunk_index]`）。
        // 以前是拿 `chunks[..chunk_index]` 现累一遍——`reproject_root_region` 每根块
        // 都要问两三次行号，10 万根块就是十万次线性累加。
        self.starts.line_prefix[chunk_index] + chunk.lines_before(local)
    }

    /// 一次遍历把多个**升序**字节偏移的「（所在行, 本行首字节偏移）」一起取回来。
    ///
    /// 单点问 [`line_of`](Self::line_of) 是沿文本块累加换行数的线性活（这里没有行
    /// 索引树），在「逐根块」的循环里调它就变成 O(根块数 × 文本块数)——10 MiB 文档
    /// 一次按键 6 秒就是这么来的。批量问一次只走一遍块。越界的偏移钳到文末。
    ///
    /// 走的是每块那张换行偏移表：一次正文都不读，代价与文档多大无关。
    pub(crate) fn lines_and_line_starts(&self, offsets: &[usize]) -> Vec<(usize, usize)> {
        shared_read!(self.lines_and_line_starts(offsets));
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
    /// 编辑区间兜底：越界钳到文末、起点大于终点交换、端点退到字符边界。
    /// 正常路径不该走到这里，钳位只保证坏区间不把缓冲区打崩。
    fn clamp_edit_range(&self, range: Range<usize>, total: usize) -> Range<usize> {
        let mut start = range.start.min(total);
        let mut end = range.end.min(total);
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        while start > 0 && !self.is_char_boundary(start) {
            start -= 1;
        }
        while end > start && !self.is_char_boundary(end) {
            end -= 1;
        }
        start..end
    }

    pub(crate) fn edit(&mut self, range: Range<usize>, text: &str) -> AppliedEdit {
        if let Some(shared) = &self.shared {
            let mut content = shared.borrow_mut();
            let edit = content.edit(range, text);
            if let Some(change) = content.shared_changes.last_mut() { change.origin = self.identity; }
            return edit;
        }
        let total = self.byte_len();
        let clamped = self.clamp_edit_range(range.clone(), total);
        if clamped != range {
            // 上游区间过期或端点落在字符中间：钳位后继续，别让一次坏区间
            // 把整窗打崩；日志留给排查越界来源。
            eprintln!(
                "velora: 编辑区间 {range:?} 非法（缓冲区 {total} 字节），已钳位为 {clamped:?}"
            );
        }
        let range = clamped;

        let removed = self.slice(range.clone());
        self.revision = self.revision.wrapping_add(1);
        self.pristine = None;
        let new_range = range.start..range.start + text.len();
        self.dirty = Some(match self.dirty.take() {
            Some(seen) => seen.start.min(new_range.start)..seen.end.max(new_range.end),
            None => new_range.clone(),
        });

        // 只在两个端点处切块，端点之间的整块被移除，新文本单独成分块。
        // 顺序要紧：切分起点会插入一个块，终点必须在切完之后重新算。
        // 切块只是把两张换行表分开（哪一侧都不重读正文），只有新进来的这段文本
        // 要现数一遍换行——所以一次编辑的索引成本是 O(改动大小)，不是 O(文档大小)。
        let start = self.split_at(range.start);
        let end = self.split_at(range.end);
        self.probed(text.len());
        let inserted = chunkify(text);
        let inserted_chunks = inserted.len();
        self.chunks.splice(start..end, inserted);
        let mut index = start.saturating_sub(1);
        let mut boundary = (start + inserted_chunks + 1).min(self.chunks.len());
        // 下标有界：`index + 1 < boundary`，而 `boundary` 初值是
        // `(start + inserted_chunks + 1).min(chunks.len())`、每次合并只减一，
        // 所以 `index + 1 < chunks.len()` 恒成立。
        while index + 1 < boundary {
            if self.chunks[index].byte_len() + self.chunks[index + 1].byte_len() <= MAX_CHUNK_BYTES {
                let right = self.chunks.remove(index + 1);
                let left = &mut self.chunks[index];
                let offset = left.byte_len();
                left.text.push_str(&right.text);
                left.newlines.extend(
                    right.newlines.into_iter().map(|at| (offset + at as usize) as u16),
                );
                boundary -= 1;
            } else {
                index += 1;
            }
        }
        self.total_bytes += text.len();
        self.total_bytes -= removed.len();
        // 拼接与合并都从 `start - 1` 起动过块边界，前缀索引只重建这一段。
        self.starts
            .rebuild_from(&self.chunks, start.saturating_sub(1));

        if !self.shared_views.is_empty() {
            self.shared_changes.push(BufferChange {
                old_range: range,
                new_len: text.len(),
                revision: self.revision,
                origin: self.identity,
                newline_delta: text.bytes().filter(|byte| *byte == b'\n').count() as i64
                    - removed.bytes().filter(|byte| *byte == b'\n').count() as i64,
            });
            self.shared_views.retain(|view| {
                let Some(view) = view.upgrade() else { return false; };
                let mut dirty = view.borrow_mut();
                *dirty = Some(match dirty.take() {
                    Some(seen) => seen.start.min(new_range.start)..seen.end.max(new_range.end),
                    None => new_range.clone(),
                });
                true
            });
        }

        AppliedEdit {
            removed,
            new_range,
        }
    }

    /// 取走「被改过的字节范围」并清空；`None` 是「自上次取走以来一个字都没改」。
    /// 谁取走谁负责把这段范围内的派生数据重算一遍——所以取走之后必须真的重算。
    pub(crate) fn take_dirty_region(&mut self) -> Option<Range<usize>> {
        if let Some(dirty) = &self.view_dirty {
            return dirty.borrow_mut().take();
        }
        self.dirty.take()
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
        // 切完之后左半段就是 `self.chunks[index]`：它的块长与换行数正好是新块的两项前缀。
        let left_lines = self.chunks[index].line_count();
        self.starts.note_split(index, local, left_lines);
        self.chunks.insert(index + 1, right);
        index + 1
    }

    /// 绝对字节偏移 →（块下标, 块内偏移）。落在块开头的偏移算给右边那块。
    fn locate(&self, offset: usize) -> (usize, usize) {
        self.starts.locate(&self.chunks, offset)
    }

    pub(crate) fn is_char_boundary(&self, offset: usize) -> bool {
        shared_read!(self.is_char_boundary(offset));
        let (index, local) = self.locate(offset);
        match self.chunks.get(index) {
            Some(chunk) => chunk.text.is_char_boundary(local),
            None => offset == 0,
        }
    }

    /// 这个区间能不能拿去做 [`slice`](Self::slice)：两端都在界内、都落在字符边界
    /// 上，而且没有倒置。
    ///
    /// 判据只此一处（`runtime_context` 的引用屏幕与 `history` 的行签名都拿它当
    /// 可信性闸）：`is_char_boundary` 对**越界**偏移会返回 true（`locate` 先把它钳到
    /// 末尾那块再问块尾），所以「在界内」必须单独查；而 `slice` 对 `start >= end`
    /// 返回空串，倒置区间会伪装成「可信的空源码」。
    pub(crate) fn span_is_trustworthy(&self, span: &Range<usize>) -> bool {
        shared_read!(self.span_is_trustworthy(span));
        span.start <= span.end
            && span.end <= self.byte_len()
            && self.is_char_boundary(span.start)
            && self.is_char_boundary(span.end)
    }

    /// 把偏移向左退到字符边界（越界先钳到文末）。
    ///
    /// 用在「拿一份**旧内容**里记下的字节偏移去问现在这份内容」的地方：外部改动
    /// 插过或删过字节之后，那个偏移可能正落在多字节字符中间，而后续的行号与块区间
    /// 换算处处按边界走（[`line_of`](Self::line_of) 直接断言失败）。
    pub(crate) fn floor_char_boundary(&self, offset: usize) -> usize {
        shared_read!(self.floor_char_boundary(offset));
        let mut offset = offset.min(self.byte_len());
        // UTF-8 的字符最长 4 字节，最多退三步。
        while offset > 0 && !self.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
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
