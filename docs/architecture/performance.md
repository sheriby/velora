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

## 3. 基线（2026-09-28，dev 构建，含测试窗口开销）

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
| P2 | 纯文本分块导入：代码/纯文本按行分块多块导入，首块同步 + 其余后台续建；序列化无损、CRLF 保持、行号连续 | ⬜ | |
| P3 | shape memo：BlockTextElement 布局缓存（文本/宽度/字体不变则跳过 shape_text） | ⬜ | |
| P4 | 每帧文档级瘦身：折叠过滤/结构检测按 revision 增量；表格列宽缓存 | ⬜ | |
| P5 | undo 大文件预算：按字节上限收缩历史，避免重复全文 clone | ⬜ | |
| P6 | 大 markdown 渲染裁剪：代码块高亮滚入窗口才执行等 | ⬜ | |
| P7 | 10x 验收：全夹具复测 + 预算断言入库 | ⬜ | |

## 6. 记录

### 2026-09-28 P1 代码路径探针与基线
新增 `manual_code_load_probe`（src/editor/tests.rs，`#[ignore]`，同一 `VELORA_PERF_FILE` 入口）：走 `Editor::from_file_source` 真实代码文件路径，断言进入 Source 模式，输出与 markdown 探针同格式。基线（dev，单 CodeBlock 现状）：

| 夹具 | construct_ms | first_draw | steady_p95 |
|---|---:|---:|---:|
| sample.lock（205KB/8.5k 行） | 98 | 28.5 | **28.4** |
| log-1mib | 196 | 38.3 | **39.5** |
| log-10mib | 2,011 | 457 | **476** |

- 代码路径（source-raw，跳过行内样式）比探针里 markdown 路径快一个量级，但仍远不达标：lock ≈35fps（用户感知的"有点卡"），10MiB 打开 ~2.5s、1.5fps。
- 教训：探针夹具文件名必须带代码扩展名（`.lock`），否则 `is_code_file` 分流不生效（`cargo-lock-real` 无扩展名 → 误走 markdown 路径，断言立刻抓住）。
