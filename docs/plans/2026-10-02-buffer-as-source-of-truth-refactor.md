# Velora 文档架构重构方案：文本缓冲区作为唯一事实源

> 状态：**待签字**。签字后才动代码。
> 基线：`main`（搜索修复已落地，1089 测试全绿）。
> 引用约定：以 `文件:函数名` 为主，行号是本方案写作时（2026-10-02）的定位参考，会漂移。

## 0. 结论速览

| 决策项 | 结论 |
|---|---|
| 事实源 | 内存 `TextBuffer`，严格镜像磁盘文件（保存时才写盘）；块树降为**指向 buffer 的锚点区间 + 派生投影** |
| buffer 数据结构 | 自研**分块文本 + 前缀和索引**（`Vec<Chunk>` + Fenwick），不是完整 rope，也不是 `String` + 区间表 |
| 块定位 | `AnchoredSpan { start: Anchor, end: Anchor }`，Anchor = `(chunk_id, byte_within_chunk)`，编辑后自动移位，零反推 |
| 写回粒度 | **区间级**（一次编辑 = buffer 上一次字节 splice），**绝不做「从模型重新生成整块」**。因为定界符在导入时就没有存进模型（`state.rs:119` 注释：`NumberedListItem` 序列化用 canonical dot marker；`inline/tree.rs:151` 注释：fragment 从不存 marker 字符），任何整块重生成必然改写用户没碰过的写法（`1)`→`1.`、`__粗__`→`**粗**`、表格重新对齐） |
| 保存 | 未编辑 → 原样写回打开时抓到的原始字节；已编辑 → buffer 内容按 `FileShape`（编码/EOL/末行换行）重编码 |
| 搜索 | 一套扫描器，两个数据源：打开的文档扫 buffer，未打开的扫文件字节。删除 `match_ordinal` 猜测式跳转 |
| 迁移 | 三阶段，每阶段可发布、全绿再进下一阶段；不存在长期双轨 |
| 隐式规范化 | 从打开/保存路径**彻底移除**；`collect_root_markdown_lines` 那套规则若要保留，只挂在显式 `格式化文档` 命令上 |
| 旧测试 | 约 120–150 个断言规范化输出的测试按新语义改写为「未编辑导出 == 原文」；确属块级序列化行为的下沉为块级单元测试 |

**与已废弃的 `s1-sticky-source` 的本质区别**（一句话）：s1 是**读取时二选一**（每块要么吐粘住的原文、要么走结构化序列化，需要一个无死角的「这块有没有被编辑过」判定）；本方案是**写入后单一来源**（buffer 只有一份，块的内容恒等于 `parse(buffer[span])`，不存在判定问题，因为「谁被改了」是编辑路径告诉我们的，不是猜出来的）。

---

## 1. 现状诊断：为什么必须切主

### 1.1 磁盘原文在导入后就消失了

- `Editor` 上唯一的内容字段是 `document: DocumentTree`（mod.rs `struct Editor`）；`roots: Vec<Entity<Block>>` 之外不留任何原文副本。
- `BlockRecord`（state.rs）持 `title: InlineTextTree`（定界符不存储，序列化时重建）、`table`/`html`/`raw_fallback`。只有 Raw 保留类块（FrontMatter/Comment/HtmlBlock/MathBlock/Mermaid/RawMarkdown）存了逐字原文。
- `last_stable_source_text`（mod.rs）名字像原文，其实是 `document.markdown_text(cx)` 的派生快照。
- 保存链路：`request_save_document` → `sync_pending_save` → `save_document` → `serialized_document_text`（persistence.rs，Rendered 走 `markdown_text`，Source 走 `raw_source_text`）→ `write_atomic`。**全程没有磁盘字节参与**。

结论：`打开 → 不编辑 → 保存` 必然改写文件。这不是 bug，是数据模型的必然后果。

### 1.2 有损点清单（都会被本方案消灭）

| 有损行为 | 出处 |
|---|---|
| 相邻根块之间强制补空行 | `tree.rs:collect_root_markdown_lines`（`separator = pending_empty_roots + 1`，列表组内例外） |
| 表格重新排版（`|------|` → `| --- |`，列宽对齐空格丢失） | `table.rs:serialize_table_markdown_lines` |
| Setext 标题 → ATX；`1)` → `1.`；`>` → `> `；缩进代码 → 围栏 | `document/tests/import_round_trip.rs` 一整批断言 |
| CRLF 丢失（仅代码文档还原） | mod.rs 导入时 `CRLF→LF`，只有 `code_uses_crlf` 在保存时还原 |
| 末行换行有无不保持 | import.rs 丢掉尾部空行段（`blank_run_len - 1`），序列化 `lines.join("\n")` |
| 非 UTF-8 文件保存后变 UTF-8 | encoding.rs 模块注释明示「保存仍按 UTF-8 写出」 |

### 1.3 性能与内存：同一个根因的第二种痛

- 每按一键：`prepare_undo_capture` clone 整篇 + `finalize_pending_undo_capture` 重新序列化整篇 + 字符串比较（history.rs）。`perf_budgets.rs` 注释实测：**10 MiB 夹具单次按键 13 秒**，所以预算表降到 1 MiB。
- `HistoryEntry { source_text }` 是全文快照，`HISTORY_LIMIT=200` → 10 MiB 文档最坏约 2 GB。
- `restore_history_entry` 撤销 = 整树重解析。
- 已有的 `docs/architecture/editor-core.md` §7 也把这写成「大文件隐患」。

### 1.4 派生出来的补丁群（本方案顺手删掉的东西）

因为「文本不是一等公民」，仓库里长出了一整层**用重解析来反推文本坐标**的 machinery：

