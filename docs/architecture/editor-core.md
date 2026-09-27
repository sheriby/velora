# Editor Core（文档模型与编辑管线）

> 面向后续 agent 的代码导览。写作于 2026-09-28，基于 `perf` 分支。
> **行号会漂移，函数名不会**——引用以 `文件:函数名` 为主，行号仅作当时定位参考。
> 相关文档：[overview.md](./overview.md)、[render-pipeline.md](./render-pipeline.md)、[workspace-ui.md](./workspace-ui.md)、[testing-and-build.md](./testing-and-build.md)

## 1. 实体层次

```
Workspace (src/editor/workspace.rs)
  └─ Editor 实体（每个标签一个, src/editor/mod.rs struct Editor）
       └─ document: DocumentTree (src/editor/tree.rs)
            └─ roots: Vec<Entity<Block>>      ← 顶层块
                 └─ children: Vec<Entity<Block>>  ← 列表/引用的嵌套内容
```

- **`Editor`**（src/editor/mod.rs `struct Editor`）：窗口级控制器。持有 `view_mode`（`Rendered`/`Source`）、`document_dirty`/`document_revision`（脏标记与修订号，很多缓存的 key）、`file_path`、`file_version`（磁盘内容哈希，外部修改检测用）、undo/redo 栈、`last_stable_source_text`（稳定全文快照，undo/引用刷新复用，避免重复序列化）、`image_reference_definitions`/`link_reference_definitions`/`footnote_registry`（`Arc` 共享）、`table_cells: HashMap<EntityId, TableCellBinding>`、`outline_follow_cache`（按 revision 缓存）、`row_stride_cache: HashMap<EntityId, f32>`（渲染窗口用）。
- **`DocumentTree`**（src/editor/tree.rs）：`roots: Vec<Entity<Block>>` + `PendingTail`（分块导入时未消费的原始行，见 §3）+ `VisibleTreeSnapshot`（DFS 可见序、entity→索引/位置 映射，`rebuild_metadata_and_snapshot` 在结构变更后重建一次）。
- **`Block`**（src/components/block/runtime/mod.rs `struct Block`）：GPUI 实体。`record: BlockRecord`（持久数据）+ `children` + 运行时状态：`selected_range`（块内字节偏移选区）、`marked_range`（IME）、`edit_mode`（富文本渲染 vs 纯源码）、`projection`（行内定界符展开态）、`last_layout`/`last_bounds`（shaping 结果）、`table_runtime`、`folded`、`cached_display_text`（可见文本 `SharedString` 缓存）。
- **`BlockRecord`**（src/components/block/state.rs）：`{ id, kind: BlockKind, title: InlineTextTree, table, html, parent, content, raw_fallback }`。**块文本就是 `record.title`**。Raw 保留类块（RawMarkdown/Comment/HtmlBlock/MathBlock/Mermaid）把原始源码存在 `raw_fallback`（`kind_uses_raw_fallback`）。
- **行内内容**：`InlineTextTree = Vec<InlineFragment>`（src/components/markdown/inline.rs）。fragment 携带文本 + `InlineStyle` 标志 + 可选 link/footnote/math。**定界符不存储**，序列化时按规则重建（`serialize_markdown`）。渲染侧有 `InlineRenderCache`（可见文本 + spans + 双向偏移映射 `InlineMarkdownOffsetMap`）。

**没有全局行索引/rope**。跨块定位靠 `SourceTargetMapping`（见 §5）。

## 2. 导入：markdown → 块

- 入口 `Editor::from_markdown` → `from_markdown_with_chunk_budget`：CRLF→LF 规范化 → `markdown_requires_source_mode_fallback` 检查（不支持的构造整体降级为单 RawMarkdown 块 + Source 模式）→ `split_markdown_lines` 切行一次 → `build_root_block_chunk` 逐块构建。
- **解析器是手写逐行扫描器，不是 pulldown-cmark**（pulldown-cmark 只用于 HTML 导出）。分发顺序见 `build_blocks_from_lines_internal`（src/editor/document.rs）：frontmatter → 空行段 → 围栏代码 → fenced div → HTML 注释/块 → 脚注定义 → 引用定义 → setext 标题 → 独立图片 → 缩进代码 → 列表 → 引用块/callout → ATX 标题 → 分隔线 → 表格（含 pipeless）→ 展示数学 → 兜底段落。
- 每块行内解析：`native_block` → `InlineTextTree::from_markdown`。
- 无法表达的构造 → `raw_block`（`BlockKind::RawMarkdown`）逐字保留。
- **分块/渐进导入**（大文档关键）：`PendingTail` + `start_pending_materialization_task`/`materialize_next_pending_chunk`（src/editor/mod.rs）。预算 `FIRST_CHUNK_ROOTS=2000`、`STEADY_CHUNK_ROOTS=250`：首屏同步建 2000 块，其余后台每轮 250 块续建。需要全文的操作调 `flush_pending_materialization`。
- **纯文本/代码文件路径（性能敏感）**：`from_file_source`（src/editor/mod.rs）按 `workspace::is_code_file`（src/editor/workspace.rs，扩展名表含 `log/lock/toml/txt/csv/json/...`）分流 → `replace_document_from_code_source` → `replace_document_content`（src/editor/file_drop.rs）：**整个文件变成单个 `BlockKind::CodeBlock` 块**（`BlockRecord::with_plain_text`），Source 模式等宽编辑；CRLF 用 `code_uses_crlf` 标记保存时还原。markdown 兜底降级也是单 RawMarkdown 块。**这是大纯文本文件性能瓶颈的结构性根源（10MB log = 1 块 10MB 文本）**。

