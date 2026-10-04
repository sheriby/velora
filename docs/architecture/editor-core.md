# Editor Core（文档模型与编辑管线）

> 面向后续 agent 的代码导览。初稿 2026-09-28（`perf` 分支）；**2026-10-03 按「buffer 为唯一
> 事实源」重构改写 §1/§2/§3/§4/§5/§7**，分支 `s2-buffer-source-of-truth`，设计记录见
> [../plans/2026-10-02-buffer-as-source-of-truth-refactor.md](../plans/2026-10-02-buffer-as-source-of-truth-refactor.md)。
> **行号会漂移，函数名不会**——引用以 `文件:函数名` 为主，行号仅作当时定位参考。
> 相关文档：[overview.md](./overview.md)、[render-pipeline.md](./render-pipeline.md)、[workspace-ui.md](./workspace-ui.md)、[testing-and-build.md](./testing-and-build.md)

## 1. 实体层次

```
Workspace (src/editor/workspace.rs)
  └─ Editor 实体（每个标签一个, src/editor/mod.rs struct Editor）
       ├─ buffer: TextBuffer (src/editor/buffer.rs)   ← 文档唯一事实源：文件里的那份文本
       └─ document: DocumentTree (src/editor/tree.rs) ← 投影（渲染用的块树），不是事实源
            └─ roots: Vec<Entity<Block>>              ← 顶层块，每块记自己占的字节区间
                 └─ children: Vec<Entity<Block>>      ← 列表/引用的嵌套内容
```

- **`Editor`**（src/editor/mod.rs `struct Editor`）：窗口级控制器。持有 `buffer`、`view_mode`
  （`Rendered`/`Source`）、`document_dirty`/`document_revision`（脏标记与修订号，缓存的 key）、
  `file_path`/`file_version`（外部修改检测）、undo/redo 栈（`Vec<HistoryEntry>`，存增量，见 §4）、
  `skip_next_resync`（一次性标记：「这一步已经按区间落笔了，别让整篇重投影把字节盖掉」）、
  `table_cells`、`image/link/footnote` 注册表、`outline_follow_cache`、`row_stride_cache`。
  **初稿里写的 `last_stable_source_text` 已删除**——那是文件全文的第二份副本，纯浪费。
- **`TextBuffer`**（src/editor/buffer.rs）：分块文本 + Fenwick 前缀和 + 可平移的 `Anchor` 槽，
  外加 `FileShape { encoding, line_ending }`（src/editor/buffer/file_shape.rs）。要点：
  - 内容是 **LF 规范化**的文本；磁盘上的 CRLF/编码形状记在 `FileShape` 里，保存时还原。
  - `file_bytes()`：没编辑过就返回打开时读到的那份**原始字节**，编辑过就按 `FileShape` 重新
    编码。这就是「打开即保存，字节不变」那条验收的实现。
  - `text()` 每次复制整篇——只在真需要全文的地方调；只要判断「有没有变」时用
    `matches_text(&str)`（零拷贝逐块比较）。
  - `edit(range, &str) -> AppliedEdit` 是唯一写入口；`AppliedEdit { removed, new_range }`
    顺手就是这次改动的逆操作，撤销栈直接存它。
  - 读侧坐标：`byte_len`/`byte_at`/`slice`/`line_of`/`line_range`/`line_count`。
- **`DocumentTree`**（src/editor/tree.rs）：`roots` + `PendingTail`（分块导入未消费的行）+
  `VisibleTreeSnapshot`（DFS 可见序与 entity→索引映射，`rebuild_metadata_and_snapshot` 在结构
  变更后重建一次）。计数 `whole_document_renders`：`markdown_text`/`raw_source_text` 每被叫一次
  加一，是性能闸门的分子（见 §3 末）。
- **`Block`**（src/components/block/runtime/mod.rs `struct Block`）：GPUI 实体。`record` +
  `children` + 运行时状态：`selected_range`（**块内**字节偏移选区）、`marked_range`（IME）、
  `edit_mode`、`projection`（行内定界符展开态）、`last_layout`/`last_bounds`、`table_runtime`、
  `folded`、`search_highlight_ranges`、`cached_display_text`。
- **`BlockRecord`**（src/components/block/state.rs）：`{ id, kind, title, table, html, parent,
  content, raw_fallback, source_span }`。`source_span: Option<Range<usize>>` 是**缓冲区字节坐标**，
  只有根块记，且不含本块自己的行尾换行——块与文档的唯一对应关系就是这一段字节。
  块文本 `record.title` 的语义从「内容」变成「`parse(buffer[span])` 的缓存」。Raw 保留类块
  （RawMarkdown/Comment/HtmlBlock/MathBlock/Mermaid）把原始源码存在 `raw_fallback`。
- **行内内容**：`InlineTextTree = Vec<InlineFragment>`（src/components/markdown/inline.rs）。
  **定界符字符不落盘**，序列化时按规则重建（`serialize_markdown`），但**写法落盘**：强调用 `*`
  还是 `_` 记在 `InlineStyle::emphasis_marker`（`stacks.rs` 序列化照写，`__粗__` 不会被写成
  `**粗**`），列表项的子弹字符与序号分隔符记在 `BlockRecord::list_marker`（`+ 项目`、`1)`
  不会被写成 `- 项目`、`1.`，显示也照写）。**转义也是写法**：`\*不强调\*` 在可见文本里就
  是两个 `*`，与 `2 * 3` 那种没配对的定界符长得一模一样，所以解析时把「这个字符是反斜杠
  换来的」记成 `InlineTextTree::escaped_offsets`（可见字节偏移，随 `split_at`/`append_tree`
  与每次重解析一起搬运）。少了这一位，一次打字就会把写法读成语法：可见文本变短、渲染变粗，
  写回也只好整块重新序列化。正因为重建出的字节只保证「语义相同、写法可能不同」，
  写回必须只动真正改过的那段字节（§3），不能「从块树重新生成这一块」。渲染侧有
  `InlineRenderCache`（可见文本 + spans + 双向偏移映射 `InlineMarkdownOffsetMap`）。

**跨块定位两条路**：块→文档直接用 `record.source_span`（缓冲区坐标，无需构建映射）；文档→块
仍走 `SourceTargetMapping`（§5，按块的字节映射表，正在被 span 直取一点点替代）。