- `source_mapping.rs:build_source_target_mappings`（约 700 行）：为了算字节偏移，把标题标记、`> ` 引用包裹、代码围栏与缩进、脚注头、表格 `+2/+3` 间距**重新拼一遍**。里面有自述「此前映射自行记账导致漂移甚至切进多字节字符中间（用户报修 coredump）」的注释，和字符边界钳制的兜底。
- `render_search.rs:open_search_hit` 的 `match_ordinal`：因为块树序列化后行号与磁盘不一致，跳转退化成「第 k 个含词行」的猜测。
- 工作区搜索只扫磁盘（`search_backend.rs:cached_file_source` + 全局 `path→(mtime,len,content)` LRU），**未保存的编辑在搜索结果里不可见**；`workspace_index.rs` 为此专门加了一个 active-document overlay（链接面板同理，还带 500ms 防抖）。
- 大纲扫的是 `last_stable_source_text`，与 `current_document_source` 是**两份不同的快照**（`workspace/tests/search.rs` 里那句 `build_outline_tree` 注释就在打印这个不一致），且 front matter 里的 YAML `#` 会被当标题。
- `selection.rs:rebuild_after_cross_block_source_edit`：跨块编辑 = 重解析全文 = 所有块身份丢失。

---

## 2. 目标架构

```
                      ┌──────────────────────────────┐
   打开文件 ──字节──▶  │  TextBuffer（唯一事实源）     │
                      │  chunks + 前缀和 + Anchor     │
                      │  FileShape{编码/EOL/末行换行}  │
                      │  pristine: 原文字节（编辑前）  │
                      └───────┬──────────────────────┘
              读（投影）       │        写（唯一窄口）
        ┌────────────────┬─────┴──────┬──────────────┐
        ▼                ▼            ▼              ▼
   搜索扫描器        大纲/字数      块树投影      源码模式视图
   (字节区间命中)    (行索引)      spans 来自解析   (直接是 buffer)
                                     │
                                     ▼
                              Block 实体（渲染、光标、IME、折叠）
                              record.title == parse(buffer[span])  ← 不变式，debug_assert
```

**三条铁律**

1. **只有一个写入方向**：所有编辑最终都落成 `TextBuffer::edit(range, text)`；块树永远不是写入的起点，而是编辑落地后被重新投影的结果。
2. **不引入「哪个块被改过」的判定**：编辑路径显式携带它影响的 buffer 区间；重投影的范围由这个区间决定，不由内容比较决定。
3. **绘制热路径不碰 buffer**：块的绘制只读自己缓存的 `InlineRenderCache`，不在 paint 里解析、不在 paint 里重锚。这条由 `perf_budgets.rs` 的「静止帧 0 次全文级工作」断言继续守住（现在已有，`perf_budgets.rs:214`）。

---

## 3. 选型一：文本缓冲区的数据结构

### 3.1 结论：分块文本 + 前缀和（自研，最小 API）

```rust
pub(crate) struct TextBuffer {
    chunks: Vec<Chunk>,            // 顺序存放，永不整体搬文本
    byte_lens: Fenwick<u64>,       // 前缀和 → offset ↔ chunk：O(log C)
    line_counts: Fenwick<u64>,     // 前缀和 → offset ↔ line / line ↔ offset：O(log C)
    revision: u64,                 // 每次 edit 递增；缓存的 key
    shape: FileShape,              // encoding / line_ending / final_newline / bom
    pristine: Option<Arc<[u8]>>,   // 打开时的原始字节，首次落编辑前保留
}

struct Chunk { id: ChunkId, text: Box<str>, line_count: u32 }

pub(crate) struct Anchor { chunk: ChunkId, byte_within: u32, affinity: Bias }
```

**为什么够用（数字依据）**

- 10 MiB ÷ 4 KiB ≈ **2560 个 chunk**（÷1 KiB ≈ 10240）。`Vec<Chunk>` 中间插入是一次 `memmove` of ~2.5k–10k × 32 B ≈ 80–320 KB，量级 **几十微秒**；文本本身不搬。
- Fenwick 查询 2560 项 = 12 步，10240 项 = 14 步。每次按键对**整篇文档的遍历次数 = 0**（这是 `perf_budgets.rs:per_keystroke_document_passes_stay_bounded` 想主张但当前做不到的）。
- 撤销栈改存 `Vec<Edit>`（逆操作），200 步历史在 10 MiB 文档上的内存从 ~2 GB 降到 ~KB 级。

**API 面（够用就行，刻意做小）**

`text()`/`slice(range)`、`byte_len()`、`edit(range, &str) -> AppliedEdit`（含逆操作）、`line_of(offset)`、`line_start(i)`、`line_count()`、`resolve(anchor) -> usize`、`anchor_at(offset)`、`rev_range(..)`。
文本区间一律 **UTF-8 字节**；GPUI 的 UTF-16 只在 block input handler 边界转换（现在是 `input.rs:range_from_utf16` 每次 O(n) 扫 chars()，有行索引后可降到 O(log C)）。

### 3.2 否掉的两个方案

