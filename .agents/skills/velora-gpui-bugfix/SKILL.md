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

- `println!` 出左右值再用 `-- --nocapture` 看。GPUI 里 `entity.update` 内的 panic 有时不出现在 stdout，打印比断言可靠。
- 如果测试是先写、实现是后改的，改实现前先 `git stash push -- <实现文件>` 跑一遍，确认红点真的是测试带来的。

## 3 修到绿

- 一次只改一处口径。修的是换算函数或状态所属的那一层，不在每个调用点打补丁。
- 绿了立刻跑全量：`cargo build`（零警告）+ `cargo test --bin velora`。
- 再 `git stash push -- <修复文件>` 验一次红、`git stash pop` 验绿：这一步证明「这条测试真的覆盖了这个 bug」，而不是碰巧绿。
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

- 测试：`src/editor/tests/common.rs`（`init_editor_test_app`、`redraw`、`perf_passes`、`perf_delta`）、`src/editor/selection/tests.rs`（`set_selection`、`assign_visible_block_bounds`）、`src/components/block/runtime/tests/`（块级）。
- 交互模拟：`cx.simulate_mouse_down/move/up(point(px(x), px(y)), MouseButton::Left, Modifiers::none())`、`cx.dispatch_action(Action)`。
- 环境：stable 1.88，别用更新的 std API；`cargo test --bin velora`（无 lib target）。