## 2. 导入：文件 → buffer → 块

- 入口 `Editor::from_loaded_document`（src/editor/mod.rs）拿到 `encoding::load_document` 的结果：
  解码后的文本 + `FileShape`。**行尾不再在导入时被抹平**：文本进 `TextBuffer` 时是 LF 规范化的，
  CRLF/编码形状由 `FileShape` 记着，保存时还原（`file_bytes()`）。
- `from_markdown` / `from_markdown_with_chunk_budget`：`markdown_requires_source_mode_fallback`
  检查（不支持的构造整体降级为单 RawMarkdown 块 + Source 模式）→ `split_markdown_lines` 切行 →
  `build_root_block_chunk` 逐块构建 → `attach_root_spans`（src/editor/mod.rs）把每根块占的行区间
  换算成缓冲区字节区间，写进 `record.source_span`。
- **解析器是手写逐行扫描器，不是 pulldown-cmark**（pulldown-cmark 只用于 HTML 导出）。分发顺序见 `build_blocks_from_lines_internal`（src/editor/document/import.rs；识别函数在 src/editor/document/parse.rs）：frontmatter → 空行段 → 围栏代码 → fenced div → HTML 注释/块 → 脚注定义 → 引用定义 → setext 标题 → 独立图片 → 缩进代码 → 列表 → 引用块/callout → ATX 标题 → 分隔线 → 表格（含 pipeless）→ 展示数学 → 兜底段落。
- 每块行内解析：`native_block` → `InlineTextTree::from_markdown`。无法表达的构造 → `raw_block`（`BlockKind::RawMarkdown`）逐字保留。
- **分块/渐进导入**（大文档关键）：`PendingTail` + `start_pending_materialization_task`/`materialize_next_pending_chunk`（src/editor/mod.rs），游标是 `ChunkCursor`（src/editor/document.rs：`root_budget`/`is_document_start`/`previous_root_is_list_item`）。预算 `FIRST_CHUNK_ROOTS=2000`、`STEADY_CHUNK_ROOTS=250`：首屏同步建 2000 块，其余后台每轮 250 块续建。需要全文的操作调 `flush_pending_materialization`；区域重投影（§3）在 `pending_tail().is_some()` 时直接放弃走全量。
- **纯文本/代码文件路径（性能敏感）**：`from_file_source`（src/editor/mod.rs）按 `workspace::is_code_file`（src/editor/workspace/search_backend.rs，扩展名表含 `log/lock/toml/txt/csv/json/...`）分流 → `replace_document_from_code_source` → `replace_document_content`（src/editor/file_drop.rs）：**整个文件变成单个 `BlockKind::CodeBlock` 块**，Source 模式等宽编辑。文件全文只有一份的事实源是 `buffer`，读取侧与保存都走它（初稿里的
`last_stable_source_text`、`document_search_source` 那几份副本已删）。**剩下的两份开销**：解析出来的
`record.title` 仍是一块拥有的副本，加上单块 10 MB 文本的 shaping/行计划——按行窗口渲染没做之前改不掉。

## 3. 编辑：按键 → 区间落笔 → 局部重投影

1. 按键由焦点 **Block** 的 GPUI input handler 处理：`Block::replace_text_in_range`（src/components/block/input.rs）→ 计算 undo 类型 → `prepare_undo_capture` → `replace_text_in_visible_range`（src/components/block/runtime/mod.rs）改 `record.title` 并 emit `BlockEvent::Changed`。
2. Editor 经 `on_block_event`（src/editor/events/block_event.rs，订阅点在 runtime_context.rs `new_block`）收所有块事件。结构性事件（换行/合并/缩进/粘贴等）经 `DocumentTree::insert_blocks_at`/`replace_root_range` + `with_structure_mutation` 改树。
3. **写回从「块改了就重序列化这一块」换成「只贴真正不同的那一段字节」**，按精度分档，逐档失败才降级：
   - `write_back_visible_insertion`（src/editor/mod.rs）：打字/插入这种「在已知字节点插一段」，直接在缓冲区那个点插。
   - `write_back_block_source`：整根块的内容变了，但只重写它自己 `source_span` 那一段。
   - `write_back_root_region` + `write_back_blank_run`：根块序列变了（一分为二、两块合一），只重写被换掉的那一段区间。
   - 表格：按行/按格落笔，见下。
   - 兜底 `mark_dirty` → `resync_buffer_from_projection`：从块树把全文重新序列化（`DocumentTree::markdown_text`），`source_serializations`/`whole_document_renders` 各加一。**这条是最后手段**：未编辑块的原始字节会在这里被洗掉，所以正常编辑路径必须走不到它。
   - 任一档成功落笔后调 `mark_dirty_written_back`（src/editor/window_state.rs）置 `skip_next_resync`，让本轮的 `Changed` 不再触发兜底重投影。
4. **表格的结构命令也都是缓冲区编辑**（src/editor/table_edit.rs）：单元格打字 `write_back_table_cell_source`（只动那一格的内容字节，同列宽填充不动）、加行 `write_back_table_row_insertion`（照最后一行的骨架插一行）、删行 `write_back_table_row_deletion`（剪掉那一行连着它前面的换行）、加/删列 `write_back_table_column_insertion`/`_deletion`（每行插/剪一格，从后往前）、调对齐 `write_back_table_column_alignment`（只重写分隔行那一格）、移动行/列 `write_back_table_row_swap`/`_column_swap`（文本对调，净长度不变）、删表头 `write_back_table_header_promotion`（改第一行 + 剪掉升上来的那行）。量不出行形状时（格子里有转义竖线、表挂在容器里没有自己的区间、行数与模型对不上）才退回 `write_back_table_structure_edit`。
   - **读侧的格子位置量的是同一把尺**（`push_table_row_mappings`，src/editor/source_mapping.rs）：按「第几行第几列」从原文的管道符之间夹出内容区间（`cell_content_range_in_line`，格子槽位由 `table_row_slots` 切：转义的 `\|` 不算列分隔，外层管道符两侧的空槽不算格）。**容器里那张表从哪里起头是走树带下来的落点**（`push_table_mappings_in_root` 取 `absolute_start` 所在行 = 表头行，只做结构核对：格数、下一行是定界行、再往后行数够，且都落在本根块区间内），不拿表头序列化出来的文字回原文里搜。搜的口径有三处会静默失配：空格子序列化出空串（于是这一格**没有映射**，光标停在里面时 `caret_source_offset` 算不出，粘贴/跳转/行列号只能退回默认位置）；写法与序列化口径不一致（`__粗__`、`` `x` ``）；表头里有转义管道符时旧口径按裸 `|` 切列，切出 3 段对不上模型的 2 格，于是**整张表一个映射都没有**（实测 `> | a\|b | 数量 |`）。同一根块里有两张同表头的表时还要靠「离起点最近」猜哪张是本表。容器里的行写着 `> | 甲 | 乙 |`，量之前要先让开容器记号（`table_row_container_prefix`），否则 `> ` 被当成第 0 列。守卫：`an_empty_table_cell_still_knows_which_bytes_it_is`、`a_quote_table_maps_columns_after_its_container_marker`、`a_quote_table_with_an_escaped_pipe_in_its_header_still_maps_every_cell`、`two_quote_tables_with_the_same_header_map_to_their_own_rows`、`a_quote_table_below_a_list_and_a_fence_maps_to_the_rows_it_is_on`、`typing_above_a_nested_table_keeps_its_cells_on_their_own_rows`。