**`String` + 区间表**（最省事）
- 任何一次 p 处的插入，p 之后所有 offset 都要 +delta：块 span 表是 O(#块) 更新（10 MiB 文档 ~20 万块 → 每键碰 1.6 MB），文本本身 `memmove` 整篇（前端插入 = 搬 10 MB）。
- 更致命的：所有消费者（搜索、大纲、字数、撤销）要么每次复制 10 MB，要么持一份必然陈旧的快照——**这正是 `last_stable_source_text` / 大纲两份快照不一致这一整类 bug 的形状**。用 String 等于把现在的病换个地方放。

**完整 rope（`gpui_sum_tree` 版，Zed 的 `Buffer` 形态）**
- 依赖树里已经有 `gpui_sum_tree 0.2.2`（gpui 内部用），技术上可行，且它提供子树摘要查询、结构共享（免费快照）。
- 但我们的需求只有：编辑、offset↔line/anchor 换算、区域重解析。**不需要**结构共享（撤销走 delta）、不需要并发分片。自研版的代码量估计 600–900 行（含单元测试），比接 SumTree 再学会它的 annotation 机制更省。
- 留口子：`TextBuffer` 的对外 API 按 rope 的形状设计，将来若 profile 出必须树形（例如超大文档要 O(log) 的区间重解析定位），换内部实现不动调用方。

**分块不变式**
- chunk 大小区间 `[1024, 8192]`；插入过长则切分；相邻过小则合并；切分点必须落在 char 边界（`floor_char_boundary`）。
- `Anchor` 的稳定性依赖 chunk id 单调分配 + 只在本地 split/merge。跨 chunk 边界的 anchor 在 merge 时按左 chunk 折算。

---

## 4. 选型二：块 = 指向 buffer 的锚点区间

```rust
pub(crate) struct SourceRegion {
    span: Range<Anchor>,      // 含标记/前缀的整块源码区间
    content: Range<Anchor>,   // 去掉 `# `、`> `、围栏行之后的内容区间
}
```

**span 从哪来：导入器本来就知道。** `build_blocks_from_lines_internal`（import.rs）是一个在 `&[String]` 上按 `index` 推进的循环，每根块消费 `index_old..index_new`（例：front matter 段 `index = close + 1`）。现在只是没把这段记下来。改动是**机械的**：返回 `(roots, Vec<LineRange>, next_index)`，再用 `line_byte_starts`（buffer 行索引一次预计算）把行区间换算成字节区间。

对比现状：span 是在**序列化时**由 `tree.rs:markdown_text_with_block_spans` 现算（`collect_root_markdown_lines` 顺手 push 行区间，再把行号换算成字节），所以 span 永远属于「重新生成后的文本」而不是磁盘文本——这是行号错位的结构根源。

**块内映射：保留这张表，但把它从「序列化空间」重锚到「源码空间」。** 每块已有的 `visible ↔ markdown` 双向表（`inline/tree.rs:markdown_offset_map`、`cursor.rs:current_range_to_markdown_range`）继续存在，范围仍然是块内（成本有界），但表的 markdown 一侧语义要换：从「该块序列化出来的 markdown」换成「该块在 buffer 里的那段源码」。

但它现在有一个**在此之前无害、切换后致命**的性质：表的 markdown 一侧是 `serialize_markdown()` 的**产物**，不是磁盘文本。今天块树的输出恒等于文档，所以两者重合、无人察觉；buffer 成为事实源后，拿序列化空间的下标去切 buffer 字节必然错位（`__粗__` 磁盘占 8 字节，序列化后的 `**粗**` 只占 6 字节）。所以这张表要改成**解析时直接记录每个 fragment 在源文本里的字节区间**，得到一张 `source ↔ visible` 表：

```
绝对字节 = buffer.resolve(region.span.start) + region.content_prefix_len + 块内 source 偏移
```

这一张表同时吃掉 `source_mapping.rs` 里「为了知道 `# ` 占几字节而重新拼一遍 `# `」的那 700 行——偏移是解析时记录的，不是反推的。

**这就是区间级写回的全部前提**：用户在块内删了 3 个字符，翻译结果是「buffer 上 `[a,b)` 换成 3 个字符的等价源码区间」， splice 完，这块里其余字节（`1)`、`__粗__`、表格 `|------|`、列宽对齐空格）一个都没被碰过。不存在「从模型重生成整块」这一步。

**不变式（写成 debug_assert）**：任意时刻 `BlockRecord.title == InlineTextTree::from_markdown(buffer[slice(span)])` 的可见文本必须一致。所有投影路径都验这条，不通过就 panic 在最近的写入点，而不是等到跳转跳错行。

---

## 5. 数据流

### 5.1 打开

```
读文件字节 → 记 pristine + FileShape(encoding, line_ending, final_newline, bom)
           → 解码为文本（CRLF→LF 是 buffer 的规范形态，可逆）
           → TextBuffer::from_text
           → 首块同步解析（FIRST_CHUNK_ROOTS=2000），其余后台续建（保留现有分块流式）
           → 每根块带 SourceRegion
```
- 现在 buffer 与分块导入是同一件事的两半（`PendingTail { lines: Arc<Vec<String>>, next_line }` 已经在持原始行并「未解析部分逐字拼接」以保证保存完整）——重构后 `PendingTail` 直接指向 buffer 的尾部字节区间，`markdown_text` 里那段拼接尾巴的特判消失。
- 外部修改（watcher.rs → `reload_externally_changed_document`）：改成用新文件字节重建 buffer（干净的标签），脏的标签保持现状（冲突在保存时报）。重解析走同一个投影入口。

### 5.2 保存

```rust
if pristine.is_some() && !dirty { 原样写 pristine  }   // 任何编码/EOL/BOM 都字节不变
else { encode(buffer.text(), shape) 原子写 }            // LF→CRLF、补末行换行、GB18030/UTF-8
```
- `write_atomic`（同目录临时文件 + `sync_all` + `rename`）、`verify_file_version`、autosave 防抖骨架全部保留，只把内容来源从「重新序列化」换成 buffer。
- `file_content_version` 改为对**原始字节**求哈希（现在是对规范化后的文本），外部修改检测从「模糊相等」变成「精确」。
- 落一次编辑后 `pristine` 置 `None`（避免长期双份内存）。

### 5.3 渲染模式编辑 → 写回 → 重投影

单块文本编辑（**唯一一条路径，区间级写回**）：

| 步 | 动作 |
|---|---|
| 1 | GPUI `replace_text_in_range`(utf16 range, text) → 块内 visible 字节区间（现有逻辑不变） |
| 2 | visible → 块内 **source** 区间（§4 重锚后的 `source ↔ visible` 表；IME marked range 同一路径） |
| 3 | `buffer.edit(resolve(span.start) + prefix_len + source_range, 新文本)` —— 一次 splice，其余字节不动 |
| 4 | 该块 span 变化 → 重投影该块（只对 `buffer[span]` 重解析出 `title`/表格/inline）；后续块的 Anchor 自动移位，**零工作量** |
| 5 | 边界检查（见下）→ 决定是否要扩大重解析范围 |
| 6 | `document_revision` 递增；缓存按 revision 失效，行类缓存可按受影响行窗口局部失效 |

**不做的事**（写死成不变式，谁做谁违反验收 3）：不调用 `BlockRecord::markdown_line` / `serialize_markdown` 去生成保存内容或去替换整块。块级序列化只服务导出（HTML/PDF/复制为 markdown）和显式格式化命令。

**第 5 步的边界分级**（这才是本方案真正的新风险，与写回粒度无关）：

