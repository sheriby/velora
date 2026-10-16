# 性能：可观测性、闸门与成本结构

> 基于 `dev`，2026-10-14 重写。这一册回答三件事：**成本怎么量**、**哪些路径禁止全文操作**、**优化按什么套路做**。
> 具体毫秒数会漂，别把这里的数字当基线；要数字就跑探针。相关：[editor-core.md](./editor-core.md) · [render-pipeline.md](./render-pipeline.md) · [testing-and-build.md](./testing-and-build.md)

## 1. 目标与测量

目标（用户设定）：1MiB/10MiB 级普通文本 **100ms 内打开 + 流畅滚动**；大 markdown 靠懒加载与裁剪渲染保证可用流畅度；整体大文件流畅度提升 ≥10x。验证在 dev 构建（热依赖已 opt-level=3，数字有代表性；本地代码 O0）。

```bash
node scripts/generate-fixtures.mjs fixtures/perf          # 夹具不进版本库，不生成则相关用例静默自跳

VELORA_PERF_FILE=<utf8 文件> cargo test manual_markdown_load_probe -- --ignored --nocapture
VELORA_PERF_FILE=<代码扩展名文件> cargo test manual_code_load_probe -- --ignored --nocapture
VELORA_STARTUP_TIMING=1 <velora>                          # 启动分阶段耗时 → stderr
cargo test probe_attribute_ten_mib -- --ignored --nocapture   # 大文档按键成本拆段归因
```

两个探针都在 `editor/tests/loading_chunks.rs`，输出 `construct_ms / first_draw_ms / steady_p95_ms / edit_update_ms / serialize_ms`。**夹具文件名必须带代码扩展名**才会走代码路径分流（无扩展名会误走 markdown 路径，断言会抓住）。`construct_ms` 含测试里同步跑完的续建部分，不代表真实打开时间；真实打开 = 读文件 + 同步构造首块 + 首帧，其余续建。

### 1.1 实测套件与基线（`editor/tests/perf_bench.rs`）

探针回答「这一下慢在哪」，实测套件回答「一次整篇工作在这台机器上要多少毫秒」，并把前后两次数出来比较。它不进 CI（墙钟在并发抢 CPU 下会假红），也不替代 §2 的计数器：**判断「有没有偷偷多做一遍整篇」看计数器，看单价与趋势看这里**。

```bash
cargo test --bin velora bench_suite -- --ignored --nocapture --test-threads=1
VELORA_BENCH_SAVE=1 cargo test --bin velora bench_suite -- --ignored --nocapture --test-threads=1   # 写/更新本机基线
VELORA_BENCH_SIZES=one-mib,ten-mib VELORA_BENCH_FILTER=model cargo test --bin velora bench_suite -- --ignored --nocapture --test-threads=1
VELORA_BENCH_TOLERANCE=5 cargo test --bin velora bench_suite -- --ignored --test-threads=1          # 收紧容差，验证这道门真会红
```

- **两组用例**：`text.*` 只要字符串，量一次整篇的单价（序列化、哈希、分词、引用定义解析）；`model.*` 要 gpui 的 App 上下文，量一次前景 `update` 里的整篇级工作（重建树、引用注册表重建、现场采集、兜底重投影），**不含渲染与字体度量**。按键那条 `model.keystroke_through_block` 走真的块事件路径，并检查文档修订号确实前进了——不然量的就不是按键。
- **怎么数**：一个单位 = 一次被量的工作。每个样本做多少个单位自动放大到 20ms 以上，否则纳秒级的工作会被计时粒度吞掉；报中位数、最小值与采样之间的散布，散布大说明这台机器此刻很吵。
- **基线一份机器一个文件**：`benchmarks/baseline.<os>.<arch>.dev.txt`，存原始纳秒。只跟本机那份比——同一份字节在 Intel、Windows、M 系上绝对值差得看不出趋势，跨机器比只会逼人放宽预算。换机器跑一次 `VELORA_BENCH_SAVE=1` 生成并提交自己的那份。保存时只覆盖这一轮跑到的用例，其余行留着，所以带过滤保存不会把基线跑丢。
- **默认容差 30%**（`DEFAULT_TOLERANCE_PCT`），慢过才红；快过容差只报一条提示，只有保存时才把数字写回文件。
- 缺夹具直接失败，不静默跳过：少了这一步就会得到一份「全绿但什么都没量」的基线。
- 档位与产品同档（测试跑的是 dev 构建：本地代码 O0，gpui/pulldown-cmark 这些重依赖继续吃 opt-level=3），所以量出来的就是用户那台机器上的成本。

