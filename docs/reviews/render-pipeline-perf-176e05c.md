# Velora 渲染管线性能分析（几千行 / 几万行两档）

> 固定点：`176e05c`（当前 HEAD）。工作树里另有使用者未提交的改动，本报告未触碰产品代码。
> 范围：`Editor::render` 每帧入口 → 行计划与视口虚拟化 → 块与文字元素 → 滚动同步与几何回填；不含导出、搜索、工作区索引。
> 方法：仓库内无窗口测试探针（临时文件，跑完已删除）+ 独立微基准复刻关键循环；**真字体成本**用「临时把测试平台的文本系统换成 macOS `MacTextSystem`」直接量（同一套探针，跑完已还原 vendor）。
> 环境边界：dev 构建（本地 crate O0，热依赖 O3）。真字体部分是真的 CoreText 排版（判据见 §2）；**仍未覆盖**字形光栅与 GPU 提交（测试平台不提交合成，gpui 的光栅走 atlas 缓存）。
> 与 [performance.md](../architecture/performance.md) 的关系：那本是优化台账（大文件目标与已完成项），本份是当前管线在**两档真实文档规模**上的成本拆解与待办。

## 0 摘要

- **几千行档（1k–12k 行）**：算法账基本清了。3,000 行静态帧 1.4ms、滚动 300px/帧 2.3ms（dev，真字体）。这一档感知到的卡顿**不可能**来自整篇扫描（那在 3,000 行只值 0.5–0.7ms）。真字体实测把剩余嫌疑缩到两处：**大块的冷排版**（512 行块 = 21ms/块）与**每帧固定成本**；源码模式另有一条明确缺陷——**行号每帧重排版**（冷 15.3µs/次，热 0.2µs/次，512 行块一次就要 7.8ms）。
- **几万行档（2 万–4 万行 / 1.6 万块起）**：主因是**每帧两趟与文档等长的扫描**，它们占整帧约 2/3；核心渲染只占 1/6。滚动时再叠加**每帧整条 stride 前缀重算**（O 剩余行数）与**提前量轮换挂载集合**；代码文件（512 行/块）还会出现**单帧挂载 51 个分块**的尖峰（Noop 测到 763ms 单帧，真字体下按每块 21ms 只会更糟）。
- **真字体不是"几千行 markdown 变卡"的原因**：短段落冷排版 19.5µs/块，快滚一帧挂 60 行 ≈1.2ms；同 fixture 下真字体帧成本**不高于**等宽模拟（Noop 的假排版路径反而更慢）。真字体只在**大块**上狠狠收钱。
- **空白帧**：本批 fixture 里**没有复现出视口内部空白带**；唯一测到的是文档末尾半屏尾距（设计内）。会产生"晚几帧才铺满"的是冷启动续帧窗口（≤8 帧 ≈130ms），以及"一帧要付 21ms×N 个大块"的长帧。

## 1 测量口径

| 项 | 值 |
|---|---|
| 每帧地板 | **0.33 ms**（2 行文档；元素树重建、Theme 深拷贝、命中区、GPUI 提交路径） |
| 行数线性项 | ≈ **0.2–0.25 µs/行**（dev/Noop；12,000 行文档静态 3.35ms − 地板 0.33ms） |
| 核心渲染（挂载行本身） | 40,000 行档 **1.36 ms/帧**（侧栏关 + 状态栏扫描短路，Noop） |
| 真字体冷排版 | 短段落 **19.5 µs/块**；20 行代码块 19.3 µs/块；512 行块 **13.59 ms/块**；单行行号 15.3 µs/次 |
| 真字体热（帧级缓存命中） | **0.2–6 µs/次** |

## 2 真字体实测（macOS CoreText）

做法：临时把 `vendor/gpui/src/platform/test/platform.rs:105` 的 `NoopTextSystem` 换成 `crate::MacTextSystem::new()`（仅 macOS），跑同一批探针后 `git checkout` 还原。
判据（等宽模拟下这两者必相等）：`width("iiii")=14.56px`、`width("WWWW")=60.69px`、`width("中")=15.88px` → 确实是真排版。

### 2.1 单项排版成本