| 情形 | 判据 | 重解析范围 |
|---|---|---|
| 常态 | 新文本不含边界 token | 只重投影该块 |
| 可能跨块 | 新文本含 `\n`、`---`、``` 围栏、`# `、列表标记、`> `、`[^`、`:` 定义式开头 | 扩到受影响区域 |
| 结构操作 | Enter 拆分 / Backspace 合并 / 粘贴多行 / 跨块删除 | **span 算术，不重解析**（见下） |

**回退层级（写死，防止「整篇重解析」变成常态）**：

1. **单块内编辑**（绝大多数）：只重投影该块。
2. **拆分 / 合并 / 跨块删除 / 粘贴**：**不需要重解析**——一个换行把一个 span 切成两个 span、合并就是把两个 span 并起来，块内容仍是 `buffer[新 span]`。这是区间算术，代价与文档大小无关。（今天的 `selection.rs:rebuild_after_cross_block_source_edit` 走的是全文重解析，属于本方案要删掉的债，不是要继承的行为。）
3. **打字生成了可能吞并后续行的构造**（新起的围栏、front-matter 式 `---`、列表标记下的缩进延续…）：扩到**该块所在的最外层容器链**，仍局限在局部。
4. **全篇重解析**：只作为「未知情形的正确性兜底」，必须打印触发原因到 perf 计数器，且 `perf_budgets.rs` 断言它在正常编辑序列里出现 **0 次**。一旦某条真实操作把它触发出来，就当成 bug 修规则，而不是接受它。

阶段 2 的工作是把第 3 级的范围从「容器链」收窄到「真正的 resync 窗口」，而不是第一次引入回退。

**与 s1 的分界线在这里**：判据是「**这次编辑产生的文本里有没有某个 token**」——局部、显式、可在调用点直接测；不是「这一块到底算不算被改过」——全局、靠内容比较推断、必然有盲区。

**区域重解析的最小规则**（阶段 2，规则要保守）：
- 受影响行窗口 = 编辑覆盖的行 ∪ 该窗口所在的最外层容器（列表/引用）的兄弟链。
- 重解析起点 = 窗口前最近的「安全 resync 点」：一个空行，且不在代码围栏/front matter 内，且其后第一个非空行的块类型与窗口前块类型不构成延续（脚注定义、懒续行、pipeless 表格都要显式排除）。
- 重解析终点 = 下一个同类安全 resync 点；找不到就扩到该容器链结束（**不许无限扩到整篇**，扩到整篇是 s1 的失败模式）。
- 每个规则配一个 golden 测试；漏判的表现是「编辑一处、远处块的结构变了」，这类测试必须成对写（改 A，断言 B 的 span 与内容不变）。

### 5.4 光标 / 选区 / 撤销的一致性

- **位置类型统一为 Anchor**：`CrossBlockSelection { anchor, focus }` 现在持 `{entity_id, offset}`（selection.rs），改为 `{Anchor, Bias}`；块内 `selected_range` 保留（块身份仍在，用于渲染与焦点），但**跨块/文档级位置不再用 entity_id**。
- 焦点块的caret：仍是块内偏移 + 现有 `CollapsedCaretAffinity`（marker 展开态下的位置亲和性），块被重投影时按 `Anchor + 块内偏移` 还原；块被合并/分裂时按 anchor 落在哪个 span 决定。
- **撤销**：`HistoryEntry { source_text }` → `EditGroup { edits: Vec<AppliedEdit>, selection: Vec<AnchorRange> }`。撤销 = 逆序 apply 逆操作 + 重投影受影响区域，不再整树重建（`restore_history_entry` 的 `build_root_blocks_from_markdown` 调用消失）。合并窗口（1s）、`HISTORY_LIMIT`、IME 组合的 `ImeComposition`/`Commit` 分类保留。
- **源码模式**：现在是「整篇塌成一个 `EditMode::SourceRaw` 的段落块」（window_state.rs 切换逻辑），且和渲染模式共享同一棵树、序列化路径分叉。改为：**源码模式 = 直接编辑 buffer 的视图**，不建块树（或仅缓存）；切回渲染模式 = 从 buffer 投影。`raw_source_text` 与 `history.rs` 里那套 source-offset 选区换算一起删。
  - 注意：源码模式的虚拟滚动（只渲染可见行窗口）是独立工作项，见 §10 待确认第 3 条。

### 5.5 搜索与跳转（验收 2）

```rust
trait TextView { fn bytes(&self) -> &[u8]; fn line_of(&self, byte: usize) -> usize; }
// 两个实现：TextBufferView（打开的文档）｜FileBytesView（内存映射/缓存的文件字节）
fn scan(tv: &dyn TextView, query: &str, scope: Range<usize>) -> Vec<Hit>;  // Hit{ byte_range, line, col }
```
- **一套扫描器，两个数据源**。工作区搜索：打开的标签用 buffer（未保存编辑立刻可见，`match_ordinal` 与 active-document overlay 这些补丁全部删除），未打开的文件用字节视图。全局 `path→content` LRU 的失效条件加「有打开的标签则不用缓存」。
- **跳转**：`Hit.byte_range` → 在按 span 排序的块区间上二分（O(log n)）→ 得到块 + 块内偏移 → 若该块未物化（分块导入还在尾部）则先物化到该处 → 展开折叠（现有 `unfold_sections_covering_source_range` 保留但输入换成字节区间）→ 选区 = `content_to_source` 的直接结果 → 滚动居中（`ensure_focused_caret_visible` 保留）。
  - 命中落在块**前缀**（如查询 `#` 命中了标题标记）时：选区钳到该块内容起点，块本身必须被选中高亮——这是唯一需要「钳」的情形，且语义明确；今天那堆字符边界钳制兜底是偏移漂移的副产品，删掉。
  - 表格：命中在表格源文本上，但表格无块级映射（`source_mapping.rs` 里对表格是特判跳过、跳转时重新锚回宿主表格）。新模型下表格 span 是完整的，cell 有字节区间（阶段 3 做），未做之前至少锚到表格块并可高亮整行。
- **大纲**：扫 buffer 行索引；修掉 front matter 里的 `#` 被当标题；节点 id 从 `outline:{行号}` 改为 `{Anchor}`，点击跳转不再需要「行号 → 字节 → 块」三段换算。
- **文内查找替换**：`find_replace.rs` 现在把全文重新序列化进 `document_search_source`，替换 = 「选中该块再 `replace_text_in_range`」。改为直接 `buffer.edit` + 重投影；`replace_all` 的「从后往前」技巧可以保留（它本来就是对的），但不再依赖块身份。