一次实测（2026-10-16 重测，macos aarch64，dev 档，单线程；**别把这张表当基线，要数字就跑套件**）：

| 用例 | 1MiB | 10MiB | 10MiB/1MiB |
|---|---|---|---|
| `text.buffer_serialize` 一次整篇序列化 | 24.0µs | 249.3µs | 10 |
| `text.content_version` 一次整篇哈希 | 1.37ms | 13.77ms | 10 |
| `text.word_count` 一次整篇分词 | 6.57ms | 65.30ms | 10 |
| `text.reference_definitions` 一次整篇引用定义解析 | 37.12ms | 375.45ms | 10 |
| `model.document_rebuild` 一次整篇重解析建树 | 556.1ms | 6010ms | 10.8 |
| `model.image_runtime_rebuild` 一次整篇引用注册表重建 | 68.4ms | 797.0ms | 11.6 |
| `model.capture_document_view` 一次整篇现场采集 | 1.62ms | 16.15ms | 10 |
| `model.buffer_edit` 一次缓冲区写入 | 2.8µs | 20.4µs | 7.3 |
| `model.edit_and_undo_record` 一次写入加撤销记账 | 2.8µs | 20.3µs | 7.3 |
| `model.keystroke_through_block` 一次按键（走块事件） | 1.04ms | 3.38ms | 3.3 |
| `model.undo_document` 一次撤销（⌘Z） | 0.60s | 6.68s | 11.1 |
| `model.document_resync_fallback` 一次整篇兜底重投影 | 56.2ms | 604ms | 10.8 |

读法：比值贴着 10 的那几条是纯整篇工作（文档大 10 倍就贵 10 倍）。四行不是：

- **按键那一条现在涨 3.3 倍**（10MiB 一次 3.38ms；早先记的 23.7 倍是 `on_block_events` 对每个块事件克隆整篇可见块列表，已收掉，见 §4.1）。它是**热路径**的数：打开之后**头一次**按键还含行计划重建等一趟冷活，10MiB 实测约 159ms、1MiB 约 17ms（`perf_budgets.rs` 两条 `*_typing_stays_within_budget` 会把它打印出来）。
- **撤销那一条涨 11.1 倍**（10MiB 档 6.68s、1MiB 档 0.60s）。账在 §4.4：整篇重投影 10.6 万根块 + 投影重排 + 恰一遍引用注册表重建。早先记的 34 倍（62 秒）里有 9.2s/键是**测量装置**的错（用例攥着撤销**之前**那个块句柄打字，写回必然失败、每键退回整篇兜底重投影）；改成每轮重新取块之后才是这个数。
- **兜底重投影从 82 倍降到 10.8 倍**（`448c721` 把逐根块扫候选换成查表：10MiB 档 17.9s → 604ms）。它本来就一次都不该发生，形状闸门仍按着 0 次；但绝对值 604ms 说明这条退路依旧碰不得。
- **一次缓冲区写入涨 7.3 倍**（1MiB 2.8µs → 10MiB 20.4µs）：正文按 4KiB 一块存（`buffer.rs` 的 `MAX_CHUNK_BYTES`），10MiB 就是两千多块，每写一次都要把 `chunks` 这一列整体后移，`ChunkStarts::rebuild_from` 还要把改动点**之后**的前缀重算一遍（§4.5）。绝对值还很小（占一次按键的 0.6%），但它每键都付。

## 2. 计数器：成本的唯一读法

**任何"每帧/每键做了多少全文工作"都必须接进计数器**，否则闸门全绿也看不见它（历史上两处盲区：兜底重投影的源码分支不计序列化、侧栏大纲不在任何计数器里）。

`Editor`（`src/editor/mod.rs`，均为 `Cell<u64>`）：