| 项 | 冷（文本首次） | 热（同一条文本重复，帧级缓存命中） |
|---|---|---|
| 短段落（~40 字 CJK+ascii） | **19.5 µs** | 4.8–6.7 µs |
| 短段落 + 写作列宽换行 | — | 0.3 µs |
| 20 行代码块 | **19.3 µs** | 5.7 µs |
| **512 行代码块正文** | **13.59 ms** | — |
| 单行行号（1–3 位） | **15.3 µs** | 0.2 µs |
| 一批 512 个行号 | **7.81 ms** | 0.2 µs/次 |
| 一批 512 个四位行号 | （首遍 15.3µs/次摊进均值 3.3µs） | 0.2 µs/次 |

gpui 的排版缓存是**帧级**的（`vendor/gpui/src/text_system/line_layout.rs:438` `previous_frame`/`current_frame`，命中即留在 current）：静止帧是热态，**新挂载的块**或**被挤出后又回来的块**要再付冷价。

### 2.2 整帧成本（同一批 fixture：真字体 vs 等宽模拟）

| 场景 | 真字体 | Noop 对照 |
|---|---|---|
| 3,000 行 markdown 静态 | **1.41 ms** | 1.51 ms |
| 3,000 行 markdown 滚动 300px/帧 | **2.33**（最慢 2.55） | 2.02（最慢 2.22） |
| 3,000 行 markdown 滚动 3000px/帧 | **6.58**（最慢 7.27，挂 61–62 行） | — |
| 20,000 行 markdown 静态 | **3.08** | 4.8 |
| 20,000 行 markdown 滚动 300px/帧 | **4.52**（最慢 5.51） | 5.7（最慢 6.7） |
| 20,000 行 markdown 滚动 3000px/帧 | **9.00**（最慢 10.16） | 12.4（最慢 60.8） |
| 源码模式 3,000 行静态 | **0.72** | 0.60 |
| 源码模式 3,000 行滚动 3000px/帧 | **1.72**（最慢 4.50） | 1.06（最慢 1.54） |

读法：
- 短块 markdown 里，真字体**没有**把帧成本抬起来（一帧挂 60 行 ≈1.2ms 排版），有些档比 Noop 还快——Noop 的假排版路径（逐字推进 + 分配）本身不便宜，不能拿它当"乐观下界"。
- 真字体真正的账在**大块**：512 行块 = 13.59ms（正文）+ 7.81ms（行号）= **≈21ms/块**。一帧挂 2 个就掉帧；1.5MB 代码文件那种"一帧挂 51 个分块"的场景，真字体下是秒级。

## 3 第一档：几千行

### 3.1 实测

| 文档 | 静态帧 | 滚动 300px/帧（平均 / 最慢） |
|---|---|---|
| 2 行（地板） | 0.33 ms | — |
| 1,000 行 | 1.31 ms | 1.77 / 1.85 ms |
| 3,000 行 | 1.41–1.51 ms | 2.33 / 2.55 ms（真字体） |
| 6,000 行 | 2.02 ms | 2.60 / 3.14 ms |
| 12,000 行 | 3.35 ms | 3.61 / 4.18 ms |
| 12 个 ×120 行代码块（36 行文档） | 0.31 ms | 0.44 / 0.48 ms（400px/帧） |
| 600 个 ×20 行代码块 + 段落（1,200 行） | — | 0.9 / 1.2 ms（400px/帧）、2.0 / 7.1 ms（3000px/帧） |
| 源码模式 3,000 行（6 个 512 行块） | 0.72 ms | 1.72 / 4.50 ms（3000px/帧，真字体） |

### 3.2 这一档的真问题

1. **大块的冷排版**（真字体实测）：一个 512 行块挂上去 = **21ms**（13.59 正文 + 7.81 行号）。markdown 里的长代码块、表格、图片块同理；`LONG_BLOCK_SOURCE_LIMIT` 护栏只覆盖"未聚焦且超限"的块。**这是"一滚到长块就卡一下"的直接账。**
2. **源码模式行号每帧重排版**（`src/components/block/element/text_element.rs:BlockTextElement::prepaint`）：`(line_start..line_start + lines.len())` 逐行 `shape_line` + 每行 `to_string()`/`SharedString::from`。冷 15.3µs/次、热 0.2µs/次——静止帧靠帧级缓存侥幸便宜，**新挂载块的那一帧就是 7.8ms/块，白付**。
3. **每帧固定成本**（`src/editor/render/paint.rs:Render::render`）：`layout_theme.as_ref().clone()`（整份 Theme 深拷贝）+ `EditorSettings::fonts(cx)` + `apply_ui_typography` + `strings_arc()` + 整棵元素树重建（GPUI 每个 `Div` 克隆一次 `TextStyle`；dev 还多一层 `with_inspector_state`）。地板 0.33ms 主要花在这里。
4. **提前量轮换**（两档共用，见 §5）：固定 800px，算在"估计行高"坐标里；3,000px/帧时一帧挂 61–62 行。
5. **整篇扫描在这一档只值 0.5–0.7ms**（3,000 行：状态栏 0.2–0.3ms + 侧栏/大纲 0.3–0.4ms）。不优先，但见 §4。