---

## 6. 分阶段迁移计划

每阶段结束都必须：`cargo test` 全绿 + 该阶段闸门达标。阶段之间不留双轨——阶段 1 完成时，buffer 已经是事实源。

### 阶段 0：把验收标准变成红测试（不碰架构，约半天）
- 建 `fixtures/round_trip/`：无尾换行、CRLF、紧凑相邻块（围栏后紧跟 `---`）、表格对齐填充、Setext 标题、`1)` 列表、中文/emoji 混排、front matter、真实长文。
- 写 `open → 不编辑 → save → 字节必须相同` 的表驱动测试（当前仓库**没有任何**真正的字节保真回环测试：`import_perf.rs`、`knowledge_recovery.rs`、`save_autosave_ime.rs` 那几处都是「与 `markdown_text` 比」的同义反复）。
- 闸门：这批测试现在是红的，失败清单 = 重构范围确认单。

### 阶段 1：buffer 落地 + 一切读取换源（最硬的主体）
1. `TextBuffer` + Fenwick + Anchor + FileShape，带独立单元测试（不含编辑器）。
2. 打开流程建 buffer；导入器输出每根块的 `SourceRegion`。
3. 保存改为写 buffer（含 pristine 快路径）；块内与表格单元格编辑走**区间级写回**（§5.3）：`source ↔ visible` 表重锚到源码空间，表格解析顺手记 cell 字节区间。拆分/合并/粘贴按 §5.3 回退层级 2 走 span 算术。
4. **撤销同期改 `EditGroup`** —— 这条不可与写回分期：写回既然产出 `AppliedEdit`，历史就是把它们的逆操作收集起来；而「每键 clone 全文 + 全文序列化再比较」正是那 13 秒的另一半，留着它，阶段 1 的保真收益会被性能回退抵掉。合并窗口（1s）、`HISTORY_LIMIT`、IME 的 `ImeComposition`/`Commit` 分类语义不变。
5. 读取侧全部换源：工作区搜索、文内查找、大纲、状态栏行列号、字数统计、链接索引。删除 `source_mapping.rs` 的整篇反推、`match_ordinal`、`document_search_source`、大纲的两份快照不一致。
- 闸门：验收 1（阶段 0 那批全绿）、验收 3（区间级写回，编辑处以外的字节一律不变，**含被编辑块自身未碰过的定界符**）、验收 2 的搜索/跳转部分（真实中文+表格+front matter 文档逐个人工验证）、验收 4（迁移后的测试全绿）、**10 MiB 单键预算达标**（`perf_budgets.rs` 夹具从 1 MiB 提回 10 MiB）。
- 可发布：用户从此不会再说「打开没动、保存被改写」。

### 阶段 2：增量重投影 + 位置换制
1. 把 §5.3 第 3 级回退的范围从「容器链」收窄成真正的**区域重解析**（最小规则）+ 成对 golden 测试。
2. 跨块编辑（拖拽选区删除/全选删除/粘贴多行）的选区端点改 `Anchor`，块身份不再被丢弃。
3. 源码模式改为 buffer 视图（删 `raw_source_text` 与「整篇塌成一块」，删 `history.rs` 的 source-offset 选区换算）。
4. 性能预算补齐：「每键全文遍历次数 = 0」「200 步撤销内存 ≤ 8 MB」「第 4 级回退出现 0 次」。
- 闸门：键盘导航 / IME / 表格 / 拖拽选区 / 折叠 相关测试全绿 + 新的 10 MiB 预算达标。

### 阶段 3：收尾
1. 表格的**结构**操作（行列增删、调对齐即改 `|---|` 行）改为 buffer 编辑；表格内搜索命中精确到 cell 高亮。（单元格文本编辑与 cell 字节区间已在阶段 1。）
2. 显式 `格式化文档` 命令：把旧的根块空行/表格重排规则搬到这里（可作为可选项，用户不签字就不做）。
3. 编码/EOL 全矩阵测试（UTF-8/GB18030/BOM/CRLF/无末行换行 × 打开即保存 / 编辑后保存）。
4. 删除残留序列化路径与 `last_stable_source_text`；重写 `docs/architecture/editor-core.md` §1/§3/§4/§5（现在那几节描述的就是被换掉的模型）。

---

## 7. 模块清单：保留 / 重写 / 删除

