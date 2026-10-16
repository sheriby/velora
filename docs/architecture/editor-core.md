# Editor Core：文档模型与编辑管线

> 基于 `dev`，2026-10-14 重写。引用用 `文件:函数名`。
> 相关：[overview.md](./overview.md) · [markdown-model.md](./markdown-model.md) · [render-pipeline.md](./render-pipeline.md) · [performance.md](./performance.md)

## 1. 实体层次与所有权

```
Editor 实体（每个窗口一个，src/editor/mod.rs）
├── buffer: TextBuffer            ★ 唯一内容事实源
├── document: DocumentTree        投影：块树 + 区间表 + 可见快照 + 懒导入尾
│    └── roots: Vec<Entity<Block>>
│         └── children: Vec<Entity<Block>>     列表/引用/脚注/callout 的嵌套内容
└── workspace: WorkspaceState     内嵌状态，不是实体（见 workspace-ui.md §2）
```

- **`Editor`** 是窗口级控制器：持有缓冲区、块树、视图模式、脏标记与修订号、undo/redo 栈、文件路径与版本号、引用定义与脚注注册表、行高缓存、行计划，以及全部性能计数器。
- **`Block`**（`src/components/block/runtime/mod.rs`）是 GPUI 实体；**`BlockRecord`**（`src/components/block/state.rs`）是它的持久数据：kind、行内树、表格数据、HTML 文档、raw 原文，以及**解析期记下的写法账**（列表记号、callout 记号、每行让开的字节数、围栏缩进、块前空行字节数）。块文本 `record.title` 的语义是"这块那段字节的解析缓存"。
- **区间表在树侧**：`DocumentTree::source_spans`（`EntityId → Range<usize>`，缓冲区字节坐标）。一次编辑平移整表只是一趟紧凑循环；挂在块实体上时，大文档一次按键要逐个 `read`+`update` 十万个实体。
  区间约定：**不含本块自己的行尾换行**；块之间的空行不归任何块；文档末尾无换行时最后一块右端 = 文档末尾。

分屏通过 `TextBuffer::shared_view` / `shared_handle` 显式取得同一内容的视图句柄，每份视图各自消费脏区。普通 `Clone` 仍为独立内容副本，供撤销试算使用；不能把它改成共享克隆。共享编辑的字节区间日志只供各组更新投影，不保存第二份正文。

## 2. 缓冲区（src/editor/buffer.rs）

- 结构：`Vec<Chunk>`，每块一段文本 + **该段内每个换行的偏移表**。行号换算只二分这张表，**一次都不读正文**（批量换算必须走 `lines_and_line_starts`；逐根块调单点 `line_of` 是平方项）。`byte_len` 必须 O(1)。
- 内容是 **LF 规范化**文本；磁盘上的 CRLF/编码/BOM 记在 `FileShape`（`buffer/file_shape.rs`），保存时还原。
- `edit(range, &str) -> AppliedEdit` 是**唯一写入口**；返回值 `{removed, new_range}` 就是这次改动的逆操作，撤销栈直接存它。
- `pristine`：打开时那份原始字节。**未编辑过的文档 `file_bytes()` 直接返回它**——这是"打开即保存，字节不变"的实现。无法无损写回的形状（`TextEncoding::Lossy`）另有一道闸：`file_bytes()` 永远交回原字节。
- `revision`（每次 edit +1）与 `identity`（进程内唯一）：跨帧缓存键要两个一起用。
- `dirty: Option<Range<usize>>`：自上次取走的改动范围，`take_dirty_region` 供大纲一类增量视图用。
- `text()` 每次复制整篇——只在真需要全文的地方调；判断"有没有变"用 `matches_text`（零拷贝逐块比较）。
- 字符边界：`edit` 端点有 debug_assert，`line_of` 直接断言；来自旧内容的偏移必须先过 `floor_char_boundary`。

## 3. 编码与读盘（src/editor/encoding.rs + buffer/file_shape.rs）

- `load_document(path)` 是**唯一读盘漏斗**，四个打开入口（标签、拖放、菜单/CLI、外部重载）都经过它：一次读出原始字节 + 解码文本 + 形状；判不出来的字节返回 `InvalidData`，**拒绝打开**而不是以乱码打开再写回。
- `FileShape::detect_and_decode` 是**唯一嗅探口**，判定与解码同一趟：UTF-16 BOM → UTF-8 BOM → UTF-8 → GB18030 → Lossy；正文含 NUL 一律 Lossy（无 BOM 的 UTF-16 刻意不猜）。
- `FileShape::encode` 是写回口径：先折平游离 `\r`，再按形状升格 CRLF、贴 BOM、编码。**UTF-16 手拼 code unit**——`encoding_rs` 的 UTF-16 `encode` 不是解码的逆操作。
- `decode_document_bytes`（只解码不拒）给搜索索引一类只读消费者用。