| 计数器 | 数什么 | 正常编辑序列上应该是 |
|---|---|---|
| `source_serializations` / `_nanos` | 整篇重新序列化（兜底重投影、格式化文档命令） | 0（格式化命令除外） |
| `source_mapping_builds` / `_full_builds` / `_nanos` | 块内偏移映射重建次数 / 其中整篇的 | 只随改动块数走，整篇 0 |
| `word_count_scans` | 状态栏整篇分词 | 0（有静默窗口缓存） |
| `document_match_scans` | 文档内命中表整篇重扫 | 只随查询/修订变化 |
| `row_plan_rebuilds` / `row_plan_block_reads` / `_nanos` | 行计划重建次数 / 其中读了几个块实体 | 打字 0 次重建；重建时只读标题 |
| `roots_reprojected` | 增量重投影产出的根块数 | 只随本次改动 |
| `outline_rebuilds` / `outline_full_rescans` / `outline_lines_scanned` / `_nanos` | 大纲标题树重拼 / 整篇重扫 / 累计扫描行数 | 整篇重扫 0 |
| `panel_rows_rendered` / `panel_first_row_rendered` | 侧栏列表上帧渲染行数 / 窗口起始行 | 一帧只画视口那些行 |
| `line_prefix_from_record` / `line_prefix_measured` | 记号宽度取解析期账 / 事后按文件量 | 后者 0（迁移进度就看它） |
| `caret_scroll_applications`、`row_stride_cache_clears` | 光标滚动入视次数、行高缓存清空次数 | 不随文档大小增长 |
| `document_view_captures` | 采一次整篇阅读现场（视图模式 / 视口偏移 / 光标，即 `DocumentView`）：全文哈希 + 每行一条行测量 + 逐块读实体 + 撤销栈拷贝 | 自动保存 0 次；换标签、外部改动重载各 1 次 |
| `reference_registry_rebuilds` | 整篇引用注册表重建次数（`rebuild_image_runtimes`）：整篇 `buffer.text()` + 三类定义表重解析 + 逐块读实体并同步 | 只随「定义行本身变了」长，不随打字长 |
| `autosave_guard_block_reads` | 自动保存守卫（`has_marked_document_text`）读了多少块实体：它遍历全部可见块加全部表格格子 | 每个自动保存周期 1 次（`autosave_task` 在跑时短路），不是每键 |
| `outline_root_blocks_cloned` | 大纲同步时整篇克隆了多少根块（`root_blocks().to_vec()`） | 只有大纲真的要重建时才涨，稳态帧 0 |
| `status_bar_selection_block_reads` | 状态栏找单块选区时按可见顺序读了多少块实体 | 0（单块选区只在活动编辑目标那一块上） |
| `panel_fill_frames` / `cold_fill_frames` / `scroll_settle_frames`（`u8`） | 补帧计数 | 有上限 |

`DocumentTree`：`whole_document_renders`（**非兜底路径上悄悄发生的**整篇序列化）、`snapshot_rebuilds` / `snapshot_nanos`、`visible_blocks_cloned`（整篇可见块列表被克隆了几块：克隆一块要付一次 Entity 引用计数与 LeakDetector 登记，打字时必须是 0）。
`TextBuffer`：`line_probe_bytes`（建换行索引读了多少正文——查询行号必须读 0 字节）。
`BlockRecord`：`title_revision`（序列化备忘键）。

## 3. 闸门清单

**形状闸门**（断言计数器增量，确定性，随常规测试跑）：