| 处置 | 模块 | 说明 |
|---|---|---|
| **新增** | `src/editor/buffer/`（`TextBuffer`、`Fenwick`、`Anchor`、`FileShape`） | 独立可测，不依赖 GPUI 实体 |
| **保留（改职责）** | `tree.rs:DocumentTree` | `roots`/可见快照/折叠/`with_structure_mutation` 都留；从「事实源」降为「投影容器」 |
| **保留** | `Block`/`BlockRecord`、`components/block/runtime/` | 渲染、光标、IME、折叠是真实需求；`title` 语义从「内容」变「`parse(buffer[span])` 的缓存」 |
| **保留** | `document/parse.rs`、`document/blocks.rs`、`import.rs` 的手写行扫描器 | 它本来就是行区间推进，天然适配；只加「输出每块行区间」和「窗口重解析」 |
| **保留** | `InlineRenderCache`/`BlockTextElement`、块内 `visible↔markdown` map、`cursor.rs` | 块内有界，成本合理；成为写回通道 |
| **保留（换内容源）** | `persistence.rs` 的 `write_atomic`/`verify_file_version`/autosave/close 流、`watcher.rs` | 骨架不动，字符串来源换成 buffer |
| **重写** | mod.rs 文档生命周期（`from_markdown_with_chunk_budget`/`from_file_source`）、`file_drop.rs:replace_document_content` | 先 buffer 后投影；`PendingTail` 指向 buffer 尾部 |
| **重写** | `history.rs` | 全文快照 → `EditGroup`；`restore_history_entry` 不再整树重建；删 source-offset 选区换算 |
| **重写** | `selection.rs` | 跨块端点用 Anchor；`rebuild_after_cross_block_source_edit` 从「全文重解析」变「区域重解析」 |
| **重写** | `search_backend.rs`、`render_search.rs`、`find_replace.rs`、`tree_ops.rs`、`tree_sync.rs` 大纲部分 | `TextView` 抽象 + 字节区间命中 + 二分定位 |
| **重写** | `table_edit.rs` / `TableData` | 加 cell→字节区间；单元格编辑走 buffer |
| **重写** | `ViewMode::Source` | 「整篇塌成一块」→ buffer 视图 |
| **删除** | `tree.rs:markdown_text_with_block_spans`、`collect_root_markdown_lines` 在保存/映射路径上的使用 | span 改由解析记录；序列化只服务导出/格式化 |
| **删除** | `source_mapping.rs` 的 `build_source_target_mappings` / `collect_single_block_source_mappings` / `content_to_source`+`source_to_content` 表 / 引用块与围栏与表格的前缀重建 / 字符边界钳制兜底 | 31 KB 里的绝大部分（约 700 行） |
| **删除** | `render_search.rs:open_search_hit` 的 `match_ordinal`、`find_replace.rs:document_search_source`、`last_stable_source_text`、`tree.rs:raw_source_text` | 猜测式补丁；buffer 下无需 |
| **删除** | 隐式规范化（根块间强制空行、表格重排、Setext→ATX、`1)`→`1.`、CRLF 抹平、末行换行吞掉、非 UTF-8 转 UTF-8） | 只在显式 `格式化文档` 命令里保留（阶段 3 可选） |
| **删除（改订阅）** | `workspace_index.rs` 的 active-document overlay + `LinkPanelState` 500ms 防抖 | 改为订阅 buffer revision |
| **不作依据** | `s1-sticky-source` 分支 | 只作为「不要读时二选一」的教训来源 |

---

## 8. 测试策略

- **总量**：542 `#[test]` + 547 `#[gpui::test]` = 1089，全内联单测，`tests/` 只有夹具。19 个 criterion bench。
- **要迁移的**：`markdown_text(cx)` 出现 190 次（约 185 次在测试里，31 个文件）。断言规范化输出的约 **120–150 个测试**。分类处理：
  1. **文档级「导出 == 规范化结果」** → 改为「导出 == 输入原文」（`import_round_trip.rs` 的 Setext/缩进代码/`1)`/`>` 断言是最典型的一批，`import_round_trip.rs:29/54/251/326/397/420/477`）。约 42 处期望串含 `\n\n` 的空行分组断言属此类。
  2. **块级序列化行为**（`BlockRecord::markdown_line` 该不该输出 `# `、围栏标记长度、列表标记选择）→ 下沉成块级单测，断言不动，但改叫「块级 markdown 生成」而不是「保存结果」。
  3. **表格 `| --- |` 断言约 75 处**（18 个文件）：区间级写回下，改单元格是字节 splice，`| --- |` 与列宽填充**永远保持原样**，所以这批「期望表格被重新排版」的断言全部落入第 1 类（改为等于输入原文）。只有显式的格式化命令或行列增删/调对齐会重排表格，为这两条路径另写测试。
  4. `canonical_markdown` 局部函数 19 处（集中在 `quotes_callouts.rs`）：按 1 处理。
- **必须新增的**：阶段 0 的字节保真表驱动套件；§5.3 每条区域重解析规则的成对 golden 测试；`TextBuffer` 自己的单元测试（Fenwick/Anchor/分块 split-merge/char 边界）；10 MiB 按键预算。
- **性能预算**：`perf_budgets.rs` 现有断言是「每键 ≤1 次全文序列化、≤2 次 mapping 重建、静止帧 0 次全文工作」（`perf_budgets.rs:200-214`）。新模型下应改为「每键 **0** 次全文级操作」并把夹具从 1 MiB 提回 10 MiB——注释里写着 10 MiB 单键 13 s 所以降到 1 MiB，这条改完正好是重构效果最直接的度量。

---

## 9. 风险登记册

| # | 风险 | 影响 | 对策 |
|---|---|---|---|
| R1 | 区域重解析的 resync 判定漏判（脚注定义、懒续行、pipeless 表格、fenced div、front matter） | 编辑一处、远处块结构变化；最难查 | 常态路径根本不重解析（§5.3 回退层级 1–2 覆盖日常所有编辑）；第 3 级宁保守勿激进 + 成对 golden 测试 + debug_assert 校验「投影 == 全量解析」（小文档每次成立，10 MiB 才关）。**逃生门**：仓库已经依赖 `tree-sitter` + `tree-sitter-md`（`code-highlight-official` feature），它的 `Parser::edit` 增量重解析正是「编辑后自动平移字节位置、只重解析受影响区」的现成答案——若手写 resync 规则反复漏判，把第 3 级的**判定**交给 tree-sitter-md（Block 实体仍我们自己构造），R1 从「我们判定」变成「解析器保证」 |
| R2 | 过渡期出现四套偏移空间（干净可见 / 含标记显示 / 序列化 markdown / 磁盘 source）与 Anchor 交叉 | 光标错位、IME 候选框位置错、写回切错字节 | **不并存**：写回与光标一律走 `source ↔ visible`，序列化空间只留在导出/格式化路径且不允许出现在编辑路径；投影只在焦点块激活；文档级位置一律 Anchor；`CollapsedCaretAffinity` 保持块内 |
| R3 | 150 个测试逐个改判断成本高，容易被「先保绿」批量 skip 诱惑 | 失去唯一回归网 | 按类批量迁移 + 每类一个抽样人工核对；禁止 skip（阶段闸门要求全绿） |
| R4 | 双轨窗口期（阶段 1 内块树与 buffer 并存）重演 s1 失败 | 架构返工 | 铁律 2：写入方向唯一；块→buffer 只有「span 替换」一个窄口，窄口后立即回读校验 |
| R5 | GPUI 的 UTF-16 边界与分块文本的字节偏移换算成本 | 大文档卡顿 | 行/块级缓存 utf16 长度；`range_from_utf16` 改为按 chunk 定位 |
| R6 | 撤销从「全文快照」改「delta 逆操作」后，历史与重投影交互出新 bug | 撤销后光标错位、结构没还原回来 | **不可延后**：撤销与写回同源（`AppliedEdit` 顺手就是逆操作），延到阶段 2 等于把 13 秒留在主干。对策：`EditGroup` 应用逆操作后 `debug_assert` 「投影 == 直接重新解析 buffer」；合并窗口 1s、`HISTORY_LIMIT=200`、IME 的 Composition/Commit 分类全部沿用现有语义，只换存储不换规则 |
| R7 | autosave/recovery/关闭流对 `serialized_document_text` 的隐式依赖（多处调用） | 保存错内容或丢恢复 | grep 全部调用点列成清单再改（`persistence.rs`、`tabs.rs:588`、`session_watcher.rs:159`、`file_drop.rs:460`） |
| R8 | `source ↔ visible` 表重锚做错（拿序列化下标去切源码字节） | 写回错位、切进多字节字符中间（历史上出过 coredump） | 重锚表独立单元测试；断言「在块内插一个字符 ⇒ 该块 source 区间只增长这一字符，块内其余字节逐字节相同」；buffer 侧强制 `floor_char_boundary`；表格 cell 的字节区间由切 `\|` 时记录，不用 `+2/+3` 算 |