### 3.3 这一档的待办

| 优先级 | 项 | 预期 | 备注 |
|---|---|---|---|
| P0 | 行号 `shape_line` 加跨帧 memo（键：行号文本 + 字号 + 字体；或整块一次 shape） | 冷挂载每块省 **7.8ms**；静止帧省 512 次哈希/分配 | 纯缓存；真字体下差 76 倍，收益确定 |
| P0 | 大块挂载分帧 + 上限（512 行块别一帧全排） | 削掉 21ms/块 的尖峰 | 与提前量改造同源 |
| P1 | 每帧固定项瘦身（Theme 借用、字体/排版只随指纹重算） | 压 0.33ms 地板 | 与块内已有 `derived_theme` 缓存同思路 |
| P2 | 两条整篇扫描移出渲染期（见 §4.3） | 本档 0.5–0.7ms | 主要收益在下一档 |

## 4 第二档：几万行

### 4.1 实测

| 场景 | 静态帧 | 滚动帧 |
|---|---|---|
| 20,000 行 markdown | 3.08 ms（真字体）/ 4.8 ms（Noop） | 300px/帧 4.52 / 5.51 ms；3000px/帧 **9.00 / 10.16 ms**（挂 61–62 行） |
| 40,000 行 markdown | 8.68 ms（Noop） | — |
| 1 MiB markdown 夹具（15,968 块，当前 HEAD） | 稳态 p95 4.8 ms（首绘 13.4 ms、编辑后绘制 4.3 ms） | — |
| 代码文件 1.5MB / 4 万行 / 79 块（512 行每块） | 2.68 ms | 400px/帧 2.4 / 3.8 ms（挂 1 行）；**3000px/帧 42.8 / 763 ms**（单帧新挂 51 块） |
| 代码文件跳转（同一文件） | — | 跳到估计高度 50% 2ms；95% 39ms；**越过末尾 1.5 倍 192ms**，同一次跳动里 `max_scroll_y` 65,633 → 142,265 → 488,706 |

滚动时每帧重算整条 stride 前缀（`stride_probe` 计数）：20,000 行文档 40 帧共 709k–770k 行 ⇒ **≈全篇/帧**。微基准：160k 行全量 refresh = 1.2ms（dev）/ 0.32ms（release）；16k 行 0.2–0.25ms（dev）。
外推校验：0.22 µs/行 × 160k 块 ≈ 35ms/帧，与台账 [performance.md](../architecture/performance.md) 里 35.8ms/帧 那一行一致。

### 4.2 归因（40,000 行，两轮重复，Noop）

| 配置 | 第 0 轮 | 第 1 轮 |
|---|---|---|
| A 侧栏开、无选区 | 8.88 ms | 8.25 ms |
| B 侧栏关、无选区 | 4.21 ms | 4.31 ms |
| C 侧栏开、选区在第 0 块（状态栏扫描短路） | 5.05 ms | 5.37 ms |
| D 侧栏关、选区在第 0 块（**核心渲染**） | **1.37 ms** | **1.35 ms** |
| E 侧栏开、选区在文档 90% 处 | 8.08 ms | 7.93 ms |

- 状态栏兜底扫描 ≈ **0.07–0.095 µs/块**；侧栏/大纲 ≈ **0.09–0.12 µs/块**；核心渲染 1.36ms。
- `sample` 采样（40,000 行、4000 帧）：`Editor::render` 占整帧 **69%**，其中 `render_workspace_panel → sync_workspace_models` **34.8%**（`sync_workspace_outline` 22.8%，内含 `root_blocks().to_vec()`：每个 `Entity::clone` 走 `LeakDetector::handle_created` 登记句柄 + memmove）、`render_status_bar` **31.8%**（`selected_visible_text` 28%，逐块 `read_untracked`）；真正的行/文本布局子树只占 **≈14%**。