## 4. 导入：文件 → 缓冲区 → 块树

- 解析器是**手写逐行扫描器**，不是 pulldown-cmark（后者只用于 HTML 导出与 ⌘-点击锚点）。识别函数族在 `document/parse.rs`，主循环 `document/import.rs:build_blocks_from_lines_with_lookahead`，各类块的收集与建块在 `document/blocks.rs`。
- 分发顺序（先命中先赢）：frontmatter → 空行段 → 围栏代码 → fenced div → HTML 注释/块 → 脚注定义 → 引用定义 → Setext 标题 → 独立图片 → `$$` 展示公式 → 缩进代码 → 列表 → 引用块/callout → ATX 标题 → 分隔线 → 表格（含无外层竖线的）→ 兜底段落。**改识别顺序会同时改导入与写回口径**，动手前先看往返测试。
- 认不出的构造 → `BlockKind::RawMarkdown`，原文存 `raw_fallback`，逐字保留。整篇都撑不住时（未闭合 fenced div、不支持的 admonition）由 `markdown_lines_require_source_mode_fallback` 判定，降级为单块 + Source 视图。
- **懒导入**：首屏同步建一批根块（`FIRST_CHUNK_ROOTS`），其余行挂 `PendingTail`，前台任务每轮续建（`STEADY_CHUNK_ROOTS`）；文档替换取消旧任务并校验加载代次。需要全文的操作先 `flush_pending_materialization`；区域重投影见 pending 尾就放弃、走全量。续建传入的行数组必须是**全文数组的后缀**（前瞻一致性）。
- **解析期记账**：剥记号的那一刻顺手把"这一行让开了几字节"记进块（`source_line_prefixes`）、把块前空行字节数记进 `source_separator_bytes`、把围栏两行的缩进记进 `source_fence_lines`。这是位置换算与写回共用的第一档数据；**谁重新分配区间，谁就负责重记或作废这份账**（`Editor::reanchor_record_span`）。
- **代码/纯文本路径**：`from_file_source` 按扩展名分流，源码按固定行数切片（`SOURCE_DOCUMENT_CHUNK_LINES`）成 source-raw 块，进 Source 视图。这类块的区间由 `source_slice_span_ranges` 按"块文本长度 + 一个换行"推进，保证 `buffer.slice(span) == block.display_text()`——**该恒等只在块顶格时成立**，容器里的 source-raw 块可见文本是剥掉容器记号那一段。

## 5. 编辑：按键 → 落笔 → 重投影

一次按键的链路：块的 input handler 改 `record.title` → emit `BlockEvent::Changed`（订阅点 `runtime_context.rs:new_block`）→ `events/block_event.rs:on_block_event` → 开撤销组 → 写回分档 → 平移区间 → 必要时区域重投影 → 引用敏感块刷新 → 批处理末尾结算撤销组。

**写回按精度分档，逐档失败才降级**（每档返回 bool）：

| 档 | 入口 | 适用 |
|---|---|---|
| 纯插入 | `mod.rs:write_back_visible_insertion` | 打字：在已知字节点插一段，条件全过才落笔（插入文本序列化不变样、落点在根块区间内且是字符边界、插入点同行前文与原文一致） |
| 表格 | `table_edit.rs:write_back_table_cell_source` 与 `write_back_table_{row,column}_*` | 格子打字、加删行列、调对齐、移动行列、删表头——都按字节区间落笔，从后往前写 |
| 单块 | `mod.rs:write_back_block_source` | 整根块内容变了，只重写它自己的区间（经 `write_minimal_diff` 取公共前后缀，撤销组因此不存整块副本） |
| 区段 | `mod.rs:write_back_root_region`（结构命令走 `write_back_structural_change`） | 根块序列变了（一分为二、两块合一），只重写被换掉的那一段；**Source 视图直接拒绝**（接缝口径不同：渲染态是"根块之间空一行"，源码视图是"隔一个换行"） |
| 换种类 / 插入块 | `paragraph_ops.rs:write_back_block_kind_region`、`insert_ops.rs:write_inserted_block_bytes` | 段落转换与插入命令；接缝归前一块，邻居写法与文件一致才并入 |
| 兜底 | `mod.rs:resync_buffer_from_projection` | 从块树整篇重序列化（渲染态 `markdown_text_with_block_spans`，源码态 `raw_source_text`）+ 重挂区间。**最后手段**：未编辑块的原始字节会在这里被洗掉，正常编辑路径必须走不到它 |