---

## 10. 决策记录与待确认

**已定**（2026-10-02 对话确认）：

1. 事实源落点：内存 `TextBuffer` 严格镜像文件，保存时才写盘（不做写穿磁盘）。
2. 交付节奏：本方案签字后才动代码，含阶段 0 的红测试。
3. 范围：三阶段，每阶段可发布、全绿再进下一阶段。
4. 旧测试：按新语义改写为「未编辑导出 == 原文」；确属块级序列化行为的断言下沉为块级单测。
5. **阶段 1 就做区间级写回**——原先准备的「阶段 1 块级粗写回 + 阶段 2 再细化」划分**作废**。理由：定界符不进模型（`1)` vs `1.`、`__粗__` vs `**粗**`、表格列宽），所以「从块树重新生成这一块」无论粒度多粗都必然改写用户没碰过的写法，直接违反验收 3。写回粒度不是可以分期偿还的债，它是保真的前提。
6. 非 UTF-8 文件编辑后保存：本轮不做编码变更提示。未编辑 → 原样写回原字节；已编辑 → 按 `FileShape.encoding` 写回。
7. 表格的 cell 字节区间提前到阶段 1（切 `|` 时顺手记），因此「改一个单元格、整张表重排」不再有过渡期。

**待你确认的最后一条**：

- **源码模式虚拟滚动**：把源码模式改成 buffer 视图顺带能解掉 10 MiB 纯文本「单块 10 MB」的老瓶颈（`editor-core.md` §2 末），但让它可流畅滚动是另一件独立工作（需要按行窗口渲染）。本方案只承诺「源码模式编辑 = 直接编辑 buffer」，不承诺大文档流畅滚动——确认这个边界，或把它并入阶段 3。

---

## 11. 性能账（量级估算，阶段 0 会把它变成实测）

| 操作 | 今天 | 本方案 |
|---|---|---|
| 1 MiB 文档一次按键 | 1 次全文序列化 + 1 次全文 clone + 1 次字符串比较 + ≤2 次 mapping 重建 | 2 次 Fenwick 查询（~14 步加法）+ 定位并改写 ≤2 个 chunk（memcpy ≤ 8 KiB）+ `Vec<Chunk>` 头部 memmove（~80 KB）+ **一个块**的 inline 重解析 |
| 10 MiB 文档一次按键 | **实测 13 秒**（`perf_budgets.rs:59` 注释，因此预算表被降到 1 MiB） | 几十微秒量级，且与文档大小近乎无关（唯一相关项是那个 80 KB 的头部 memmove） |
| 撤销栈内存 | 最坏 200 × 全文 = **10 MiB 文档约 2 GB** | 与编辑过的文本量成正比（KB–MB 级） |
| 打开 10 MiB | 导入 ≤3 s（分块流式） | 同上 + 一趟行索引（数 `\n`，几毫秒，可随分块惰性建） |
| 全文搜索 10 MiB | 先全文序列化，再建 mapping，再扫 | 直接一次字节扫描（量级 5–15 ms），无前置成本、无陈旧快照 |
| 静止帧 | 已断言 0 次全文级工作（`perf_budgets.rs:214`） | 同，且新增第 3 条铁律：paint 不碰 buffer |
| 新增常驻内存 | — | chunk 头 + 两棵 Fenwick ≈ 10 MiB 文档 120 KB；每块 `SourceRegion` ≈ 48 B × 20 万块 ≈ 10 MB——但现有 `Block` 实体（UUID + `Vec<InlineFragment>` + 渲染缓存）单个就远超 48 B，所以 span 表是块树自身开销的零头 |

**不会变差、但也不会被这次修好的**：单个巨大块（10 MB 日志 = 1 块 10 MB 文本，`editor-core.md:32` 明写这是结构性瓶颈）。重投影成本是 O(块大小)，与今天同级。真解决要靠「代码块/长段落也按行窗口投影」——文本进了 buffer 之后这件事反而变容易（可以直接从 buffer 取行窗口喂给 `BlockTextElement`，不必物化整块 `display_text`），列为阶段 3 可选。

**唯一真实的新增性能风险**：§5.3 的第 3/4 级回退被误触发。已经钉住：回退次数进 perf 计数器，`perf_budgets.rs` 断言正常编辑序列里第 4 级出现 0 次、第 3 次出现次数 == 触发它的操作用例数。触发不出来就当规则没漏，触发出来就修规则，不接受「反正只是偶尔全篇重解析」。

## 12. 业界是怎么做的（为什么方向不是我们拍的）