## 3. 编辑：按键 → 变更 → 序列化

1. 按键由焦点 **Block** 的 GPUI input handler 处理：`Block::replace_text_in_range`（src/components/block/input.rs）→ 计算 undo 类型 → `prepare_undo_capture` → `replace_text_in_visible_range`（src/components/block/runtime/mod.rs）修改 `record.title` 并 emit `BlockEvent::Changed`。
2. Editor 经 `on_block_event`（src/editor/events.rs，订阅点在 runtime_context.rs `new_block`）收所有块事件。结构性事件（换行/合并/缩进/粘贴等）经 `DocumentTree::insert_blocks_at` + `with_structure_mutation`（重建快照一次）改树。
3. `Changed` 之后：`mark_dirty`（src/editor/window_state.rs）推进 `document_revision` + `document_dirty` + `schedule_autosave`；`finalize_pending_undo_capture`（src/editor/history.rs）落 undo 条目；引用敏感块才刷新 image/link/footnote 运行时（`changed_block_needs_runtime_context_refresh`，src/editor/runtime_context.rs）。
4. **序列化是惰性的**：`DocumentTree::markdown_text` / `raw_source_text`（tree.rs，逐块 `BlockRecord::markdown_line`）只在保存、autosave 快照、undo 快照、引用注册表重建、跨块编辑时执行。按键路径从不重序列化全文。

## 4. Undo/历史（src/editor/history.rs）

- **全文源码快照制，不是 diff**：`HistoryEntry { source_text, selection, timestamp, kind }`。
- `prepare_undo_capture` 在编辑**前**拍快照（优先复用 `last_stable_source_text` 的廉价路径）；`finalize_pending_undo_capture` 编辑后落栈；`CoalescableText` 在 1s 窗口（`HISTORY_COALESCE_WINDOW`）内合并；栈深 `HISTORY_LIMIT=200`。
- undo/redo → `restore_history_entry`：Rendered 模式整树重建（`build_root_blocks_from_markdown`）；Source 模式替换单块。
- **大文件隐患**：每个 undo 条目持有整篇文档源码字符串（10MB 文档 × 200 条 ≈ 2GB 上限），大文件下 finalize 一次就要 clone 全文——编辑耗时与内存的主来源之一。

## 5. 选区/光标与源码映射（src/editor/source_mapping.rs）

- 每 Block 自持 `selected_range`（块内字节偏移）+ IME `marked_range`；焦点走每块 `focus_handle`；Editor 记 `active_entity_id`。
- 跨块选区：`CrossBlockSelection { anchor, focus }`（`{entity_id, offset}`，src/editor/selection.rs）。
- `SourceTargetMapping`：按块的字节映射表（块内容偏移 ↔ 序列化文档偏移），`build_source_target_mappings_until`（可提前停止）/ `source_mapping_for_entity`（单块短路）。用途：undo 选区恢复、跨块替换、大纲跟随滚动、光标历史、状态栏行列号。
- 表格单元格是独立 Block，经 `TableCellBinding` 绑定并单独建映射（`push_table_mappings`）。

## 6. 持久化（src/editor/persistence.rs）

- **原子写**：`write_atomic` = 同目录临时文件 + `sync_all` + `rename`。
- **外部修改检测**：`file_content_version`（规范化文本 DefaultHasher）；手动保存与 autosave 前 `verify_file_version` 重读比对，不一致则报「外部修改」。
- **Autosave**：`schedule_autosave` 防抖后台任务（默认 800ms，`[editor] autosave_debounce_ms`）；IME 组合中跳过；后台写恢复快照 + 临时文件，回主线程校对 revision 后落盘。
- **Watcher**（src/editor/watcher.rs）：每工作区递归 notify 监听，干净标签自动重载，脏标签走冲突提示。
- **关闭流**（src/editor/close.rs）：脏文档拦截为应用内对话框；保存后关闭经 `pending_close_after_save`。

## 7. 已知性能事实（perf 分支基线，dev 构建）

| 场景 | 数据 |
|---|---|
| 1MiB log（探针走 markdown 路径，全文 1 块） | 构建 1697ms / 首绘 655ms / **稳态绘制 647ms/帧** |
| 每帧全量克隆 | `render()` 中 `visible_blocks().to_vec()` + 折叠过滤扫描全部块 |
| 单块纯文本 | 非 markdown 文件整文件 1 个 CodeBlock，渲染/undo/序列化都压在一块 |

诊断探针：`VELORA_PERF_FILE=<file> cargo test manual_markdown_load_probe -- --ignored --nocapture`（src/editor/tests.rs）。性能优化进行中的设计记录见 [performance.md](./performance.md)。