5. **重投影从「整棵树重新解析」换成「只重解析变了的那一段」**：`reproject_root_region`（src/editor/document/import.rs）把一根根块换成它那段行重新解析出的若干根块（窗口 = 本段行 + 前瞻 ≤2 行，且要求解析结果落在本段内才接受，否则放弃走全量 `rebuild_root_blocks_from_buffer`）；计数器 `Editor::roots_reprojected` 记增量重投影了几根。
6. **引用敏感的块才刷新运行时**（`changed_block_needs_runtime_context_refresh`，src/editor/runtime_context.rs）：image/link/footnote 注册表从 `buffer.text()` 解析，且刷新必须排在写回**之后**，否则读到的是改动前的文本（`editing_image_reference_definition_refreshes_existing_image` 钉住这一顺序）。
7. **闸门**：`a_real_editing_session_never_falls_back_to_whole_document_serialization`（src/editor/tests/perf_budgets.rs）把打字、回车拆块、勾任务框、缩进/提级/降级、标注里拆块、表格加行/删行/调对齐/加删列/移动行列、删整张表、多行粘贴一条条走一遍，断言 `source_serializations + whole_document_renders` 增量为 0。白名单常量 `WHOLE_DOCUMENT_RESYNC_STILL_ALLOWED` 现在是空表——每加一条命令都只能让它更短。
   **源码模式那一档也计数**（2026-10-04）：`resync_buffer_from_projection` 的源码/代码分支以前不加
   `source_serializations`，白名单空表看着全绿，实际未闭合 fenced div / 不支持 admonition 触发的
   整篇兜底里，1 MiB 文档一次按键实测 **2.28 秒**（成本随文档线性增长，大头是把整篇文本重解析成
   行内树）。数出来之后就归零了：源码/代码文档的块挂上自己的缓冲区区间，按键按那一
   段字节落笔（不变式 20）：同一份 1 MiB 文档切到源码视图，一次按键 **1.99s（整篇落笔 1 次）→
   141ms（0 次）**（`one_mib_source_mode_typing_stays_within_budget`）。守卫
   `source_mode_typing_writes_through_the_buffer_and_preserves_every_other_byte`、
   `typing_in_a_code_document_writes_through_the_buffer`（同时钉住字节：除了光标那一个字符，其余原样）。
   `raw_source_text` 还在，但只剩源码视图里的**结构**命令（回车拆块、删块）会走——那几档的接缝规
   则是渲染态的，对源码文档不适用。