- `a_real_editing_session_never_falls_back_to_whole_document_serialization` — 打字、拆块、勾任务框、缩进/提级/降级、标注里拆块、表格全套结构命令、多行粘贴逐条走一遍，整篇落笔增量为 0。白名单 `WHOLE_DOCUMENT_RESYNC_STILL_ALLOWED`（`editor/tests/round_trip_fidelity.rs`）是**空表**，每加一条命令只能让它更短。
- `a_real_editing_session_never_measures_marker_widths_after_the_fact` — 记号宽度不许事后按文件量（`line_prefix_measured` 全程 0）。
- `typing_a_plain_character_does_not_rebuild_the_row_plan`、`typing_in_a_heading_without_a_toc_block_reuses_the_row_plan`、`row_plan_rebuild_reads_only_the_headings_not_every_block`。
- `per_keystroke_document_passes_stay_bounded`、`typing_does_not_rescan_status_bar_statistics_every_key`。
- `typing_does_not_clone_the_visible_block_list` — 一次按键不许克隆整篇可见块列表（`DocumentTree::visible_blocks_cloned` 增量为 0；这条闸门同时证明计数器自己是活的）。
- `steady_frames_do_not_clone_the_outline_root_blocks` — 侧栏打开时稳态帧不许克隆整篇根块（`outline_root_blocks_cloned` 增量为 0；拆一次块把计数器推到非 0，防止闸门量不到东西）。
- `status_bar_finds_the_selection_on_the_active_block_only` — 状态栏找选区文本只读活动块，且不把别的块上的残影选区报出来。
- `chunk_lookups_do_not_scan_every_chunk` — 按偏移/行号取字节不许从第 0 块线性找（`ChunkStarts::probes` ≤ 32 次比较 / 272 块）。
- `reattaching_root_spans_does_not_scan_candidates_per_block`、`undo_does_not_fall_back_to_a_whole_document_resync` — 整篇落笔给根块挂区间是每根块查一次表；撤销收尾不许再序列化整篇。
- `typing_a_link_paragraph_does_not_rebuild_the_reference_registry` — 段落里带 `[文字](链接)` 或 `a < b` 时打字不许触发整篇引用注册表重建；正控是改定义行必须触发。
- `introducing_a_reference_definition_refreshes_the_reference_registry` — 屏幕看的是**落笔前**的缓冲区字节，所以「原来没有 `]:`/`[^`、这一下才写进去」的编辑（粘贴一行定义、把定义搬过来）要由落笔后再问一次兜住；`renaming_a_footnote_definition_sub_block_refreshes_the_registry` 钉住另一半：没有源码区间的定义类子块（容器里的脚注定义，标题就是标签本身）只能靠宽口径里的 kind 那一档认出来。两条修复前都红。
- `a_directory_that_failed_to_scan_is_not_retried_every_frame` — 侧栏目录扫失败要留痕：面板补齐挂在每帧的 `sync_workspace_models` 上，失败不留痕就是每帧一次 `read_dir` 加一次 `notify`（修复前实测 5 帧 5 次重试）；`a_walk_reports_how_many_entries_it_could_not_read` 钉住走盘名单的跳过条目要有计数。
- `a_second_display_render_does_not_read_the_disk_again` — 同一条 mermaid 的第二次渲染一次盘都不许读（`TEST_CACHE_READS`）。
- `autosave_does_not_capture_the_reading_position` — 自动保存的那一下只同步内容与脏位，
  不许采整篇阅读现场；同时守住「换标签必须采一次」，防止把该记的现场一并删掉。
- `the_incremental_outline_matches_a_whole_document_scan`（差分 + 钉住"全程一次整篇重扫都不许发生"——只比结果的话退回整篇照样一致，等于自证）、`the_outline_is_only_built_for_a_reader`、`outline_panel_renders_only_the_rows_in_the_viewport`。
- `clicking_an_outline_heading_unfolds_it_without_a_document_wide_mapping`、`document_find_highlights_map_only_the_blocks_with_hits`、`typing_at_the_document_head_keeps_every_root_span_tiled`。
- `asking_for_line_numbers_does_not_read_the_text`、`rebuilding_the_line_index_costs_only_the_edited_bytes`。
- `undo_memory_does_not_scale_with_document_size`、`two_hundred_undo_steps_stay_within_the_memory_budget`。

**墙钟闸门**（`#[ignore]`，本地手动跑，CI 永不跑；并发抢 CPU 下会假红，单跑能过就按假红处理，不要放宽预算）：`large_document_opens_within_budget`、`large_code_document_opens_within_budget`、`one_mib_typing_stays_within_budget`、`ten_mib_typing_does_not_scan_the_whole_document`、`one_mib_source_mode_typing_stays_within_budget`、`one_mib_code_document_typing_stays_within_budget`。

**实测套件**（§1.1，`perf_bench.rs` 的 `bench_suite_measures_the_core_costs`）同样 `#[ignore]`、CI 里 `--skip perf_bench` 排掉：它量的是这台机器上的单价，与 `benchmarks/` 里本机基线比趋势，慢过 30% 才红。它不数「做了多少遍整篇」，那还是计数器的活。