- 任一档成功落笔后调 `window_state.rs:mark_dirty_written_back`，置一次性标志 `skip_next_resync`，让本轮 `Changed` 不再触发兜底。写不进任何档的改动走 `mark_dirty`（程序化重同步）。
- **区间平移的顺序是不变量**：先 `shift_root_spans_after`（用**编辑前**坐标）平移别人的，再写自己这一根的 span——反过来零宽区间（回车新建的空块）会被二次平移。
- **落笔前的守卫**：span 末端在界内且是字符边界；块必须**在树里**（幽灵块的旧区间会复活已删内容）。
- **区域重投影**：`document/import.rs:reproject_root_region` 把一根块换成它那段行重新解析出的若干根块（窗口 = 本段行 + 少量前瞻，要求解析结果正好落在本段内才接受，否则退整篇 `rebuild_root_blocks_from_buffer`）。
- **引用定义/脚注注册表从 `buffer.text()` 解析，且刷新必须排在写回之后**，否则读到的是改动前的文本（`runtime_context.rs:changed_block_needs_runtime_context_refresh` 判定哪些块需要）。
- **闸门**：`a_real_editing_session_never_falls_back_to_whole_document_serialization` 把打字、拆块、勾任务框、缩进、标注里拆块、表格全套结构命令、多行粘贴逐条走一遍，断言整篇落笔计数增量为 0；白名单常量 `WHOLE_DOCUMENT_RESYNC_STILL_ALLOWED` 是空表，每加一条命令只能让它更短。

## 6. 位置换算（src/editor/source_mapping.rs）

块内偏移 ↔ 缓冲区字节的双向表 `SourceTargetMapping`，按根块构建，每根锚在自己的 `source_span.start`。构建入口按范围收窄：`source_mapping_for_entity`（单块）、`source_mappings_in_range`（相交根块 + 左右各紧邻一根）、`block_id_at_source_offset`（先问根块区间，进容器才重建那一根）；只有窗口内一根有区间的块都没有时才退回整篇。

**记号宽度有三档退路**（改一族形状不必动消费者）：

1. **解析期账**（`recorded_block_line_prefixes` 一族）：问块上记下的数据。常规路径。
2. **按文件量**（`measured_block_prefix` / `measured_*_line_prefixes` 一族）：用解析器自己的剥记号函数，从文件那一行现量。只在账缺失或过期时用；量不到（起点不是行首、落在多字节字符中间、记号与块类对不上）就退下一档。
3. **按模型拼**：按 kind 与嵌套深度拼记号。最后的退路，也是"洗写法"的来源。

迁移进度看两个计数器 `line_prefix_from_record` / `line_prefix_measured`：常规编辑序列上后者应为 0。

**两套消费口径，不能混**（都在 `selection.rs`）：

- `mapping_source_offset`：**插入点**。第 0 个可见字符也按内容换算（否则字会落进 `# `、`- `、`> ` 记号里）；`offset >= 可见长度` 时交内容在文件里真正结束的那一位，而不是区间末尾（区间可能压着闭合 `#`、围栏闭合行、行尾空格）。
- `mapping_source_range_endpoint`：**区间端点**。端点 0 就是整块区间起点（含记号）。

跨块选区、搜索命中、大纲跳转、状态栏行列号、光标历史一律用**缓冲区字节坐标**，不要再引入"序列化文本里的偏移"。跨块复制交出去的是 `buffer.slice(选区源区间)`——按块树重拼会丢 Setext 下划线、洗掉用户选的写法。

表格格子按"第几行第几列"从原文的管道符之间量（`table_edit.rs:cell_content_range_in_line`、`source_mapping.rs:push_table_row_mappings`），**不拿格子文本回原文搜**（用户刚打的字还没进文件）。读写两侧共用同一把尺：转义的 `\|` 不算列分隔、外层管道符两侧的空槽不算格、容器里那一行要先让开容器记号。

## 7. Undo / Redo（src/editor/history.rs）