### 4.3 这一档的主因（按权重）

1. **状态栏每帧全篇兜底扫描** — `src/editor/status_bar.rs:render_status_bar` 每帧调 `selected_visible_text`；无跨块/表格选区时落入 `src/editor/selection.rs:selected_visible_text` 的兜底循环，`selection.rs:single_block_selection_candidates` 从**第一块**逐块 `read_untracked` 直到命中非空选区——没有选区（阅读/滚动/光标态）就是全篇。P7 那次只去掉了 `accessed` 集合登记，扫描本身还在。默认开词数统计（`src/config/preferences.rs:137`）。
2. **大纲每帧克隆全部根块** — `src/editor/workspace/tree_sync.rs:sync_workspace_outline` 第 776 行 `let roots = self.document.root_blocks().to_vec();` 在**短路判断（779–782 行）之前**，每帧执行。侧栏默认展开（`src/editor/workspace.rs:449`）。
3. **每滚动帧整条 stride 前缀重算** — `src/editor/render.rs`：`RenderedRowPlan::set_stride` → `StridePrefix::invalidate_from` → `stride_prefix()` 的 `refresh` 从失效点重算到末尾；向下滚失效点在窗口底部、向上滚在顶部，两者都等于"到文档末尾"。
4. **提前量轮换 + 巨块整块挂载** — 512 行分块下，一块真实高度上万像素而估计值只有 `block_min_height`=28px（`src/theme/theme/presets.rs:223`），带状窗口在估计坐标里会把几十个分块一次圈进来：实测单帧新挂 51 块（Noop 结构成本 763ms 单帧；真字体按 21ms/块只会更高）。长块护栏只覆盖"未聚焦"的块。

### 4.4 这一档的待办

| 优先级 | 项 | 预期 | 备注 |
|---|---|---|---|
| P0 | 状态栏改从**活动块/焦点块**直取选区（表格与跨块分支已有），禁止从第一块起扫 | 4 万行省 2.8–3.8ms/帧；16 万块档 ≈10ms/帧 | 改动最小、收益最大 |
| P0 | `sync_workspace_outline`：`to_vec()` 移到短路之后；不克隆（`Arc<[Entity<Block>]>` 快照或借用 + `len()` 比较） | 4 万行省 3.7–4.7ms/帧；16 万块档 ≈7ms/帧 | 注意借用冲突 |
| P1 | stride 前缀改增量更新（Fenwick / 后缀 delta） | 滚动帧省 0.3–1.2ms（16 万行） | 与"稳定帧不扫全文"同一族判据 |
| P1 | 512 行块的挂载分帧 + 行号 memo（同 §3.3 两条） | 削掉 763ms 级尖峰；每块省 7.8ms | 可分两笔 |
| P2 | 提前量按已测量行高 + 速度预测；`COLD_RUN_MAX_ROWS`/`COLD_FILL_MAX_FRAMES` 按剩余未知行数自适应 | 快滚不再整段轮换 | 需覆盖闸门 |

## 5 两档共用的问题

1. **每个滚轮 tick = 一整帧**：`vendor/gpui/src/elements/div.rs:paint_scroll_listener` 在 offset 变化时 `cx.notify(current_view)`，所以滚动帧付的是完整 `Editor::render` 成本。
2. **提前量固定 800px 且在估计坐标里**：`src/editor/render.rs:31 RENDER_OVERDRAW_PX = 800`；窗口选取在 `src/editor/window_state.rs:rendered_window_with_row_floor`，带状区间是**前缀坐标**——估算行高与实际差多少，提前量就缩水多少倍。
3. **夹取时机**：`ScrollHandle::set_offset` 不夹取，夹取发生在 **prepaint**（`vendor/gpui/src/elements/div.rs:clamp_scroll_position`，并把结果写回句柄）。渲染窗口用上一帧的 `max_offset`，绘制位置用这一帧量出的内容高度——不一致时错位，下一帧纠正（源码注释称"估算爬行"）。
4. **列宽/主题变化清空全部行高测量**：`paint.rs` 的 `width_changed || layout_changed` 分支 `row_stride_cache.clear()` + `fill_strides(estimate)`，下一次 refresh 是整条前缀（40,000 行 = 80,000 项）。开合侧栏、缩放、换字号都会触发一次；切标签恢复阅读位置的老毛病也出自这里（`src/editor/mod.rs:533-547`）。
5. **冷启动续帧**：`needs_fill` → `schedule_followup_frame`（16ms 定时器），上限 `COLD_FILL_MAX_FRAMES = 8`（≈130ms）；单帧预挂载上限 `COLD_RUN_MAX_ROWS = 12`。

