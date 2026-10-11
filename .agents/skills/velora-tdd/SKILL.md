---
name: velora-tdd
description: Develop Velora (GPUI desktop app) test-first — for any defect (user report, CI failure, review finding, self-found) and any new feature: reproduce or define the expectation, write a failing test at the lowest level that shows it, confirm the failure lands on an assertion, implement minimally, run related regressions, then run the full gates. Triggers include "修 bug", "复现", "报修", "加功能", "新功能", "测试先红后绿", "TDD".
---

# Velora / TDD

一个循环，适用于任何缺陷修复、新功能与行为变更：

**复现或定义期望 → 写测试（确认 Red）→ 最小实现（确认 Green）→ 定向回归 → 全量验收 → 保留回归测试。**

核心原则：未看到测试失败，无法证明测试能钉住目标行为——Red 未确认不得改动实现，Green 未确认不算完成；回归测试保留在仓库。

## 1 复现或定义期望，写测试

- 问题来源不限于用户报修：CI 或测试失败、代码评审发现、开发中自遇、新功能与行为变更，都走本流程；来源只决定第一步的做法：
  - 缺陷：先复现。使用真实素材（文档片段、键鼠序列、尺寸、CI 日志）；来源提供的例证（报修描述、CI 报错、评审给出的复现路径）即验收口径。无法复现时先排查输入条件、夹具与生命周期，不要凭猜测修改实现。
  - 新功能 / 行为变更：没有故障可复现，「应有行为」即验收口径；先把需求写成测试（测试即规格），确认 Red 后再实现。
- 每条问题描述或需求至少对应一条断言；一个测试围绕一个用户可观察行为（测试名描述该行为），可包含多条相关断言，但不夹带无关行为：

| 现象 / 需求 | 断言 |
|---|---|
| 「删完文件里留下半截记号」 | 结果字节 == 期望字节 |
| 「最后两个字没高亮」 | 高亮区间 == 屏幕文本全长 |
| 「点了没反应」 | 动作之后的状态变了 |
| 「拖不动」 | 选区端点 == 落点换算出来的偏移 |
| 「面板位置不对」 | 渲染几何（`debug_bounds`）== 期望落点 |
| 「画面不刷新」 | 第二帧的渲染输入变了 |

- **测试层级自低而高，优先使用能覆盖目标行为的最小有效层级**：
  - 纯文本转换、偏移换算、区间计算 → 模块内单测；
  - 组件状态与块级输出 → 块级测试（`src/components/block/runtime/tests/`）；
  - 鼠标命中、事件传播、焦点、跨块选区、真实几何 → 窗口级（见骨架）；
  - 跨帧、异步、定时器、去抖 → 能驱动该生命周期的测试（`#[gpui::test]`）。
  低层无法复现或覆盖真实行为时再升级到窗口级；不为统一形式强行使用，也不为简化省略关键条件。
- **边界按根因或功能输入维度选取，不机械铺满**：块首/块中/块尾、空块/空文本、隐藏记号（`**`、`` ` ``、`[](…)`）、多字节（CJK / emoji）、表格跨格/跨块。改动落在共享换算、通用命中、共享状态机或缓存键上时，扩大相应维度的覆盖。
- **断言用户可见的产物**：字节、屏幕文本、选区范围、渲染几何与输入；不断言内部临时值，此类断言会随重构失效。
- **注释写清现象与判据**（缺陷写现象与根因，新功能写行为契约），不写叙事口吻。该测试即为回归守卫。

骨架（窗口级）：

```rust
let mut app_cx = TestAppContext::single();
init_editor_test_app(&mut app_cx);
let (editor, cx) = app_cx.add_window_view(|_window, cx| {
    Editor::from_markdown(cx, MARKDOWN.to_string(), None)
});
redraw(cx);
redraw(cx);                       // 几何要两帧才稳定