- **增量组，不是全文快照**：`HistoryEntry { edits: Vec<AppliedEdit>, selection, timestamp, kind }`。撤销 = 从后往前把每条的 `new_range` 换回 `removed`；重做镜像。栈深 `HISTORY_LIMIT`，合并窗口 `HISTORY_COALESCE_WINDOW` 只并 `CoalescibleText`；IME 组合各自成步；`NonCoalescible` 永远单独一步。
- `prepare_undo_capture` 在编辑**前**开组（只记选区，不拷文本）；每次 `buffer.edit` 的返回值经 `record_buffer_edit` 落进这组；`finalize_pending_undo_capture` 结算入栈。`history_group_is_noop` 在副本上真重放一遍判断"改了等于没改"，那种组不入栈。批处理末尾结算用 `cx.defer`（一次动作的多个事件落同一组）。
- **记账不变式**：用户可见的 edit 必被某组记账；无组到达的合法来源只有撤销重放（`history_restore_in_progress`）与 `mark_dirty` 的程序化重同步。新增写入路径必须先开组。
- 撤销后：缓冲区已是目标状态，置 `skip_next_resync`，投影按 `rebuild_document_from_buffer` **从缓冲区重新解析**（不是从块树序列化），再把选区快照放回。快照偏移属于上一份内容，入口两端先 `floor_char_boundary`；Source 视图把选区**按块切段**发给每个被盖住的投影块。
- 行高度量与折叠态按块源码签名迁移（`capture/restore_history_row_layouts`），撤销首帧不塌成最低行高。

## 8. 规范化只住一条命令

`window_state.rs:format_document`（命令 id `format_document`）是**唯一**做写法规范化的地方：Setext→ATX、`__粗__`→`**粗**`、`1)`→`1.`、表格列宽重排、根块之间补分隔空行。三步：① 清掉模型里"用户自己选的记号"数据（`DocumentTree::canonicalize_writing_style`；**转义不清**，反斜杠转义是语义不是写法）；② 按默认写法序列化并与缓冲区比对，没变就什么都不做；③ 变了走一次最小差异写入再重建投影——第三步必须做，否则撤销把字节放回去了而树里还留着默认记号，再打一个字又被覆盖。

这条命令自己付一次整篇序列化（计数器数得到，有意不豁免），但它不出现在按键路径上。

## 9. 陷阱清单

- 结构编辑与全文扫描前先 `flush_pending_materialization`；懒导入未完时区域重投影与大纲增量都直接放弃。
- `write_back_root_region` 之后要调 `note_outline_dirty_region`：块自己的字节可以一个字没动却指着另一段字节，只看脏区间会漏。
- 脚注序号是**渲染形状**不是字节形状：缓冲区里始终是 `[^id]`，可见文本按注册表贴成上标；回填要排在写回之后。
- 换出代码块的所有入口必须走 `BlockRecord::clear_code_family_books`（清围栏账与缩进标记），否则下次序列化把正文包回围栏。
- 认"这是缩进代码块还是围栏代码块"靠解析期记的 `code_is_indented`，**不靠账在不在**——账会因为重新落笔而缺。
- 块树要跟着视图模式建：源码视图的块是行切片，渲染态的块是解析结果。只换 `view_mode` 不重建树，位置换算会拿非行首的块起点去问行号，`line_of` 直接断言失败。
- 同一实体不能边 `read` 边 `update`；闭包里的内层 `cx` 必须用它自己那个。

## 10. 测试落点

- 字节保真：`editor/tests/round_trip_fidelity.rs`、`block_source_write_back.rs`、`block_source_spans.rs`（打开—编辑—保存后未改动的字节逐字节还原；区间仍铺满全文）。
- 位置与读侧：`editor/tests/source_mapping_fidelity.rs`、`read_side_sees_the_file.rs`（行列号、块偏移落在文件真字节上）。
- 撤销：`editor/tests/undo_history_deltas.rs`、`undo_selection_capture.rs`（含内存闸门：200 步不随文档大小增长）。
- 加载与懒导入：`editor/tests/loading_chunks.rs`、`import_perf.rs`（分块导入与单遍导入逐字节等价）。
- 保存事务：`editor/tests/save_transactions.rs`。
- 性能与形状闸门：`editor/tests/perf_budgets.rs`（见 performance.md）。
- 作者语料整篇验收：`editor/tests/author_report.rs`（测试名按报修条目编号）+ `fixtures/regressions/`。