> 并行跑测下**绝对墙钟会误报**：新写性能用例优先断言形状（计数器、相对关系、访问规模），不要断言毫秒数。

> **量滚动窗口的闸门必须注入真滚轮会产生的偏移（负值）**：gpui 的内容偏移是「已滚过距离的相反数」（`workspace/tests/sidebar.rs` 用真滚轮断言过 `offset < 0`），注入正值是物理上到不了的状态——窗口算式把符号搞反也照样绿。三条视口闸门（文件树 / 搜索 / 大纲）都踩过这个坑，实测「滚到第 1000 行」时窗口停在 `0..52`。

## 4. 成本结构：什么已经收敛，什么还随文档长

**与文档大小无关（已收敛）**：稳态帧（不打字不重算任何东西）· 打字后的整篇序列化（0 次）· 引用注册表重建（只在改动块的**源码字节**出现 `]:` 或 `[^` 时才走：带 `[文字](链接)` 的段落、`a < b` 这类正文不再触发；10 MiB 档它一次 766ms）· mermaid 重绘（进程内缓存命中，一次盘都不读）· 行号查询（只二分每块的换行偏移表）· 大纲（按根块增量，没人看就不算）· 行计划（打字不重排，缓存键是结构版本不是文档修订）· 区间平移（树侧一趟循环）· 搜索高亮（只为有命中的根块建映射）· shape（跨帧备忘）· 撤销内存（存增量）。

**仍与文档大小同向**：

1. **可见列表重排与行计划重建的剩余部分**。渲染态 markdown 大文档的根块数就是文档规模；源码视图按固定行数切片，块数少一个量级，但**这一档的可见列表不裁剪**（所有切片块都算"可见"），每键的行计划工作随块数走。要解掉需要"块内/列表也按行窗口投影"。
   这一档的 23.7 倍超线性曾经被归到这里，`sample` 采样证明不是：10MiB 档那次按键的 17ms 全花在 `on_block_events` 对**每一个块事件**克隆整篇可见块列表（`flatten_visible_blocks` → 每块一次 Entity 引用计数 + `LeakDetector::handle_created/released` + 句柄表插入），而文本改动只需要比「可见序列有没有变」。改成身份序列（`visible_block_ids`，`EntityId` 是 `Copy`）之后，同一台机器上 `probe_keystroke_stage_split` 的块侧耗时 17.4ms → 3.7ms（10MiB、连敲 200 键），`model.keystroke_through_block` 单跑 3.38ms／1MiB 档 1.04ms。剩下的 3.7ms 里还有整篇 `Chunk::line_count` 遍历与被编辑块自己的行内分隔符解析——**这一档现在才轮到行窗口那件事**，闸门见 §3 的 `typing_does_not_clone_the_visible_block_list`。