editor.update(cx, |editor, cx| { /* 设状态、断言 */ });
drop(editor);
app_cx.quit();                    // 不要用 VisualTestContext 收尾，会 SIGSEGV
```

运行单条：`cargo test --bin velora <测试名> -- --nocapture`（没有 lib target）。

## 2 确认 Red

Red 必须落在**断言**上，左右值要对得上用户可见现象（屏幕上少两个字 ⇔ 区间少 4 个字节）；对不上说明测试没有钉住目标行为：

- 编译失败不是 Red——新接口不存在时，先补一个不改行为的空壳使测试可运行，再确认断言失败。
- 未实现前测试直接通过：不构成 Red——说明测试没有钉住目标行为（缺陷未复现，或行为已存在）；先修测试。
- 夹具 panic、环境或超时不是 Red，先修夹具。
- 断言因其他原因失败不算数：先查清失败的真实原因。
- 对称两半（两个入口、两条路径）各自确认 Red；仅验一半时，另一半的断言可能并未生效。

手段：

- 用 `println!` 输出左右值，配 `-- --nocapture` 查看。GPUI 中 `entity.update` 内的 panic 有时不出现在 stdout，打印比断言可靠；输出未出现在终端时，直接运行 `target/debug/deps/velora-*` 里的测试二进制。
- 「拖不动 / 选不上 / 点不中」定位配方：在窗口级测试中先输出现状——每个可见块的 `kind()` / `display_text()` / `last_bounds` / `clean_visible_len()`、格子的 `last_bounds`，再看 `cross_block_selection`、各块 `selected_range` / `editor_selection_range`、`active_entity_id`。哪一块缺几何、缺选区，一目了然，快于读代码猜测。

仅在 Red 来源存疑时做隔离验证（非固定步骤）——测试与实现同时改动、Red 非直接观察所得、或新语义只能在「旧行为中无此路径」上推断：

- `git stash push -- <实现文件>` 后运行一次，再 `git stash pop`；stash 后编译失败（新字段 / 新接口）时，把该处判定临时改回旧行为，确认断言是否失败，验证后立即还原。
- 仅对目标文件执行 stash/pop，不触及工作区其他未提交改动；恢复后核对 `git status` 与隔离前一致。

## 3 实现与验收

### 3.1 最小实现

- 一次只改一处口径；改在判据所属的那一层（换算函数 / 状态机 / 组件），不在每个调用点打补丁。
- 实现范围以验收口径为界：不加未被要求的行为与抽象，不夹带顺带重构；临时实现有明显重复可局部整理并重跑相关组；大范围重构另开一笔，不进本次提交。
- 运行目标测试确认 Green。若 Red 为直接观察所得、且测试此后未再改动，直接进入定向回归；否则按 §2 补充验证 Red。
- 测试仍失败时先查实现，不先放宽断言；确属测试写错，回 §2 重新确认 Red。

### 3.2 定向回归

- Green 后先运行**相关测试组**（`cargo test --bin velora <关键词>`）。
- 同族扫描：共享口径变更后，grep 全部读者与写者（源改而消费者未同步 = 缺陷）。
- 涉及缓存的变更确认 key / generation 覆盖全部输入（文本代数、字号、字体指纹、主题代数、换行宽……）。
- 平台判据（路径、字节表、大小写）：本地通过不代表 CI 通过，按平台逐条核查。
- 遗留副作用（多一个空行、多一次重算）写入总结，不静默忽略。

### 3.3 全量验收

- 任务范围界定的是交付物，不是验证范围：收尾一律运行全量门禁，不因任务只点名某处而缩小。
- `cargo build`（零警告）与 `cargo test --bin velora` 各运行一次，仅在收尾执行——全量测试默认 5 秒上下、构建分钟级，逐版反复运行无收益。
- 记录通过数与失败数；验收后若再改代码，按影响范围重跑（相关组，必要时全量）。

## 4 GPUI 的坑（写测试与确认 Red 时按需查用，非固定步骤）

1. **更新是延后的**：`cx.notify()` 只排一帧。要断言渲染结果，先 `window.draw(cx).clear()`（仓库中是 `redraw`）+ `run_until_parked()`，否则断言的是上一帧。
2. **几何来自上一帧**：`last_bounds` / `last_layout` 在 `prepaint` 中写入。未 draw 过则没有几何，命中测试一律返回 0——「拖不动」类问题常见原因是夹具未先 draw。
3. **借还锁与重入**：`entity.read(cx)` 的 guard 必须在 `entity.update(cx, …)` 之前落地；`update` 闭包内不得再更新同一实体（本仓库历史上因此 coredump）。
4. **测试上下文**：`TestAppContext::single()` + `add_window_view` 返回 `VisualTestContext`，不是 app 上下文；收尾用 `app_cx.quit()`。键盘 / 焦点测试先 `window.activate_window()`（测试窗口默认不是 key）。仅异步 / 定时器（光标闪烁、自动保存、去抖）场景使用 `#[gpui::test]`，不要用它替代窗口测试。
5. **事件是两段式**：capture → bubble。点击无响应时先查：谁调用了 `stop_propagation`、处理处在哪个阶段、焦点与 hitbox 判定是否通过。
6. **文本系统**：真实字体才能量像素宽；无窗口测试用 `NoopTextSystem`，只量结构与偏移。
7. **渲染输入 ≠ 屏幕**：prepaint 的 run/quad 是断言渲染的最外层抓手；颜色、行高、`bounds` 都在这里看。
8. **后台与定时器**：测试中后台执行器运行在调用线程；`run_until_parked` 只泵后台任务；计时用 `cx.background_executor().timer(..)`，`smol::Timer` 不被调度器跟踪。

