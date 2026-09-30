# Performance Ledger（性能基线与优化台账）

> 大文件性能优化的活文档：基线数据、已定位根因、已做/计划中的优化、验证方法。
> 写作于 2026-09-28，`perf` 分支。相关：[render-pipeline.md](./render-pipeline.md)、[editor-core.md](./editor-core.md)

## 1. 目标（用户设定）

- 长普通文本（cargo.lock ~8k 行、1MiB/10MiB log）：**100ms 内打开 + 流畅滚动**。
- 大 markdown：懒加载/裁剪渲染，保证可用流畅度。
- 整体大文件流畅度提升 **≥10x**。dev 构建验证（热依赖 crate 在 dev 下已 opt-level=3，数字有代表性；本地代码 O0）。

## 2. 复现与测量方法

```bash
# 夹具（markdown 两个，gitignored）
node scripts/generate-fixtures.mjs tests/fixtures/perf

# 加载探针（构造/首绘/稳态 p95/编辑/序列化）
VELORA_PERF_FILE=<file> cargo test manual_markdown_load_probe -- --ignored --nocapture
VELORA_PERF_FILE=<file> cargo test manual_code_load_probe   -- --ignored --nocapture   # 代码文件路径（P1 新增）
```

| 夹具 | 说明 |
|---|---|
| `tests/fixtures/perf/one-mib.md` / `ten-mib.md` | 重复中文段落单元，15,968 / 159,683 块 |
| `/tmp/velora-perf-fixtures/log-1mib.log` | 9,891 行无空行日志（自建，勿入库） |
| `/tmp/velora-perf-fixtures/log-10mib.log` | 98,909 行无空行日志 |
| `/tmp/velora-perf-fixtures/cargo-lock-real` | 仓库 Cargo.lock 拷贝，8,536 行 / 205KB |

## 3. 基线与终态（2026-09-28，dev 构建，含测试窗口开销）

markdown 路径（`Editor::from_markdown`，`manual_markdown_load_probe`）：

| 夹具 | 块数 | construct_ms | first_draw | steady_p95 | serialize_ms |
|---|---:|---:|---:|---:|---:|
| log-1mib（全文 1 段落块） | 1 | 1,697 | 655 | **647** | 6.9 |
| log-10mib（全文 1 块） | 1 | 17,465 | 6,338 | **8,701** | 86 |
| cargo-lock-real（空行分块） | 829 | 236 | 4.2 | **4.0** | 26 |
| one-mib.md | 15,968 | 2,986 | 31 | 35 | 231 |
| ten-mib.md | 159,683 | 109,759（测试内跑完全部续建） | 282 | **293** | 2,079 |

读法：
- Cargo.lock 证明**多块 + 视口窗口化是对的**：829 块稳态 4ms/帧。
- log 的 647ms~8.7s/帧证明**整文件单块是毒药**：窗口化救不了单块（块即窗口）。
- ten-mib.md 稳态 293ms/帧 ≈ 每帧文档级固定开销 × 160k 块（见 §4-3）。
- 探针 construct_ms 含后台续建在测试里同步跑完的部分；真实应用首屏时间以首绘为准。

### P7 边缘指标收敛（2026-09-28 第二轮复测，read_untracked + 哈希后台化后）

代码路径（`manual_code_load_probe`）：

| 夹具 | 同步构造 | 首绘 | 稳态帧 | 编辑更新 |
|---|---:|---:|---:|---:|
| sample.lock | 4.5 | 1.1 | 1.2 | 22.7 |
| log-1mib | **12.1** | 1.3 | 1.3 | 31.1 |
| log-10mib | **99.7（≤100 达标）** | 2.2 | 2.3 | 54.7 |

markdown 路径（`manual_markdown_load_probe`）：

| 夹具 | 首绘 | 稳态帧 |
|---|---:|---:|
| one-mib.md（15,968 块） | 1.4 | **1.3（基线 35 → 26.9x）** |
| ten-mib.md（159,683 块） | 10.8 | **10.8（基线 293 → 27.1x，≥10x 达标）** |

收敛手段：`file_content_version` 后台化 + 规范化直通哈希；vendored gpui
新增 `Entity::read_untracked`（跳过 accessed 集合登记）——状态栏选区
扫描每帧 16 万次集合插入曾是 160k 块文档稳态帧的最大单项。

### P7 终态（2026-09-28，优化后同一探针复测）

代码路径（`manual_code_load_probe`，真实 `from_file_source`）：

| 夹具 | 构造(同步) | 首绘 | 稳态帧 | 编辑更新 | 编辑后绘制 |
|---|---:|---:|---:|---:|---:|
| sample.lock（205KB） | 22.6 | 1.1 | 1.1 | 21.9 | 2.1 |
| log-1mib | 51.0 | 1.4 | 1.4 | 32.4 | 2.7 |
| log-10mib | 103.3 | 2.3 | 2.2 | 55.6 | 4.0 |

markdown 路径（`manual_markdown_load_probe`）：

| 夹具 | 首绘 | 稳态帧 | 编辑后绘制 |
|---|---:|---:|---:|
| one-mib.md（15,968 块） | 3.4 | 3.3 | 15.6 |
| ten-mib.md（159,683 块） | 35.7 | 35.8 | 199.4 |

对照基线（§3）的提升倍数（dev 构建）：

- **纯文本/代码**：10MiB log 稳态 476→2.2ms（**216x**）、打开同步段 2011→103ms（**20x**）、
  编辑 765→56ms（**14x**）；1MiB log 稳态 39.5→1.4ms（28x）、打开 177→51ms；
  lock 稳态 28.4→1.1ms（26x）、打开 98→23ms。1MiB 内普通文本打开已进 100ms
  （dev O0），10MiB 同步构造 103ms、release 预期 ~30-40ms。