## 6 空白帧

尝试过的形态：短行 + 空行块（6,001 行）、20 行代码块文档（1,200 行）、markdown 夹具（15,968 块）、源码模式（3,000 行 / 4 万行）、跳转到估计高度 50%/95%、越过文档末尾 1.5 倍，步长最大 3,500px/帧（真字体与等宽模拟各跑一遍）。**没有一例在视口内部出现空白带**；覆盖检查（挂载真实行的并集 vs 视口区间）始终成立。

唯一测到的"下缺 517px"是文档末尾半屏尾距（`paint.rs` 的 `pb(... + scroll_beyond_bottom)`，`scroll_beyond_bottom = viewport × 0.5`），设计内。

因此空白帧的确定性解释只到这一步：它出现在**一帧要挂载的东西太多**的窗口里——冷启动续帧（≤8 帧 ≈130ms），或**长块冷排版**的那一帧（真字体 21ms/个 512 行块；一帧挂 N 个就是 N×21ms）。真机复现与验证需要窗口内真实文本系统 + 逐帧截图/计时。

## 7 复现方法

本报告的探针是临时文件（已删除，未入库；`vendor` 与 `src/editor/tests.rs` 都已还原到 HEAD）。复现步骤：

1. 在 `src/editor/tests/` 下新建探针文件，`src/editor/tests.rs` 加一行 `mod <名字>;`。
2. 用例体：`init_editor_test_app(cx)` → `Editor::from_markdown(...)` / `from_loaded_document(...)` → 排空续建（`pending_tail`/`pending_source`）→ 若干 `redraw(cx)` → 循环改 `editor.scroll_handle.set_offset(...)` 并计时 `redraw(cx)`。
3. 计数器：`editor.row_plan_rebuilds`、`editor.row_stride_cache_clears`、`editor.document.snapshot_rebuilds`、`editor.word_count_scans`、`editor.source_serializations`、`crate::editor::render::stride_probe::rows()`（`#[cfg(test)]`）。
4. 跑法：`cargo test --bin velora <用例名> -- --ignored --nocapture`；打印被工具链包装吃掉时直接跑 `target/debug/deps/velora-*` 加用例名。
5. 归因：关侧栏 = `editor.workspace.is_open = false`；短路状态栏扫描 = 把选区放到第 0 块（`selected_range = 0..3`）。
6. **真字体**：临时把 `vendor/gpui/src/platform/test/platform.rs` 里的 `let text_system = Arc::new(NoopTextSystem);` 换成 macOS 分支的 `Arc::new(crate::MacTextSystem::new())`，跑完 `git checkout --` 还原；用 `width("iiii") vs width("WWWW")` 做判据（等宽模拟下两者相等）。
7. **量排版缓存**要注意它是**帧级**的（`vendor/gpui/src/text_system/line_layout.rs:438`）：同一文本重复量到的是热态，换一条新文本才是冷态。

建议固化成常驻闸门（现在漏的两条）：
- 渲染期"逐块读取次数"计数器（现有 `count_row_plan_block_read` 只覆盖行计划重建）；
- 滚动帧的 `stride_probe::rows()` 断言（现有 `core_repair_stable_frames_do_not_rescan_all_row_heights` 只断言静止帧）；
- 长块冷排版预算（挂载一个 N 行块的墙钟上限）。

## 8 边界与未做

- 绝对值是 **dev 构建 + 测试平台**；release 约快 3–5 倍。真字体部分是真实 CoreText 排版；**字形光栅与 GPU 提交仍未测**（测试平台不提交合成），但 gpui 的光栅走 atlas 缓存，稳定帧不重复光栅。
- 未做真机 GUI 采样（按要求停止）；真机上的滚动体感还需一次窗口内的逐帧计时/截图验证。
- 未覆盖：导出、搜索、表格列宽测量（`ColumnLayoutMemo` 已有跨帧复用）、图片/公式/Mermaid 的异步加载路径。