| 工具 | 事实源 | 保真表现 | 对 Velora 的含义 |
|---|---|---|---|
| **Typora**（闭源，按公开资料与可观察行为推断） | 文档模型 / DOM 是事实源，保存时**导出** markdown | 会重排用户的 markdown：列表标记统一、`_x_`→`*x*`、表格重排、补空行——**和你现在一模一样**；对不认识的构造整块保留源码兜底（正是 s1 的思路，同样有盲区） | 「Typora 也这样」不是理由：它的保真问题就是社区长期抱怨的那批。你要的是另一种架构 |
| **Obsidian / MarkText / VS Code 生态**（CodeMirror 6） | **纯文本 buffer 是唯一事实源**（CM6 `Text` = piece tree），markdown 由 Lezer 语法树当**投影**，"所见即所得" 靠 decoration 把定界符淡化/隐藏 | 字节保真是架构自带的性质，不是补丁 | 这就是本方案的目标形态。它不需要「块身份」，编辑始终是文本编辑 |
| **Zed** | `Buffer` = `SumTree<Chunk>`（rope）+ `TextAnchor` + CRDT 操作 | 同上 | 依赖树里的 `gpui_sum_tree` 就是它的底座。§3 自研的 Fenwick 版是同一形状**去掉协同编辑**的最小子集，将来要长回去有路 |
| **remark / mdast**（生态标准） | AST 节点持 `position{start,end}{line,column,offset}` | 偏移在解析时记录，不反推 | §4 的 `SourceRegion` 就是这个约定 |
| **tree-sitter** | 增量解析：`Parser::edit` 让旧树的字节位置随编辑自动平移，只重解析受影响区 | 工业级答案 | Velora 已依赖 `tree-sitter` + `tree-sitter-md`。它是 §5.3 第 3 级判定现成的逃生门（R1） |

一句话：**「文本 buffer 为事实源 + 语法树为投影 + 定界符靠装饰隐藏」是这一类产品的主流做法**；「文档模型为事实源 + 保存时导出」是 Typora 路线，而它带来的字节改写正是这次重构要消灭的东西。

---

## 13. 阶段 0 实测失败清单（2026-10-02，分支 `s2-buffer-source-of-truth`）

套件：`src/editor/tests/round_trip_fidelity.rs::opening_then_saving_without_edit_preserves_every_byte`
（27 个夹具，走真实打开漏斗：读字节 → `encoding::read_document_string` → `Editor::from_file_source` → ctrl-s 保存 → 比字节）

**结果：24 / 27 失败。** 通过的只有「无末行换行的单段文档」「多个末行换行」「空文件」。按类别归纳：

| 类别 | 实测行为 | 严重度 |
|---|---|---|
| 末行换行被吞 | 凡以 `\n` 结尾的文件，保存后少一个字节（`front matter`、`中文与 emoji 混排`、`脚注定义`、`fenced div`、`展示数学`、`转义字符`、`链接引用式`…全部命中） | 高（最常见） |
| CRLF 丢失 | `# 标题\r\n…` → `\n`，且末行换行同时丢 | 高 |
| 空白内容被吞 | `"   \n\n\t\n"` → `"\n\n\n"`：**行内的空格/制表符内容直接消失** | 高（真数据丢失，不只是风格） |
| 硬换行被改坏 | `"第一行\\\n第二行"` → `"第一行\\\\\n第二行"`：反斜杠硬换行变成字面反斜杠 | 高（**语义错误**，不只是写法） |
| 根块之间补空行 | `段落一\n## 紧跟的标题` → 中间插空行；围栏后紧跟 `---` 被隔开 | 中 |
| 表格重新排版 | 对齐填充 `179 → 102` 字节；`|a|b|` → `| a | b |` | 中 |
| Setext → ATX | `一级标题\n========` → `# 一级标题`（并顺手把二级 Setext 也改了） | 中 |
| 列表标记归一 | `1)` → `1.`；`* 星号`/`+ 加号` → `- 星号`/`- 加号`（**混合标记被统一**） | 中 |
| 定界符归一 | `__粗__` → `**粗**`，`_斜_` → `*斜*` | 中 |
| 围栏改写 | `~~~~rust title="示例"` → ` ```rust title="示例"` | 中 |
| 缩进代码 → 围栏 | 4 空格缩进代码块变成围栏，并额外插空行 | 中 |
| 引用前缀归一 | `>引用二` → `> 引用二` | 低 |

这张表就是重构范围确认单：上面每一行在阶段 1 结束后都必须变成绿，且这套用例**一字不改**（它们只断言磁盘字节，不碰任何内部表示，因此对本次重构的内部决策完全中立）。

---

## 12. 实施记录与偏差（边做边记，2026-10-02）

提交序列：`9478b07` 块编辑按区间写回 → `13e6224` 保存写缓冲区字节 → `e1d12c3` 拆合块按根块片段写回 →
`ec7c5a0` 撤销栈存字节增量 → `1eee96b` 读取侧换源到缓冲区 → `36c4287` 同一文件命中落位守卫。

阶段 1 的主体（保真 + 写回 + 读取）到此完成。实测收益：一次按键的整篇序列化次数从 1 降到 0
（闸门已从 `≤1` 收到 `=0`），测试套件 8.1s → 7.0s，红测试「Setext + 填充表格里的命中」从
报第 5 行变成报文件的第 6 行。

两处与本文原计划的偏差，都是实施时才发现计划的前提不成立：

1. **`match_ordinal` 不删**（本文 §5-5 与 §模块清单里都写了「删除」）。理由：工作区搜索读
   **磁盘**、报磁盘行号是用户定盘的语义（见 `clicking_a_search_hit_in_a_dirty_file_lands_on_the_match`
   的注释），文档有未保存编辑时磁盘行号与缓冲区行号必然错位，这层「文件内第 k 个含词行」对应
   就是为此存在的。它不是给重新序列化擦补丁，读取换源之后仍然需要。已把注释改准。
2. **引用块内的表格仍按序列化推算单元格位置**（`push_inferred_table_mappings`）。根块的表格
   已改成从缓冲区原文量（在原文行里找单元格文本），但区间只挂在根块上，嵌套表格没有自己的
   区间可锚。漂移被限制在单个根块的区间之内。阶段 3 让解析期记下每个单元格的字节区间后删掉。

阶段 1 收尾还剩：`document_search_source` 字段可以去掉（现在读缓冲区只要一次 `clone`），
但它同时充当「结果所属文本」的快照，去掉前要先定「搜索跑完之后又编辑了文档」的语义。