2. **大纲标题树整篇一份**：节点 id 记着整篇行号，任何一处改动都重拼全树。已优化掉的是"只有文字变了"那一档（原地换标签，不重拼）；剩下的要按改动那段增量拼。另有两条已经收掉的每帧克隆：大纲同步的短路判断原来先 `root_blocks().to_vec()` 再比长度（侧栏打开时每帧一次，10MiB 档 106,456 根），现在比 `root_count()`；状态栏找选区原来从第一块起逐块扫（10MiB 档 159,683 块/帧），现在只看活动编辑目标那一块。
3. **一次性全文操作**：整篇序列化、整篇映射重建、全文分词——都不在按键路径上，但格式化文档、全部替换、导出会付一次。单价见 §1.1（10MiB：序列化 261.7µs、哈希 13.81ms、分词 65.29ms、引用定义解析 374.43ms、重建树 8.5s）。
4. **撤销与兜底重投影（已实测）**：一次 ⌘Z 在 10MiB 档 **6.96s**、1MiB 档 0.60s
   （`model.undo_document`，用例已修，见下「测量装置的坑」）。这条路的账（一次撤销的
   计数器增量，`probe_undo_stage_counters`）：
   - 整篇重投影 **106,456 根块** + 投影重排 1 次（约 0.5s）——这是当前的大头。
   - 整篇引用注册表重建 **1 次**（10MiB 档 766ms）。它原本是 2 次：
     `rebuild_document_from_buffer` 先调 `rebuild_table_runtimes`（收尾已经刷过引用
     注册表）又直接调一次 `rebuild_image_runtimes`，同一份缓冲区、同一棵树连着重建
     两遍——已删掉重复那次，闸门 `undoing_once_rebuilds_the_reference_registry_exactly_once`
     钉住「恰好一遍」。
   - 可见块列表整篇克隆 2 次（319,366 块）——`flatten_visible_blocks` 在撤销路径上还有
     调用点，量级不大（约 30ms）但属于同一族，值得下次清掉。
   - `reattach_root_spans`（重挂根块区间）**不在撤销路径上**（探针：候选比较 0 次）；
     它的查表化是给**打开/整篇落笔**那条路的（`finish_pending_materialization` 与
     resync 会走），别把撤销的提速记在它头上。
   - 兜底重投影那一笔（10MiB 档 17.9s）在撤销里**本来就被跳过**：`replay_history_group`
     置了 `skip_next_resync`（闸门 `undo_does_not_fall_back_to_a_whole_document_resync`）。
   - **测量装置的坑**：跨「换掉整棵树」的操作（撤销、切视图）复用块句柄，会让下一键
     落进整篇重投影。性能用例与探针都要每轮重新取块（`first_block(&editor, cx)`）。
   再降一档只有把撤销接到增量重投影上：`import.rs:reproject_root_region` 按根块重投影，
   唯一生产调用点在 `history.rs` 的引用结构规范化。**试过一版，卡在两处，按现状退回**：
   1. 区域边界对不上：`reproject_root_region` 要求窗口解析恰好在「本段行数」处给出一个
      根边界，而根的区间右端按导入器口径**带着块尾空行**、窗口解析不把空行算进这一根
      （实测：一次撤销改到段落，区域 2 行，解析出 `span 末端 [1, 3]`，`within=1` 而
      `1 != 2` → 返回 `None` → 退回整篇）。要么统一「空行归谁」的口径，要么让区域由
      **解析结果**去对齐而不是由根的区间去推。
   2. 第二版（放宽验收：只要解析出的根不越进下一根原来的块就收）之后，增量那条路真的
      跑起来了，却在**收尾刷引用注册表**时崩：`block_may_change_document_references` 拿
      某根块的源码区间去 `buffer.slice`，区间起点落在 '第' 中间（1112..1131）。调用栈
      （`slice` 里临时打的 `Backtrace::force_capture`）指着
      `rebuild_table_runtimes → rebuild_image_runtimes → block_may_change_document_references`。
      **可疑点**：给那次切片加的「区间端点是不是字符边界」体检**没有触发**，说明
      `TextBuffer::is_char_boundary` 与 `TextBuffer::slice` 对同一个偏移给出了不同答案——
      两处都走 `starts.locate`，那就得查增量重投影之后**区间本身**是不是对不上了
      （部分重投影只重挂新根，旧根的区间靠 `shift_source_spans_after` 平移）。
      下一版先写一条差分测试：「任意编辑序列之后，所有根的区间都必须落在字符边界上、
      且互不重叠、按文档顺序单调」，再动增量。
      **闸门已就位**（`root_spans_stay_on_char_boundaries_across_edits_and_undo`）：
      今天的产品路径（撤销走整篇重建）上这条不变式一路成立；把增量接回去再跑，它崩在
      `slice`（'第' 中间，偏移 718..737）。**已定位到确切位置**（安全诊断：在 `slice`
      里打 `Backtrace::force_capture`，消息里绝不切片）：

      ```
      [slice-raw] 区间 718..737
        TextBuffer::slice → Editor::history_row_signature → restore_history_row_layouts → undo_document
      ```

      也就是说：**增量重投影之后某根块的区间起点落在了字符中间**（718 是 '第' 的第二个
      字节），而 `restore_history_row_layouts` 里的 `history_row_signature` 直接拿区间去
      `slice`，于是崩在撤销收尾。同一个坏区间在收尾更早的「刷引用注册表」那一步也出现过
      ——那处已经加了可信性闸（不可信就退回保守口径，不再崩）。
      下一版的第一个断言点因此是**区间的产地**而不是消费者：在
      `root_span_ranges`（`attach_root_spans` 用它把行区间换成字节区间）里断言算出来的
      start/end 都在字符边界上——它一响就能指出是「增量那条 attach」还是「平移那条
      `shift_source_spans_after`」把区间弄坏的。三道保险（`slice` 留诊断、消费者加闸、
      产地加断言）之后，再谈把增量接回主线。

      （另：早先记的「`is_char_boundary` 与 `slice` 矛盾」是错的——那次体检在拼消息时
      自己调了 `slice`，打印前就崩了，见下一轮修正。）

   改动本身（判定改动字节范围 → 落在一根块里就只重投影那一根，否则退回整篇）已经写出
   来过，卡的是上面两处；谁接着做，先给 `reproject_root_region` 补一条「区域与解析边界
   对不上时怎么办」的判据，再谈接撤销。