8. **字节保真**由另一组按字节断言的测试守（src/editor/tests/round_trip_fidelity.rs、block_source_write_back.rs、block_source_spans.rs）：`__下划线__` 写法、字面转义 `\*`、Setext、表格列宽、CRLF、末行换行、无末行换行，打开—编辑—保存之后没改过的字节必须逐字节还是磁盘上那样。
9. **`record.title == parse(buffer[span])` 的渲染侧对照**：`typing_one_char_only_changes_the_text_at_the_caret` 对全部保真形状（含脚注定义）断言「打一个字只动光标那一个字」——本块可见文本正好多出那一个字符，其余块的可见文本一字不改。字节表盯磁盘，这条盯投影：写法被重新解释时（`\*` 读成强调、脚注退回源码形状），字节可能没变而渲染已经变了。
10. **脚注序号是渲染形状，不是字节形状**：`[^1]` 进树是一个带 `InlineFootnoteReference` 的片段，可见文本按注册表贴成 `¹`。编辑后的重解析认得出 `[^1]` 这个形状、认不出序号（片段 `ordinal` 为空，文本退回源码形状），所以 `sync_footnote_registry` 不能只在注册表换人时回填——段首打一个字并不换注册表，不回填的话屏幕上就是 `[^1]`，可见长度多出三个字节，光标与字数都跟着错。回填排在写回**之后**，因此缓冲区里始终是 `[^1]`（`typing_next_to_a_footnote_reference_keeps_the_ordinal_label`、`typing_between_two_footnote_references_keeps_both_ordinals`、`deleting_a_char_next_to_a_footnote_reference_keeps_the_ordinal_label`）。
11. **记号占几字节是当场从文件量出来的**（`measured_block_prefix`，src/editor/source_mapping.rs）：ATX 标题按 `parse_atx_heading_line_with_marker` 量的宽度（含前导缩进与记号后面那个空格），Setext 量到 `0`——内容行里根本没有 `# `。读侧的标题映射按这个宽度起算内容，不再按模型拼一个 `# ` 猜：猜错就整块偏移整体漂，Setext 漂两字节还会切进中文字符中间，搜索命中的选区、高亮、行列号、粘贴插入点说的都不是那几个字节。这曾记在 `BlockRecord::content_marker_len` 上（导入时量一次），2026-10-04 换成走查时现量——同一把尺要能覆盖嵌套层（见 15），而记在块里只有导入过的那几个形状有值。守卫：`block_offsets_land_on_the_bytes_the_file_actually_has`、`typing_in_a_setext_heading_keeps_the_underline_and_the_offsets`。
12. **段首的落点在记号后面**：`mapping_source_offset`（src/editor/selection.rs）把「块内第 n 个可见字符」换算成缓冲区字节，第 0 个也不例外——它以前直接返回整块起点，而起点含 `# `、`- `、`> ` 这些记号，于是在标题开头打一个字，字落进 `#` 前面（屏幕上是标题 `X标题`，磁盘上是 `X# 标题`，重新打开就是个段落）。记号宽度既然是数据，插入点就只能在它后面。守卫：`typing_at_the_start_of_a_block_lands_after_its_marker`（七种写法逐个钉落点、kind 与光标）、`typing_at_the_start_of_a_heading_keeps_the_marker_on_disk`（钉到磁盘，并重新打开确认还是标题）。
13. **「这个字节落在哪一块」按块的区间问，不整篇重拼映射**：`block_id_at_source_offset` 先问根块自己的 `source_span`，只有落进容器（引用、列表）才把**那一根**块的子块映射重建出来；`source_mappings_in_range`（src/editor/source_mapping.rs）只重建与给定区间相交的根块，再各带左右紧邻的一根——端点落在两块之间的空行时，`endpoint_for_source_offset` 要按距离挑最近的一块，少带就挑到别处去了。之前点一次大纲标题要走四次整篇重建（`heading_block_at_source_line`、`unfold_sections_covering_source_range`、`apply_marked_source_range`、`apply_selection_snapshot_in_current_mode`），每次都是 O(文档)：1 MiB 实测一次 227ms，10 MiB 就是秒级。守卫：`clicking_an_outline_heading_unfolds_it_without_a_document_wide_mapping`（`source_mapping_full_builds` 增量为 0，同时钉住那个 Setext 标题确实被展开）。
14. **跨块复制交出去的是缓冲区里的那段字节**：`cross_block_selected_markdown`（src/editor/selection.rs）现在就是 `buffer.slice(选区的源区间)`。以前它按块树的序列化口径逐块重拼、再按「块间补空行、紧排列表项不补」的规则粘起来——Setext 的下划线在这趟里丢掉（复制—粘贴之后那一块不再是标题），`__强调__` 与 `1)` 也随时可能被洗成别的写法，而且为了算边界还要整篇重拼 source mapping。端点换算用 `source_mapping_for_entity`（只走这一根块），拿不到映射的原子块（表格整块）退回它自己的 `source_span`；两者都没有区间时（刚插进树、尚未写回的空段落）给一个就近的零宽锚点，删除才不因它中止。守卫：`copying_a_cross_block_selection_gives_the_bytes_from_the_file`、`copy_then_paste_a_cross_block_selection_keeps_the_writing_style`、`delete_selection_*`。
15. **子块在它那一行的起点也按文件量**：走查算子块的绝对位置时，前缀以前是按模型拼的（列表每级两个空格、引用一律 `> `）。文件里缩进四格、制表符、`>引用`（记号后没空格）时整条链就漂几个字节——实测在 `- 父甲` / `(四空格)- 子乙` 的子项里打一个字，文件变成 `- X父甲`：字节进了**父项那一行**，屏幕上子项却照常多出那个字符（制表符那一例更狠，落笔顺手把 `\t` 洗成两个空格）。`measured_block_prefix` 用解析器自己的剥记号函数把「本行里内容从第几个字节开始」量出来：引用每层都在自己那一行上，逐层重量；列表的上级只留下缩进，交给本块的记号（`parse_list_marker` 把前导空白一并吃掉）或段落继承来的 `list_dedent`（父项记号的实测宽度）吃掉。量不到就退回按模型拼——起点不是行首、起点落在多字节字符中间（说明上游的字节账已经错）、这一行的记号与本块的类对不上（剥不动）。多行内容另有每行的量法（见 18）。守卫：`typing_in_an_indented_list_item_lands_on_that_item`（四空格/制表符/引用里四空格/缩进四格的序号项，钉到文件字节）、`nested_shapes_put_block_offsets_on_the_real_bytes`（11 个嵌套形状钉块内偏移落在文件真字节上）、闸门里多出的那一步「子项里打字」。这张量表按「整段可见文本应是文件里连续的一段」比对，放不下多行块（脚注定义续行、引用容器正文——可见文本跨行，行间还夹着各自的记号），它们由 18 那条盯。
16. **围栏行与内容行的缩进也按文件量**（`measured_code_block_line_prefixes`）：模型里根本没有围栏那两行（映射从绝对起点直接拼围栏加信息串），内容行存的是**上级容器 dedent 之后**那一段——于是两级缩进都不在模型里，按 `render_depth` 拼「每级两个空格」就整体漂。实测（2026-10-04）缩进两格的围栏里在内容第 3 个字符处打一个字，字落到内容行的行首（文件 `X  let a`，屏幕 `  Xlet a`）；列表项里四格的那种，字写进了同一根列表的 `- 步骤` 那一行。量法：围栏开行按「文件那一行去掉围栏与信息串之后剩下的必须是纯空白」，内容行按「文件行的缩进 − 模型行的缩进」——**只比缩进不比整行**，因为算光标时这一次按键的字节已经在模型里、还没进缓冲区，要求「文件那行以模型这段结尾」在算的一刻就不成立了。闭合行与开行同规矩（文件写的围栏与信息串对不上就不量，比如信息串里带反引号而模型改用 `~~~~` 那类）。守卫：`typing_inside_an_indented_code_fence_lands_on_those_bytes`（两格、列表里四格）、`typing_at_the_end_of_an_indented_fence_line_keeps_the_fence_indent`（块末的光标走整块写回，围栏两行的缩进与信息串得原样）、`nested_shapes_put_block_offsets_on_the_real_bytes` 里那格围栏。
17. **缩进代码块不补围栏行**：制表符开头的 ` ``` ` 按 CommonMark 就是缩进代码块（制表符算四列，超过围栏允许的三列），可 `push_code_block_mapping` 以前给**任何**代码块都先拼一遍围栏 + 信息串 + 换行，内容行的起点因此整体后移 4 字节——实测在内容第 3 个字符处打一个字，`\t```rust` 那一块写成 `\t```ruXst`（字跑到下一行去了），`\tfoo bar` 写成 `\tfoo bXar`。围栏量不到（开行去掉围栏与信息串之后不是纯空白）就换 `measured_indented_code_line_prefixes`：块占几行按本块的 `source_span` 数（数不齐就说明这一块其实有围栏，交回围栏那条路），每行前缀仍是「文件行的缩进 − 模型行的缩进」，前后不补任何东西。守卫：`typing_inside_an_indented_code_block_lands_on_those_bytes`（伪围栏、制表符、四空格三例钉文件字节）。
18. **多行内容每一行的记号都按文件量**（`measured_block_line_prefixes` + `build_line_prefixed_content_mapping`）：`build_prefixed_content_mapping` 只有「首行前缀 + 续行前缀」两个字符串，于是多行块（多行引用、引用容器正文、列表项里的续段）从第二行起一律按 `> ` 每级、列表每级两个空格拼。文件里第二行写 `>引用二`（记号后没空格）就少一位、`>   引用三` 就多一位——实测在 `> 引用一\n>引用二\n>   引用三` 的第二行行首打一个字，字节写成 `> X引用二`（比内容起点晚一位，落进了记号与内容之间）；四空格嵌套列表的续段更狠，把每行的量法关掉之后在续段行首打一个字，写成 `  X  子丙`（字落在缩进里），更早一轮还整块重贴成 `- 父甲\n  - 子乙\n    子丙X`——子项那行用户自己写的四格缩进被模型洗成两格。首行沿用 15 那条（本块记号在那里量），续行按「这一行剥掉容器记号后的缩进 − 模型这一行的缩进」量：上级列表的 dedent 是逐级累出来的、走查只带得到累计值，而缩进差正好等于上级容器从这一行吃掉的字节数。行的**长度**也按文件算（`MeasuredBlockLines.file_lens`）：模型为保住一个字面反斜杠会多写一位（`第一行\` + 换行在模型里是 `第一行\\`），位置换算按模型的 markdown 长度走，后面每一行的落点就漂一位——实测在这块的续行行首打一个字，落点算不成字符边界，退回整块写回，把文件里那个 `\` 转义成 `\\`（21 字节变 23 字节）。行内逐位对齐、超出文件那行长度的部分钳在行尾，行与行之间按文件的长度推进。任一行对不上（引用记号剥不动、模型行的缩进比文件行还多、行不在缓冲区里）就整块退回按模型拼。守卫：`typing_on_the_second_line_of_a_quote_with_varied_markers_lands_on_those_bytes`（没空格/三个空格/四空格续段三例钉文件字节）。
19. **「块内最后一个位置」的落点按映射表，不按块区间末尾**：`mapping_source_offset`（src/editor/selection.rs）在 `offset >= visible_len` 时以前直接交 `full_source_range.end`，而块区间是可以压着不属于内容的字节的（`# 标题寅 #` 的闭合 `#`、围栏的闭合行、行尾空格）。现在交 `content_to_source` 的最后一项——内容在文件里真正结束的那一位。跨块选区端点、源码/渲染视图切换后的光标、查找命中选中、大纲改名时的选中区间都读这一个换算（改这一步时它们各有测试红过，落点按内容末尾走才全部对上）。
20. **源码/代码文档的块就是缓冲区的一段切片**：文件里既没有 ``` 围栏行、也没有列表记号，所以区间不问解析器——`attach_source_slice_spans`（src/editor/mod.rs）按「块自己的文本长度 + 一个块间换行」推进，`buffer.slice(span) == block.display_text()` 恒成立，块内位置换算也就是恒等表（`push_source_slice_mapping`）。**这条恒等只在块顶格时成立**：渲染态里源码形状的块（HTML、注释、展示数学、认不出的语法，`EditMode::SourceRaw`）挂在引用/列表里时，可见文本是**剥掉容器记号**那一段，而块起点是整行的行首——拿恒等表换算就把内容第 0 位算到了 `>` 记号上（实测在 `> <p>甲文</p>` 块首打一个字，字写在 `>` 前面，整块掉出引用）。那一段按不变式 23 记下的每行宽度落笔（`measured_slice_line_prefixes` 是没记到时的退路：只让容器记号，剥完必须与模型这一行一字不差）。两条推论：`block_markdown_source` 对 source-raw 的块交回那份原文（走序列化会给它补一对围栏，等于把 markdown 写进用户的纯文本文件），而**结构写回那几档对源码文档不适用**（`write_back_root_region` 在 Source 视图直接退回）——渲染态的接缝是「根块之间空一行」，源码视图的接缝是「隔一个换行」。这一档以前没有区间，位置靠「拷整篇文本 + 累加序列化长度」记账，还拿围栏口径去量代码文档的行：`这一行字节数 − 围栏长度` 会切进多字节字符中间（`def 甲():` 一打字就 panic）。守卫：`typing_in_a_code_document_with_chinese_lands_on_the_caret_bytes`、`source_mode_typing_writes_through_the_buffer_and_preserves_every_other_byte`、`typing_in_a_code_document_writes_through_the_buffer`、`newline_in_a_code_document_inserts_only_a_line_break`、`one_mib_source_mode_typing_stays_within_budget`。
21. **规范化只住在「格式化文档」这一条命令里**（`format_document`，src/editor/window_state.rs；命令注册表 id `format_document`，默认键 `cmd/ctrl-shift-L`）：Setext→ATX、`__粗__`→`**粗**`、`1)`→`1.`、表格列宽重排、根块之间补分隔空行，**只**在这条命令里发生；打开、打字、保存、撤销、重投影一律不许规范化——那是缓冲区当事实源换来的性质，混进隐式路径就全废了。三步：①`DocumentTree::canonicalize_writing_style` 把模型里那份「用户自己选的记号」数据清成默认（`BlockRecord::list_marker`、`InlineStyle::emphasis_marker`，含子树与表格每一格；**`escaped_offsets` 不清**，反斜杠转义是语义不是写法）；②按默认写法序列化，与缓冲区比对，没变就什么都不做（不动字节、不标脏、不留空撤销组）；③变了走一次 `apply_resynced_text`（最小差异，撤销组因此只存真正的差）再 `rebuild_document_from_buffer` 重挂投影。第三步是必须的：撤销只把字节放回去，而写法数据记在树里，不重投影的话树里还留着默认记号——用户撤销成功了，再打一个字又被 `**` 覆盖回去。这一条命令自己付一次整篇序列化，`source_serializations` 数得到（有意不加豁免：漏一档计数就等于给隐形成本开门），但它**不出现在按键路径上**——闸门量的是后者。守卫：`formatting_the_document_writes_the_models_canonical_style`、`undo_after_formatting_restores_the_original_bytes`、`formatting_an_already_canonical_document_writes_nothing`、`typing_never_formats_the_document`、`formatting_a_source_document_does_nothing`、`formatting_again_after_an_undo_still_normalizes`。
22. **读侧的派生视图只重算「被改过的字节所在的块」**：文档大纲（侧栏「大纲」页签与正文 `[TOC]`）是块树的一份投影，它以前每帧拿整篇文本比较一次再把整篇按行重扫（实测一次按键 105ms / 58.5 万行，且不在任何计数器里）。现在它按根块缓存行摘要（`OutlineSegment`：这段的标题 + 段字节数 + **进出这两头的围栏状态**），摘要键是「块的 `EntityId` + 走进这块时的围栏状态」——重投影换掉的实体自然没缓存于是重算，没动的块至多平移行号；围栏状态进键是因为同样的字节在栏里/栏外扫出来的东西完全不同（`Option<(char, usize)>` 带记号字符与长度：反引号栏里的 `~~~` 关不掉它）。走根序列时把上一片的 `fence_out` 当下一片的 `fence_in` 喂进去，跨接缝的未闭合围栏就不再作废整条增量路。判断「这块动过没有」有三个信号，缺一个就会算错（前两个都是差分测试抓出来的真缺陷）：①缓冲区记的 `take_dirty_region` 与这块区间相交；②段的**字节数**变了（拆块合块把接缝重新分配，块自己的字节可以一个字没动却指着另一段字节，`write_back_root_region` 为此调 `note_outline_dirty_region`）；③区间起点没落在行首 → 段内相对行号对不上整篇行号，退回整篇重扫（`outline_full_rescans` 数得到：markdown 与源码视图实测都是 0）。把「块起点」换算成整篇行号这一步问的是缓冲区每块那张换行偏移表（`Chunk::newlines`，每行两个字节），**一次都不读正文**——以前它每批量换算都要沿块把正文数一遍（含给每块反找最后一个换行符），`take_line_probe_bytes` 现在钉住查询读 0 字节、维护成本只跟着改动大小走。另外**没人看就不算**：侧栏收起时不再每帧重算，`[TOC]` 块改成在自己需要的那一帧直接要清单（旧实现靠「整篇文本没变」短路，懒导入续建期间一直留着半份大纲，`[TOC]` 要到第一次编辑才补全）。加新的读侧视图（搜索索引、反链、字数）照这一条办，并先把成本接进计数器。守卫：`the_incremental_outline_matches_a_whole_document_scan`（9 种形状 × 每根块块首打字与回车，逐条与整篇扫比对，并且钉住「全程一次整篇重扫都不许发生」——只比结果的话退回整篇照样一致，等于自证）、`the_outline_is_only_built_for_a_reader`、`a_fence_crossing_a_source_chunk_seam_still_scans_per_block`、`a_longer_fence_above_pulls_the_following_chunks_inside_it`（开栏长一格，后面没动过的片要跟着改口；撤销再回来）、`ten_mib_typing_does_not_scan_the_whole_document`、`one_mib_code_document_with_a_fence_on_the_seam_scans_one_chunk`、`lines_and_line_starts_agrees_with_asking_one_offset_at_a_time`、`byte_len_survives_many_edits_without_counting_the_chunks`、`asking_for_line_numbers_does_not_read_the_text`、`rebuilding_the_line_index_costs_only_the_edited_bytes`。


23. **块内每行的记号宽度是解析期记下的数据，不是事后比出来的；谁重新分配区间，谁就让它过期**（`BlockRecord::source_line_prefixes`，src/components/block/state.rs）：ATX 标题由 `parse_atx_heading_line_with_marker` 交出记号宽度，根段落一律 0（一行都没剥），列表项按「行长度 − `marker.text` 长度」（缩进、`+`/`*`/`-`、`1)`、制表符分隔、任务框一起算进去）。位置换算先问这份数据（`recorded_block_line_prefixes`，src/editor/source_mapping.rs），问不到才退回按文件量那一族（`measured_block_*`），所以迁一族形状不用动消费者。容器里那些「模型里就是原文」的形状（HTML、注释、展示数学、分隔线、认不出的语法）由 `note_region_line_prefixes` 在解析期把每一行让开的字节记到块上——这一族事后**没有记号可剥**可比，只有剥容器记号的那段代码当场知道；顶格的切片块不记（全零与不记等价），继续走恒等表。**记下来的位置会过期**：结构写回会改块自己那一行的形状（Setext 提成 ATX 就多了 `# `），旧账让每个块内偏移整体漂两位（实测内容末尾报第 9 位，文件里在 11 位）。规矩是 `write_back_root_region` 重新挂哪一段的区间，就把那一段里每块的这份账清掉，换算自然交回按文件量，直到重新解析给它新的账。还没记到的形状：引用与列表里的**子块**（上级容器吃掉的字节要一起进账——得让 `collect_quote_block` 剥 `>` 与 `dedent_lines` 那两步把每行「已让开几字节」传给递归解析）、多行块的续行、代码围栏、表格格子，所以 `measured_*` 那一族暂时删不掉，删它的条件就是这些形状都记上了。迁移进度看 `line_prefix_from_record` / `line_prefix_measured` 两个计数器（后者该一路降到 0）。守卫：`an_atx_heading_remembers_where_its_content_starts`、`typing_in_an_atx_heading_uses_the_parse_time_prefix`、`typing_in_a_paragraph_uses_the_parse_time_prefix`、`typing_in_a_list_item_uses_the_parse_time_prefix`（五种记号写法）、`a_rewritten_block_drops_its_parse_time_prefix_record`、`an_atomic_block_in_a_quote_remembers_what_its_lines_yielded`（记下之后还要验一遍「按这份账从文件里读得出这一行的内容，且行长度正好等于记号加内容」）、`typing_at_the_head_of_an_atomic_block_in_a_quote_stays_after_the_marker`（七种写法：`> `、记号后不带空格的 `>`、两层 `>>`、注释、多行数学、分隔线、引用里认不出的标题）。

