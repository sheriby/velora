---
name: velora-gpui-bugfix
description: Fix any bug in Velora (GPUI desktop app) the test-first way — write the failing test, watch it go red, fix, watch it go green, then run the full suite. Use for every bug report or 报修 about wrong rendering, dead clicks, off-by-a-few-characters selection/highlight, stale views, crashes or slowness. Triggers include "修 bug", "复现", "报修", "为什么不对", "测试先红后绿".
---

# Velora / GPUI 修 bug

只有一个循环，任何 bug 都走它：

**写测试 → 看它红 → 修 → 看它绿 → 全量绿 → 留下这条测试。**

跳过任何一步都不算修完。没看到红点之前不动实现；没看到绿点不算修完。

## 1 先写测试

一条断言对应一句报修。报修说什么，断言就断什么：

| 报修 | 断言 |
|---|---|
| 「删完文件里留下半截记号」 | 结果字节 == 期望字节 |
| 「最后两个字没高亮」 | 高亮区间 == 屏幕文本全长 |
| 「点了没反应」 | 动作之后的状态变了 |
| 「拖不动」 | 选区端点 == 落点换算出来的偏移 |
| 「画面不刷新」 | 第二帧的渲染输入变了 |

- **用报修里的真实素材**：真实文档片段、真实键鼠序列、真实尺寸。自己捏一个理想输入，往往复现不出那个 bug。
- **从最小单元写起**：能落到单个组件/状态机就落到那儿；只有跨实体、跨帧才上窗口级测试。
- **断言用户看得见的东西**：字节、屏幕文本、选区范围、渲染输入。别断言内部临时值，那种测试会随着重构碎掉。
- **边界各来一条**：块首、块中、块尾、空块/空文本、含隐藏记号（`**`、`` ` ``、`[](…)`）、多字节字符（CJK / emoji）。
- **注释写清两件事**：现象（客观、可复现的描述，例如“两行全选后按删除，残留 `**`”）与根因（哪一处口径写错了）。这条测试以后就是回归守卫。

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
app_cx.quit();                    // 别拿 VisualTestContext 收尾，会 SIGSEGV
```

跑单条：`cargo test --bin velora <测试名> -- --nocapture`（没有 lib target）。

## 2 看它红

红点必须红在**断言**上：

- 编译不过不是红，先修测试。
- panic 不是红（先修夹具）。
- 断言失败才是红，而且左右值要能对上用户看到的现象：屏幕上少两个字 ⇔ 区间少 4 个字节。对不上就说明你复现的不是那个 bug。

手段：

- `println!` 出左右值再用 `-- --nocapture` 看。GPUI 里 `entity.update` 内的 panic 有时不出现在 stdout，打印比断言可靠；打印被工具链包装吃掉时直接跑 `target/debug/deps/velora-*` 里的测试二进制。
- 「拖不动 / 选不上 / 点不中」的定位配方：在一个窗口级测试里先把现状打全——每个可见块的 `kind()` / `display_text()` / `last_bounds` / `clean_visible_len()`，格子的 `last_bounds`，再看 `cross_block_selection`、各块 `selected_range` / `editor_selection_range`、`active_entity_id`。谁没几何、谁没选区，一眼就出来，比读代码猜快得多。
- 如果测试是先写、实现是后改的，改实现前先 `git stash push -- <实现文件>` 跑一遍，确认红点真的是测试带来的。
- 新语义（以前根本没有这条路径）没法靠 stash 验红：把新分支临时改回旧行为（或改掉那一处判定）跑一遍，看断言红不红；验完立刻还原。

## 3 修到绿

- 一次只改一处口径。修的是换算函数或状态所属的那一层，不在每个调用点打补丁。
- 绿了先跑**相关那一组**（`cargo test --bin velora <关键词>`）；全量 `cargo build`（零警告）+ `cargo test --bin velora` 留到收尾各跑一次。全量 100–130 秒，改一版跑一版纯磨时间。
- 再验一次「这条测试真的覆盖了这个 bug」：`git stash push -- <修复文件>` 跑一遍看红、`git stash pop` 看绿；新语义用上面那条「临时改回旧行为」的办法。
- 报修里给的例子（键鼠序列、复制出去的形状、截图里的那几个字）就是验收口径，照它写断言；粒度或格式不清楚就先问一句，别自己发明一套再被打回。
- 同族扫描：共享口径一改，grep 所有读者与写者（源改了消费者没跟 = 缺陷）。
- 缓存类修复要确认 key / generation 覆盖全部输入（文本代数、字号、字体指纹、主题代数、换行宽……），漏一个输入就是下一次“不刷新”。
- 遗留副作用（多一个空行、多一次重算）写进总结，不要静默吞掉。

