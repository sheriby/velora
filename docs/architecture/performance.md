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

> 并行跑测下**绝对墙钟会误报**：新写性能用例优先断言形状（计数器、相对关系、访问规模），不要断言毫秒数。

## 4. 成本结构：什么已经收敛，什么还随文档长

**与文档大小无关（已收敛）**：稳态帧（不打字不重算任何东西）· 打字后的整篇序列化（0 次）· 行号查询（只二分每块的换行偏移表）· 大纲（按根块增量，没人看就不算）· 行计划（打字不重排，缓存键是结构版本不是文档修订）· 区间平移（树侧一趟循环）· 搜索高亮（只为有命中的根块建映射）· shape（跨帧备忘）· 撤销内存（存增量）。

**仍与文档大小同向**：

1. **可见列表重排与行计划重建的剩余部分**。渲染态 markdown 大文档的根块数就是文档规模；源码视图按固定行数切片，块数少一个量级，但**这一档的可见列表不裁剪**（所有切片块都算"可见"），每键的行计划工作随块数走。要解掉需要"块内/列表也按行窗口投影"。
2. **大纲标题树整篇一份**：节点 id 记着整篇行号，任何一处改动都重拼全树。已优化掉的是"只有文字变了"那一档（原地换标签，不重拼）；剩下的要按改动那段增量拼。
3. **一次性全文操作**：整篇序列化、整篇映射重建、全文分词——都不在按键路径上，但格式化文档、全部替换、导出会付一次。

## 5. 优化套路（这个仓库反复用的四招）

1. **脏区间 + 增量重算**：缓冲区记 `take_dirty_region`，派生视图只重算相交的部分；结构命令重新分配接缝时要显式 `note_outline_dirty_region`（块自己的字节没动但指向变了）。
2. **缓存键要含身份与结构版本**：`identity + revision` 缺一不可（两份未编辑文档 revision 同为 0）；跨接缝的状态（围栏）要进键；行计划用 `row_meta_version` 而不是文档修订。
3. **没人看就不算**：侧栏收起时大纲一帧都不算；`[TOC]` 块在自己需要的那一帧直接要清单；状态栏统计等文档安静下来再补算。
4. **把逐块读实体换成同趟记下的元数据**：可见列表同步那一趟顺手把结构量存进快照（两块 vec 同进同出），下游不再逐个 `read` 实体；`Entity::read_untracked` 用于纯读的热路径（跳过 accessed 集合登记）。

改性能相关的代码，收尾要跑对应闸门并**当场实测数字**（不沿用记忆），把结论写进提交正文。