## 4. Undo/历史（src/editor/history.rs）

- **增量组，不是全文快照**：`HistoryEntry { edits: Vec<AppliedEdit>, selection, timestamp, kind }`
  （src/editor/mod.rs）。一次编辑动作 = 一组 `AppliedEdit`（每条是 `{ removed, new_range }`），
  撤销 = `replay_history_group` 从后往前把每条的 `new_range` 换回 `removed`；重做反向再来一遍。
- `prepare_undo_capture` 在编辑**前**开组；编辑路径里每次 `TextBuffer::edit` 的返回值都经
  `record_buffer_edit` 落进这组；`finalize_pending_undo_capture` 收组入栈。
- 合并窗口 `HISTORY_COALESCE_WINDOW`（1s）只并 `CoalescibleText`；`ImeComposition`/
  `ImeCompositionCommit` 各自独立成步；`NonCoalescible` 永远单独一步。栈深 `HISTORY_LIMIT=200`。
  `history_group_is_noop` 在一份副本上真重放一遍来判断「改了等于没改」，那种组不入栈。
- undo/redo 之后：缓冲区已经是目标状态，置 `skip_next_resync = true`；投影按 `rebuild_document_from_buffer`
  （从缓冲区重新解析，**不是**从块树序列化）重建，`apply_selection_snapshot_in_current_mode` 把选区
  放回缓冲区坐标里的那几个字节。
