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

一次实测（2026-10-15，macos aarch64，dev 档，单线程；**别把这张表当基线，要数字就跑套件**）：

| 用例 | 1MiB | 10MiB | 10MiB/1MiB |
|---|---|---|---|
| `text.buffer_serialize` 一次整篇序列化 | 23.8µs | 261.7µs | 11 |
| `text.content_version` 一次整篇哈希 | 1.37ms | 13.81ms | 10 |
| `text.word_count` 一次整篇分词 | 6.44ms | 65.29ms | 10 |
| `text.reference_definitions` 一次整篇引用定义解析 | 37.14ms | 374.43ms | 10 |
| `model.document_rebuild` 一次整篇重解析建树 | 628.6ms | 8506ms | 13.5 |
| `model.image_runtime_rebuild` 一次整篇引用注册表重建 | 64.1ms | 766.5ms | 12 |
| `model.capture_document_view` 一次整篇现场采集 | 1.63ms | 16.62ms | 10 |
| `model.buffer_edit` 一次缓冲区写入 | 1.6µs | 10.7µs | 6.7 |
| `model.edit_and_undo_record` 一次写入加撤销记账 | 1.6µs | 10.7µs | 6.7 |
| `model.keystroke_through_block` 一次按键（走块事件） | 1.61ms | **38.15ms** | 23.7 |
| `model.document_resync_fallback` 一次整篇兜底重投影 | 217.4ms | 17869ms | 82 |

读法：`text.*` 那四条和 `capture_document_view` 的比值贴着 10（文档大 10 倍就贵 10 倍），`document_rebuild`、`image_runtime_rebuild` 略高（12~13.5），这几位都是纯整篇工作。三行不是：**按键那一条涨 23.7 倍**（10MiB 档一次按键 38ms，一秒钟敲不到 26 个字——这是 §4.1 说的「渲染态根块数就是文档规模」那一档，块内/列表没按行窗口投影）；**兜底重投影涨 82 倍**（超线性，一次都碰不得，所以 §3 的形状闸门把它按在 0 次）；**一次缓冲区写入涨 6.7 倍**（1.6µs→10.7µs：正文按 4KiB 一块存（`buffer.rs` 的 `MAX_CHUNK_BYTES`），10MiB 就是两千多块，每写一次都要把 `chunks` 这一列整体后移，`locate()` 还是沿块线性找下标；dev 档的 `debug_assert` 也走这一位，发布包会轻一些）。

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
| `panel_fill_frames` / `cold_fill_frames` / `scroll_settle_frames`（`u8`） | 补帧计数 | 有上限 |

`DocumentTree`：`whole_document_renders`（**非兜底路径上悄悄发生的**整篇序列化）、`snapshot_rebuilds` / `snapshot_nanos`。
`TextBuffer`：`line_probe_bytes`（建换行索引读了多少正文——查询行号必须读 0 字节）。
`BlockRecord`：`title_revision`（序列化备忘键）。

## 3. 闸门清单

**形状闸门**（断言计数器增量，确定性，随常规测试跑）：

- `a_real_editing_session_never_falls_back_to_whole_document_serialization` — 打字、拆块、勾任务框、缩进/提级/降级、标注里拆块、表格全套结构命令、多行粘贴逐条走一遍，整篇落笔增量为 0。白名单 `WHOLE_DOCUMENT_RESYNC_STILL_ALLOWED`（`editor/tests/round_trip_fidelity.rs`）是**空表**，每加一条命令只能让它更短。
- `a_real_editing_session_never_measures_marker_widths_after_the_fact` — 记号宽度不许事后按文件量（`line_prefix_measured` 全程 0）。
- `typing_a_plain_character_does_not_rebuild_the_row_plan`、`typing_in_a_heading_without_a_toc_block_reuses_the_row_plan`、`row_plan_rebuild_reads_only_the_headings_not_every_block`。
- `per_keystroke_document_passes_stay_bounded`、`typing_does_not_rescan_status_bar_statistics_every_key`。
- `autosave_does_not_capture_the_reading_position` — 自动保存的那一下只同步内容与脏位，
  不许采整篇阅读现场；同时守住「换标签必须采一次」，防止把该记的现场一并删掉。