## 5 本仓库抓手

- 测试：`src/editor/tests/common.rs`（`init_editor_test_app`、`redraw`、`perf_passes`、`perf_delta`）、`src/editor/selection/tests.rs`（`set_selection`、`assign_visible_block_bounds`）、`src/editor/tests/selection_mouse.rs`（真实按下—拖动—抬手的选区用例集中于此）、`src/components/block/runtime/tests/`（块级）。
- 几何断言钩子：元素挂 `.debug_selector(|| "名".into())`，窗口级测试中用 `cx.debug_bounds("名")` 取 `Option<Bounds<Pixels>>`（`Pixels` 转 f32 用 `.into()`）。
- 交互模拟：`cx.simulate_mouse_down/move/up(point(px(x), px(y)), MouseButton::Left, Modifiers::none())`、`cx.dispatch_action(Action)`、`cx.simulate_input("x")`（敲字）。
- 选区代码：`src/editor/selection.rs`（跨块）、`src/editor/selection/table.rs`（表格跨格）、`src/editor/selection/pointer.rs`（指针与「点 → 端点」换算）；两套坐标与表格两层的坑见 §6。
- 环境：当前 Rust stable 工具链；`cargo test --bin velora`（无 lib target）。

## 6 表格与选区（本仓库专属的坑）

- 表格是**两层**：表格块（`BlockKind::Table`）+ 格子。**格子不在块树里**——`visible_blocks()`、`block_entity_by_id()` 都找不到它们，只能从表格块的 `table_runtime`（`header` / `rows`）或编辑器里的 `table_cells` 绑定表获取；格子的几何是格子绘制完一帧后的 `last_bounds`。
- 表格块**自己没有文本元素**：`last_bounds` 恒为空、`clean_visible_len()` 是 0、`index_for_mouse_position()` 恒给 0。任何「按可见块扫一遍」的代码（选区端点换算、命中测试、滚动锚点、大纲、搜索高亮）对表格内容一概失效——「表格里拖不动 / 选不上 / 点不中」类现象先查这一条。
- 屏幕上的点归哪一根块只有一处换算：`cross_block_endpoint_for_point`（`src/editor/selection/pointer.rs`）。它看不见量不出文本布局的块（表格、分隔线、未聚焦的公式与图表）时，会把该段空间归为「上一块的块尾」，进而命中 `on_editor_mouse_move` 的「锚点与落点同块则交由块自身处理」提前返回——拖动在该块上整段失效。修改选区 / 命中前先查这两处。
- 同一段文本在块内有**两套坐标**：干净偏移（可见文本，跨块选区端点存这套）与显示偏移（编辑时显形出来的 `**` 这类记号也算，命中测试与高亮用这套）。换算只走 `clean_to_current_*` / `clean_range_to_display_range`，不要在两套之间手算加减。
- 选区有两个模型：跨块是 `CrossBlockSelection`（`src/editor/selection.rs`，端点是「可见块 + 块内干净偏移」）；表格里跨格是 `TableTextSelection`（`src/editor/selection/table.rs`，端点是「第几行第几列 + 格内干净偏移」，高亮按格切段铺到格子的 `editor_selection_range`，复制交可见文本——同行制表符、行间换行）。格子的命中测试在 `table_edit.rs:table_cell_at_point`。