## 4 GPUI 的坑（写测试与看红点时最常撞）

1. **更新是延后的**：`cx.notify()` 只排一帧。要断言渲染结果，先 `window.draw(cx).clear()`（仓库里是 `redraw`）+ `run_until_parked()`，否则断言的是上一帧。
2. **几何来自上一帧**：`last_bounds` / `last_layout` 在 `prepaint` 里写。没 draw 过就没有几何，命中测试一律返回 0——「拖不动」在测试里常常是夹具没先 draw。
3. **借还锁**：`entity.read(cx)` 的 guard 必须在 `entity.update(cx, …)` 之前落地；同一实体不能同时持读锁与写锁。
4. **测试上下文**：`TestAppContext::single()` + `add_window_view` 返回 `VisualTestContext`，不是 app 上下文；收尾用 `app_cx.quit()`。异步/定时器（光标闪烁、自动保存、去抖）才需要 `#[gpui::test]`，别用来代替窗口。
5. **事件是两段式**：capture → bubble。点了没反应先查谁 `stop_propagation` 了、这段在哪个阶段、焦点与 hitbox 判定过没。
6. **文本系统**：真实字体才能量像素宽；无窗口测试用 `NoopTextSystem`，只量结构与偏移。
7. **渲染输入 ≠ 屏幕**：prepaint 的 run/quad 是断言渲染的最外层抓手；颜色、行高、`bounds` 都要在这里看。

## 5 本仓库抓手

- 测试：`src/editor/tests/common.rs`（`init_editor_test_app`、`redraw`、`perf_passes`、`perf_delta`）、`src/editor/selection/tests.rs`（`set_selection`、`assign_visible_block_bounds`）、`src/editor/tests/selection_mouse.rs`（真实按下—拖动—抬手的选区报修都在这里）、`src/components/block/runtime/tests/`（块级）。
- 交互模拟：`cx.simulate_mouse_down/move/up(point(px(x), px(y)), MouseButton::Left, Modifiers::none())`、`cx.dispatch_action(Action)`、`cx.simulate_input("x")`（敲字）。
- 选区代码：`src/editor/selection.rs`（跨块）、`src/editor/selection/table.rs`（表格跨格）、`src/editor/selection/pointer.rs`（指针与「点 → 端点」换算）；两套坐标与表格两层的坑见 §6。
- 环境：当前 Rust stable 工具链；`cargo test --bin velora`（无 lib target）。全量只在收尾跑。

## 6 表格与选区（本仓库专属的坑）

- 表格是**两层**：表格块（`BlockKind::Table`）+ 格子。**格子不在块树里**——`visible_blocks()`、`block_entity_by_id()` 都找不到它们，只能从表格块的 `table_runtime`（`header` / `rows`）或编辑器里的 `table_cells` 绑定表拿；格子的几何是格子自己画完一帧后的 `last_bounds`。
- 表格块**自己没有文本元素**：`last_bounds` 恒为空、`clean_visible_len()` 是 0、`index_for_mouse_position()` 恒给 0。任何「按可见块扫一遍」的代码（选区端点换算、命中测试、滚动锚点、大纲、搜索高亮）对表格内容都是瞎的——「表格里拖不动 / 选不上 / 点不中」的报修先查这一条。
- 屏幕上的点归哪一根块只有一处换算：`cross_block_endpoint_for_point`（`src/editor/selection/pointer.rs`）。它看不见量不出文本布局的块（表格、分隔线、未聚焦的公式与图表）时，会把那一段空间算成「上一块的块尾」，再撞上 `on_editor_mouse_move` 的「锚点与落点同块就交给块自己」早退——拖动在那一块上会整段失效。改选区/命中先看这两处。
- 同一段文本在块内有**两套坐标**：干净偏移（可见文本，跨块选区端点存这套）与显示偏移（编辑时显形出来的 `**` 这类记号也算，命中测试与高亮用这套）。换算只走 `clean_to_current_*` / `clean_range_to_display_range`，不要在两套之间手算加减。
- 选区有两个模型：跨块是 `CrossBlockSelection`（`src/editor/selection.rs`，端点是「可见块 + 块内干净偏移」）；表格里跨格是 `TableTextSelection`（`src/editor/selection/table.rs`，端点是「第几行第几列 + 格内干净偏移」，高亮按格切段铺到格子的 `editor_selection_range`，复制交可见文本——同行制表符、行间换行）。格子的命中测试在 `table_edit.rs:table_cell_at_point`。