- `the_incremental_outline_matches_a_whole_document_scan`（差分 + 钉住"全程一次整篇重扫都不许发生"——只比结果的话退回整篇照样一致，等于自证）、`the_outline_is_only_built_for_a_reader`、`outline_panel_renders_only_the_rows_in_the_viewport`。
- `clicking_an_outline_heading_unfolds_it_without_a_document_wide_mapping`、`document_find_highlights_map_only_the_blocks_with_hits`、`typing_at_the_document_head_keeps_every_root_span_tiled`。
- `asking_for_line_numbers_does_not_read_the_text`、`rebuilding_the_line_index_costs_only_the_edited_bytes`。
- `undo_memory_does_not_scale_with_document_size`、`two_hundred_undo_steps_stay_within_the_memory_budget`。

**墙钟闸门**（`#[ignore]`，本地手动跑，CI 永不跑；并发抢 CPU 下会假红，单跑能过就按假红处理，不要放宽预算）：`large_document_opens_within_budget`、`large_code_document_opens_within_budget`、`one_mib_typing_stays_within_budget`、`ten_mib_typing_does_not_scan_the_whole_document`、`one_mib_source_mode_typing_stays_within_budget`、`one_mib_code_document_typing_stays_within_budget`。

**实测套件**（§1.1，`perf_bench.rs` 的 `bench_suite_measures_the_core_costs`）同样 `#[ignore]`、CI 里 `--skip perf_bench` 排掉：它量的是这台机器上的单价，与 `benchmarks/` 里本机基线比趋势，慢过 30% 才红。它不数「做了多少遍整篇」，那还是计数器的活。

> 并行跑测下**绝对墙钟会误报**：新写性能用例优先断言形状（计数器、相对关系、访问规模），不要断言毫秒数。

## 4. 成本结构：什么已经收敛，什么还随文档长

**与文档大小无关（已收敛）**：稳态帧（不打字不重算任何东西）· 打字后的整篇序列化（0 次）· 行号查询（只二分每块的换行偏移表）· 大纲（按根块增量，没人看就不算）· 行计划（打字不重排，缓存键是结构版本不是文档修订）· 区间平移（树侧一趟循环）· 搜索高亮（只为有命中的根块建映射）· shape（跨帧备忘）· 撤销内存（存增量）。

**仍与文档大小同向**：

1. **可见列表重排与行计划重建的剩余部分**。渲染态 markdown 大文档的根块数就是文档规模；源码视图按固定行数切片，块数少一个量级，但**这一档的可见列表不裁剪**（所有切片块都算"可见"），每键的行计划工作随块数走。要解掉需要"块内/列表也按行窗口投影"。§1.1 量到的单价：一次按键 1MiB 档 1.61ms、10MiB 档 38.15ms（涨 23.7 倍，比文档本身快得多）。
2. **大纲标题树整篇一份**：节点 id 记着整篇行号，任何一处改动都重拼全树。已优化掉的是"只有文字变了"那一档（原地换标签，不重拼）；剩下的要按改动那段增量拼。
3. **一次性全文操作**：整篇序列化、整篇映射重建、全文分词——都不在按键路径上，但格式化文档、全部替换、导出会付一次。单价见 §1.1（10MiB：序列化 261.7µs、哈希 13.81ms、分词 65.29ms、引用定义解析 374.43ms、重建树 8.5s）。
4. **缓冲区自己的写入**：正文按 4KiB 一块存（`buffer.rs` 的 `MAX_CHUNK_BYTES`），一次写入要把 `chunks` 这一列整体后移，`locate()` 沿块线性找下标。1MiB 档 1.6µs、10MiB 档 10.7µs——绝对值还很小，但它是每键都付的，别把它当成常数。

## 5. 优化套路（这个仓库反复用的四招）

1. **脏区间 + 增量重算**：缓冲区记 `take_dirty_region`，派生视图只重算相交的部分；结构命令重新分配接缝时要显式 `note_outline_dirty_region`（块自己的字节没动但指向变了）。
2. **缓存键要含身份与结构版本**：`identity + revision` 缺一不可（两份未编辑文档 revision 同为 0）；跨接缝的状态（围栏）要进键；行计划用 `row_meta_version` 而不是文档修订。
3. **没人看就不算**：侧栏收起时大纲一帧都不算；`[TOC]` 块在自己需要的那一帧直接要清单；状态栏统计等文档安静下来再补算。
4. **把逐块读实体换成同趟记下的元数据**：可见列表同步那一趟顺手把结构量存进快照（两块 vec 同进同出），下游不再逐个 `read` 实体；`Entity::read_untracked` 用于纯读的热路径（跳过 accessed 集合登记）。

改性能相关的代码，收尾要跑对应闸门并**当场实测数字**（不沿用记忆），把结论写进提交正文。
