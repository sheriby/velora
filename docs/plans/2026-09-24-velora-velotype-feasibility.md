# Velora：Velotype 编辑器可行性验证实施计划

> 状态：已暂停。用户要求当前阶段功能优先，开发时只用 debug 构建；以下性能测量和 release 构建任务待核心编辑功能可用后再执行。

> **执行要求：** 按 Superpowers 的 `executing-plans` 流程逐项实施、验证与评审。本计划只覆盖编辑器抽取前的可行性关口。

**目标：** 用可复现的实测结果判断 Velotype 的原生编辑器是否适合作为 velora 选择性抽取的基础。

**架构：** Velora 的代码直接在当前工作区开发。Velotype 固定版本只在独立临时副本中构建和测量，不改动原始 `../velotype`。先验证原版性能与中文输入，再制定编辑部件抽取计划。

**技术栈：** Rust 2024、GPUI 0.2、Cargo；Node.js 只用于生成测试样本和汇总测量结果。

---

## 范围与当前进度

- 设计依据：[Velora 原生编辑器设计](2026-09-24-velora-gpui-editor-design.md)。用户选择新建 Velora 的 GPUI 应用结构，一次性抽取 Velotype 的编辑代码，后续按需手工移植上游修复。
- 固定的 Velotype 来源提交为 `ed65977be94f2f2703037fcb8b6cbab2e7579571`。原始仓库在 `../velotype`，独立基线副本在 `/private/tmp/velora-velotype-baseline`。
- 当前工作区已包含最小 GPUI 窗口和固定样本生成器。`cargo check` 及 `node --test scripts/generate-fixtures.test.mjs` 已通过；应用窗口尚未完成实际运行验收。
- 原版 Velotype 的 `cargo build --release --locked` 已通过，产物为基线副本中的 `target/release/velotype`，耗时 213.45 秒。`cargo test --locked` 在输出 522 项通过记录后以 SIGSEGV 退出，没有最终测试汇总，**不能记为测试通过**。
- 本阶段不把 Velotype 全仓复制进 velora，不开发工作区功能，不恢复已删除的 Tauri 代码。性能或中文输入若未达到目标，先处理问题或重新评估方案。

### 任务 1：保存现有基线构建证据

**文件：** 新建 `docs/benchmarks/2026-09-24-velotype-baseline.md`。

1. 从 `/private/tmp/velotype-cargo-test.log` 和 `/private/tmp/velotype-cargo-build-release.log` 读取原始输出；记录命令、退出码、耗时、最后可见的测试名称和构建产物路径。记录当前 Mac mini M4、16 GB 内存、macOS 与 Rust 版本。
2. 用 `git -C /private/tmp/velora-velotype-baseline rev-parse HEAD` 核对固定提交；用 `git -C ../velotype status --short` 核对原始仓库未被修改。
3. 报告必须明确区分“release 构建成功”和“测试进程崩溃”。不得因为已有 522 项通过记录就写“测试通过”。
4. 运行 `git diff --check`，提交报告：`docs(bench): record Velotype build baseline`。

### 任务 2：定位基线测试崩溃

**文件：** 更新 `docs/benchmarks/2026-09-24-velotype-baseline.md`；仅在独立基线副本中运行诊断命令。

1. 先从现有日志确认 SIGSEGV 发生在单个测试、测试间切换，还是测试进程退出阶段。若日志无法判断，记录“未知”，不要猜测。
2. 先列出相关测试，再用过滤参数缩小范围。例如运行 `cargo test --locked editor::selection::tests:: -- --test-threads=1`；只有该范围仍崩溃，才继续缩小到单个测试。不要先重复整套长时间测试。
3. 若单线程或单个测试仍崩溃，保留失败命令、退出码与日志尾部；必要时用 macOS 崩溃报告定位原生栈。不得修改 Velotype 源码或跳过测试以制造通过结果。
4. 记录“已定位原因 / 仅定位到范围 / 尚未定位”三者之一，并提交诊断报告更新。该问题是否阻止抽取，由任务 7 根据证据决定。

### 任务 3：检查原版应用能否打开真实样本

**文件：** 更新 `docs/benchmarks/2026-09-24-velotype-baseline.md`。