- **内存预算有闸门**：`two_hundred_undo_steps_stay_within_the_memory_budget`（200 步 ≤ 8 MiB，且
  ≤ 64 KiB 的绝对上限）与 `undo_memory_does_not_scale_with_document_size`（同样动作在大文档上记的
  字节数不跟着文档长）——初稿写的「10MB × 200 ≈ 2GB」是这次还掉的账。

## 5. 选区/光标与位置（src/editor/source_mapping.rs 与缓冲区坐标）

- 每 Block 自持 `selected_range`（**块内**字节偏移）+ IME `marked_range`；焦点走每块
  `focus_handle`；Editor 记 `active_entity_id`。跨块选区：`CrossBlockSelection { anchor, focus }`
  （`{entity_id, offset}`，src/editor/selection.rs）。
- **文档级位置一律用缓冲区字节坐标**：`record.source_span`、撤销/重做的 `UndoSelectionSnapshot.range`、
  搜索命中 `source_range`、大纲与锚点跳转、状态栏的「行 : 列」（`compute_source_cursor_position`，
  src/editor/status_bar.rs，直接在缓冲区里数行与字素）。这些都不要再引入「序列化文本里的偏移」。
- `SourceTargetMapping`（块内容偏移 ↔ 文档偏移的逐块映射表）还在，用途：undo 选区恢复、跨块替换、
  搜索高亮落到具体块、大纲跟随滚动、光标历史。它是从 `record.source_span` + 块内映射推出来的，
  计数器 `source_mapping_builds`/`source_mapping_full_builds` 盯着它别在按键路径上整篇重建
  （`typing_does_not_rescan_status_bar_statistics_every_key`、`per_keystroke_document_passes_stay_bounded`）。