5. **缓冲区自己的写入**：正文按 4KiB 一块存（`buffer.rs` 的 `MAX_CHUNK_BYTES`），一次写入要把 `chunks` 这一列整体后移，`ChunkStarts::rebuild_from(start - 1)` 还要把改动点**之后**的全部前缀重算一遍（`edit` 里那句「前缀索引只重建这一段」说的是**前缀没变**，不是尾段不扫：改在第一块时它就是整篇）。1MiB 档 2.8µs、10MiB 档 20.4µs（≈7.3 倍，随块数线性）。这笔账是 2026-10-15 的前缀索引（`c3e9e16`）换来的：`locate` / `line_of` / `slice` 从「从第 0 块线性找」改成二分，撤销那条路上省的是几十秒（§4.4），代价就是每键 O(尾块数)。绝对值还很小，但它是每键都付的，别把它当成常数；要再降一档得把尾巴从「逐块重算」改成「整体加常数」，或者换掉这层扁平索引。

6. **行高起估为什么还是「一行高」**（试过，退回来了）。渲染态没量过的行一律按
   `block_min_height`（28px）起估；想按「这块有几行」放大时踩到三件事：
   - 行元数据里那个 `source_document_lines` **只在开源码行号（源码模式）时非 0**，
     渲染态恒为 0（`Block::source_document_line_count`）。也就是说「数据已在快照里」
     这个前提不成立——要放大得先新加一份「显示文本行数」的元数据（`display_line_count`
     那个方向，随可见列表同步那一趟一起记）。
   - 放大必须**限在渲染态**：源码/代码视图的块是固定切片，高度账在滚动条权重那边
     （`scrollbar_strides` 已经是 `estimate × source_lines`），布局这一侧再乘一次会把
     `scrollbar_reaches_the_end_when_the_last_source_line_is_visible` 算飞（末块压根
     挂不上，4 帧后 `last_bounds` 还是 None）。
   - 即便限在渲染态，`undo_and_redo_a_visible_character_keep_the_viewport_still` 也会红：
     未实测行的起估一变，偏移映射跟着变（实测 -17593px → -35618px，而可见字没动）。
     真要放大，得先做出「起估变化时按内容锚点还原视口」这一条。

## 5. 优化套路（这个仓库反复用的四招）

1. **脏区间 + 增量重算**：缓冲区记 `take_dirty_region`，派生视图只重算相交的部分；结构命令重新分配接缝时要显式 `note_outline_dirty_region`（块自己的字节没动但指向变了）。
2. **缓存键要含身份与结构版本**：`identity + revision` 缺一不可（两份未编辑文档 revision 同为 0）；跨接缝的状态（围栏）要进键；行计划用 `row_meta_version` 而不是文档修订。
3. **没人看就不算**：侧栏收起时大纲一帧都不算；`[TOC]` 块在自己需要的那一帧直接要清单；状态栏统计等文档安静下来再补算。
4. **把逐块读实体换成同趟记下的元数据**：可见列表同步那一趟顺手把结构量存进快照（两块 vec 同进同出），下游不再逐个 `read` 实体；`Entity::read_untracked` 用于纯读的热路径（跳过 accessed 集合登记）。

改性能相关的代码，收尾要跑对应闸门并**当场实测数字**（不沿用记忆），把结论写进提交正文。