1. 在当前 velora 工作区运行 `node scripts/generate-fixtures.mjs /private/tmp/velora-velotype-fixtures`。确认生成 1 MiB 与 10 MiB 两个 UTF-8 Markdown 文件；不要把大样本提交到 Git。
2. 分别用独立基线副本的 release 可执行文件打开两个样本，关闭应用后再开另一个。检查是否进入富文本编辑、能否输入、是否回退源码模式、有无崩溃或明显渲染错乱。不要保存对样本的修改。
3. 把每次操作和可观察结果写入报告；仅有“打开成功”不能推断满足 1 秒或 5 秒性能阈值。运行 `git diff --check` 后提交。

### 任务 4：实现测量结果汇总脚本

**文件：** 新建 `scripts/summarize-bench.mjs`、`scripts/summarize-bench.test.mjs`。

1. 先写公开行为测试：向脚本标准输入传入五条 `{"kind":"input_ms","value":1}` 至 `5` 的 JSON 行，断言输出的中位数为 `3`、最近秩定义的 p95 为 `5`、样本数为 `5`。缺失数值、`NaN` 或无穷大必须报错。
2. 运行 `node --test scripts/summarize-bench.test.mjs`，确认先因脚本不存在而失败。
3. 最小实现读取标准输入中的 JSON 行，按 `kind` 分组。对每组数值排序：奇数个取中间值，偶数个取中间两值平均；p95 使用索引 `Math.ceil(0.95 * n) - 1`。输出含原始样本、数量、中位数与 p95 的 JSON。
4. 重跑聚焦测试，确认通过；提交脚本与测试：`test(perf): summarize desktop timings`。

### 任务 5：给基线副本加入整机测时探针

**文件：** 在独立基线副本临时修改 `src/main.rs`、`src/editor/render.rs`、`src/components/block/input.rs`；在 velora 中新增 `docs/benchmarks/velotype-probe.patch` 并更新基线报告。

1. 在进程启动、开始读取目标文件、编辑区首次获得焦点并可输入、普通输入提交及其下一次画面更新处记录单调时钟。向标准错误输出 `launch_ms`、`open_ms`、`input_ms` 与数值，供任务 4 的脚本汇总。
2. 用一个极小文件做控制实验：时间值必须有限且非负；首次“就绪”记录时实际能输入；一次普通按键只产生一次输入延迟记录。输入法尚未确认的组合文字不计入普通输入样本。
3. 若首次渲染早于真正可输入状态，移动就绪标记到首次获得焦点的画面；在报告中写清测量定义。不得把“第一帧开始构造”冒充“用户可输入”。
4. 在独立基线副本运行 `cargo build --release --locked`；把该副本的 `git diff` 输出保存为当前 velora 仓库中的 `docs/benchmarks/velotype-probe.patch`。不要把探针直接合入原始 `../velotype` 或 velora 正式应用。提交探针补丁与方法说明。

### 任务 6：测量性能和 macOS 中文拼音输入

**文件：** 更新 `docs/benchmarks/2026-09-24-velotype-baseline.md`；新建 `docs/benchmarks/2026-09-24-velotype-ime.md`。

1. 在 release 构建中分别对空白启动、打开 1 MiB、打开 10 MiB 执行 10 次独立进程运行，保留每次原始值。输入延迟在 1 MiB 文档的开头、中部、末尾合计记录至少 200 次，使用任务 4 的脚本计算 p95。
2. 真正使用 macOS 中文拼音在富文本与源码模式输入：选择候选词、修改未确认的组合文字、在行内格式和块边界附近编辑、撤销、保存并重开。记录输入样本、预期文本、实际文本，以及候选框错位、丢字或重复字。自动化 UTF-16 测试不能替代此项。
3. 对照已确认目标逐项写“通过 / 未通过 / 无法测量”：当前 Mac mini M4/16 GB 冷启动 ≤1 秒、打开 1 MiB ≤1 秒、打开 10 MiB ≤5 秒、输入 p95 ≤50 毫秒。阈值不得悄悄改动。提交原始数据与结论。

### 任务 7：记录抽取关口的决定

**文件：** 新建 `docs/plans/2026-09-24-velora-editor-extraction-decision.md`。

1. 根据构建、崩溃诊断、性能和输入法结果，只作以下一种结论：① 可进入编辑部件抽取计划；② 先针对已定位的问题制定修复计划；③ 风险过高或证据不足，返回架构讨论。
2. 决策记录必须指出下一阶段拟复用的 Velotype 文件与需留在应用层的文件；解释原始代码的许可与归属处理。不得把未实测性能写成已达标。
3. 重读报告与原始数据，运行 `git diff --check`，提交决策。若进入抽取阶段，再写新的中文实施计划，覆盖 GPUI 编辑部件、工作区、多标签、安全保存和打包。