- 表格单元格是独立 Block，经 `TableCellBinding` 绑定；它在原文里的字节区间由
  `table_cell_source_range`（src/editor/table_edit.rs）按「第几行第几列」从管道符之间量出来
  ——不能拿格子文本去原文里找，用户刚打的字还没进文件。
- **还没做完的**：磁盘搜索命中里那些没有 `source_range` 的仍要靠 `match_ordinal`（在缓冲区里重数
  第 k 个含词行）定位。表格单元格里的搜索命中**已经画得出**（格子是独立 Block，走
  `BlockTextElement`，它读 `search_highlight_ranges`；由 `document_search_hit_inside_table_jumps`
  钉住），还没画出来的是那几个不走 `BlockTextElement` 的格子：含行内数学/上下标/内嵌图片的格子、
  长块兜底那一档，以及 HTML `<table>`（src/components/block/render/inline_visuals.rs、paint_parts.rs）。
- **渲染态已经没有整篇 source mapping 走查**（2026-10-04）：`sync_document_search_highlights` 改成
  「先在缓冲区里按字节找命中，只为**有命中的那一根块**重建它自己的映射」（`search_ranges_for_hits`
  做偏移换算），既不再复制整篇文本，也不整篇走查；守卫
  `document_find_highlights_map_only_the_blocks_with_hits`（`source_mapping_full_builds` 增量为 0，
  并钉住正文与表格格子两处命中）。`build_source_target_mappings` 现在两种视图共用同一条「按根块区间
  起锚」的走查（源码/代码文档的块也挂上了区间，见不变式 20；以前它另走一遍「拷整篇文本 + 累加
  序列化长度」的记账），只剩一处入口：窗口内一根有区间的块都没有时的退回。**块内**那段前缀重建已经收窄：单行内容的块（标题、段落、列表项、任务项、单子块引用）
  按文件量记号宽度（不变式 15），代码围栏的开行、内容行、闭合行也按文件量（不变式 16）。
  块内那段前缀重建已经全部改成按文件量：单行按 15、围栏与缩进代码块按 16/17、多行按每行（18）。
  它还是要为每一根块现算一遍映射，真正能删掉它的是「每个子块与每个格子在解析期就记下自己的字节区间」；再往下要等
  每个子块与每个格子都在解析期记下自己的字节区间才能删（方案 §4 的 `SourceRegion`，表格 cells 已经在
  按结构量了）。
- **大纲跟随滚动已经不付全文的钱**（`sync_outline_follow_scroll`，src/editor/workspace/tree_sync.rs）：
  块起点问它自己的 `source_span`（容器里的子块退回那一根块的映射），行号问 `buffer.line_of`，
  于是 `outline_follow_cache`（按 revision 缓存的整篇 ranges + 百万条 `newlines`）整个删掉。
  守卫：`scrolling_with_the_outline_open_follows_the_heading_above_the_viewport`——这条路径此前 0 测试。