- **markdown**：10MiB/160k 块稳态 293→35.8ms（**8.2x**）、首绘 282→35.7ms；
  1MiB 稳态 35→3.3ms（**10.6x**）。
- 说明：探针 construct_ms 在测试平台内含流式续建排水与自动重绘，不代表真实
  打开时间；真实打开 = 文件读 + 同步构造（首块）+ 首帧，其余后台续建。

## 4. 已定位根因（代码证据）

1. **纯文本/代码文件整文件单块**：`replace_document_content`（src/editor/file_drop.rs）对 `is_code_file` 的文件建单个 `BlockKind::CodeBlock`。渲染、shape、undo 快照、序列化全压在一块。
2. **shape_text 无跨帧 memo**：`BlockTextElement::request_layout`（src/components/block/element.rs）每帧对每个挂载块重新 `build_text_runs` + `shape_text`。滚动时静态块白白重 shape。
3. **每帧文档级固定开销**：`visible_blocks().to_vec()` 全量克隆 + 折叠过滤每帧扫描全部块 + 结构变化检测逐 id 比较；主题 clone + 全字号缩放；表格列宽每帧重测所有单元格（render-pipeline.md §6）。
4. **undo 全文快照**：每条 undo 条目持有整篇源码（10MB × 200 条上限），大文件下 finalize 一次 clone 全文（src/editor/history.rs）。
5. 10MiB markdown 总构建成本 ≈ 160k 个 GPUI 实体创建（G8 渐进导入已拉开首屏，总量固定）。

## 5. 优化台账（每项一个功能点提交；完成一项更新一行）

| # | 优化 | 状态 | 结果 |
|---|---|---|---|
| P1 | 代码路径探针（`from_file_source` 真实路径的 ignored 探针，补齐基线） | ✅ 2026-09-28 | 见 §6 |
| P1.1 | 代码路径基线（`manual_code_load_probe` 实测） | ✅ 2026-09-28 | lock 稳态 28ms/帧；10MiB 稳态 476ms/帧、打开 ~2.5s |
| P2a | 纯文本按 512 行分块导入 + 行号续号 + 无损守卫 | ✅ 2026-09-28 c84ef19 | 10MiB log 稳态 442→8.8ms/帧（50x），编辑 765→167ms |
| P2b | 分块边界编辑（块尾回车/块首退格/块尾前删）+ 行号刷新 | ✅ 2026-09-28 639eabd | 边界编辑语义与单块一致，round-trip 无损 |
| P2c | 渐进导入（首块同步 + PendingTail 后台续建，每步 4 块） | ✅ 2026-09-28 c697df8 | 首屏只建 512 行；续建每步 ~2.6ms |
| P4a | 状态栏字数/行数按修订缓存；导入零拷贝规范化 | ✅ 2026-09-28 c654bb7 | 移除每帧整篇重序列化（原 10-80ms/帧） |
| P4b | 行结构计划缓存（折叠过滤/分组扫描按修订+折叠+TOC 版本缓存）；元素构建推迟到挂载 | ✅ 2026-09-28 bc6cf13 | 10MiB markdown（160k 块）稳态 293→36.5ms/帧（8x） |
| P5 | undo/编辑路径瘦身：raw_source_text 单遍追加；代码文档跳过全文档图片注册表重建 | ✅ 2026-09-28 73db8ca | 10MiB 序列化 86→1.0ms；编辑触发器（含 \`[\`/\`<\` 的日志）不再每键 O(文档) |
| P3 | shape memo：BlockTextElement 布局键缓存（文本代数/宽度/字号/字体指纹/主题），min-content 测量短路，冷启动挂载上限 | ✅ 2026-09-28 | 编辑后首帧从 ~250ms（病态 1px 重 shape × taffy 多次 measure）降至 ~22ms |
| P7 | 10x 验收：全夹具复测 + 台账更新 | ⬜ | |

## 6. 记录

### 已知剩余（后续候选）
- 大 markdown 编辑路径：undo finalize 的 `markdown_text` 全文重序列化
  （160k 块约 2.1s，基线即如此，非本次回归）。候选：按块脏标记的
  增量序列化缓存。
- 表格列宽仍每帧重测所有单元格（见 render-pipeline.md §6）。
- 历史条目无字节预算：200 条 × 全文快照，超大文档内存上限偏高。
- 流式续建循环无 yield，真实应用打开 10MiB 文档首秒内会有一次
  ~100-200ms 的主线程占用（G8 markdown 同样存在）。

### 2026-09-28 P1 代码路径探针与基线
新增 `manual_code_load_probe`（src/editor/tests/loading_chunks.rs，`#[ignore]`，同一 `VELORA_PERF_FILE` 入口）：走 `Editor::from_file_source` 真实代码文件路径，断言进入 Source 模式，输出与 markdown 探针同格式。基线（dev，单 CodeBlock 现状）：

| 夹具 | construct_ms | first_draw | steady_p95 |
|---|---:|---:|---:|
| sample.lock（205KB/8.5k 行） | 98 | 28.5 | **28.4** |
| log-1mib | 196 | 38.3 | **39.5** |
| log-10mib | 2,011 | 457 | **476** |

- 代码路径（source-raw，跳过行内样式）比探针里 markdown 路径快一个量级，但仍远不达标：lock ≈35fps（用户感知的"有点卡"），10MiB 打开 ~2.5s、1.5fps。
- 教训：探针夹具文件名必须带代码扩展名（`.lock`），否则 `is_code_file` 分流不生效（`cargo-lock-real` 无扩展名 → 误走 markdown 路径，断言立刻抓住）。