## 6. 持久化（src/editor/persistence.rs）

- **保存的字节来自缓冲区**：`document_text_for_save()` = `buffer.text()`，`document_bytes_for_save()`
  = `buffer.file_bytes()`（没编辑过就是打开时那份原始字节，编辑过按 `FileShape` 重编码）。
  会话标签缓存、自动保存的恢复快照、导出（HTML/PDF/PNG）、拖拽替换前的另存**全部取这一份**，
  生产代码里已经没有「从块树整篇序列化再当作文档内容用」的入口了（`DocumentTree::markdown_text`
  / `raw_source_text` 只剩兜底重投影那一档和测试在用）。行尾形状也不再靠 `code_uses_crlf` 这种
  标记还原——它就记在缓冲区的 `FileShape` 里。
- **原子写**：`write_atomic` = 同目录临时文件 + `sync_all` + `rename`。
- **外部修改检测**：`file_content_version`（规范化文本 DefaultHasher）；手动保存与 autosave 前 `verify_file_version` 重读比对，不一致则报「外部修改」。
- **Autosave**：`schedule_autosave` 防抖后台任务（默认 800ms，`[editor] autosave_debounce_ms`）；IME 组合中跳过；后台写恢复快照 + 临时文件，回主线程校对 revision 后落盘。
- **Watcher**（src/editor/watcher.rs）：每工作区递归 notify 监听，干净标签自动重载，脏标签走冲突提示。
- **关闭流**（src/editor/close.rs）：脏文档拦截为应用内对话框；保存后关闭经 `pending_close_after_save`。

## 7. 已知性能事实（dev 构建，闸门测试实测）

性能夹具不进仓库：`node scripts/generate-fixtures.mjs tests/fixtures/perf` 生成
`tests/fixtures/perf/{one,ten}-mib.md`；缺文件时相关测试打印 `skipping:` 直接通过。

| 场景 | 数据（2026-10-03，`--nocapture` 实测） |
|---|---|
| 1 MiB 一次按键 | 52.5ms；整篇遍数 = 序列化 0 / mapping 3 / 字数 0 / 行计划 2 |
| 1 MiB 源码模式一次按键 | 改前 **1.99s（整篇落笔 1 次）**，改后 141ms（整篇落笔 0 次、整篇 mapping 0 次）（2026-10-04） |
| 1 MiB 五个静止帧 | 7.1ms，遍数全 0（不打字不重算任何东西） |
| 10 MiB 一次按键（markdown） | 改前 **546ms，其中大纲把整篇按行重扫 585499 行**；改后 **241ms**，大纲重扫 0 行（2026-10-04，预算 1500ms） |
| 10 MiB 代码文档一次大纲同步（围栏压在 512 行切片接缝上） | 改前 **142ms，重扫 585499 行（=全文），每帧整篇重扫 1 次**；改后 **44ms，重扫 512 行（=改动那一片），整篇重扫 0 次**（2026-10-04） |
| 同上，那 40ms 拆段量（临时段计时，2026-10-04 深夜） | 按字节数行号 **5ms → 0.26ms**（每块记换行偏移表，`99a7f93`）、按块走查 0.54ms、**拼那棵 5.3 万节点的标题树 37ms**（未解，树是整篇一份） |
| 单次全文操作 | 序列化 1 MiB 31.8µs；数 32 万词 6.9ms；建 15968 条 mapping 236.8ms |
| 撤销栈 200 步 | ≤ 64 KiB（存的是增量；旧制最坏 200 × 文档大小） |

**已经解决的一档**：10 MiB 按键里最大的一笔不是文档模型，而是**侧栏大纲**——它每帧先拿整篇文本
比较一次，再把整个缓冲区按行重扫一遍找标题（`build_outline_tree`，实测一次按键 105ms / 58.5 万行，
占那次渲染的 74%）。它当时不在任何计数器里，所以「整篇落笔 0 次」的闸门全绿也看不见它：继
`ed2df8f` 之后第二处「计数器漏一档 = 闸门盲区」。现在它有三个计数器
（`outline_rebuilds` / `outline_lines_scanned` / `outline_full_rescans`），实现改成按根块缓存行摘要、
只重扫被改过的字节所在的块，并且没人看（侧栏收起又没 `[TOC]` 块）就不算。

**剩下的线性成本**：241ms 仍与文档大小同向，在行计划重建与可见列表重排（markdown 那份 10 MiB 是
106456 根块，源码视图按 512 行切块则是 1144 根）。源码模式同一处根，而且更直白：这一档的**可见列表
不裁剪**（1 MiB 的文档 115 根块全部算「可见」，10 MiB 是 1144 根），每键 2 次行计划重建就随文档长
（实测 1 MiB 141ms、10 MiB 1.37s，整篇落笔都是 0 次）。静止帧 5 帧只要 4.9ms，说明贵的不是绘制本身，
是每键重排这份整篇的可见列表。两条都要「块内/列表也按行窗口投影」才解得掉。曾经的另一处退路已经
解决：源码视图按行切块时 ``` 围栏会跨过块边界，摘要只记「结尾还在不在栏里」，认不出下一片该接着
栏内的状态扫，于是整篇重扫；现在每片带进带出围栏状态，缓存键也含它（见不变式 22）。大纲同步剩下
的 40ms（10 MiB 代码文档一次按键后的一次同步，拆段计时实测）也定了位：按字节数行号那一档换成每块
的换行偏移表之后 5ms → 0.26ms（不变式 22 的缓冲区侧支撑），按块走查 0.54ms，**其余约 37ms 全在
「把 5.3 万条标题拼成一棵树」**——标题树整篇一份，节点 id 又记着整篇行号，任何一处改动都重拼全树，
那是下一档要按改动那段增量拼的活。诊断探针：
`cargo test probe_attribute_ten_mib -- --ignored --nocapture`。
计数入口：`Editor::{source_serializations, source_mapping_builds, source_mapping_full_builds, word_count_scans, row_plan_rebuilds, roots_reprojected, outline_rebuilds, outline_lines_scanned, outline_full_rescans}`、
`DocumentTree::{whole_document_renders, snapshot_rebuilds}`。性能优化进行中的设计记录见 [performance.md](./performance.md)。

