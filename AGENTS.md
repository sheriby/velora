# Velora 开发约定

> 面向在本仓库干活的协作智能体。这里只写**容易踩错的地方**，不写模块地图——地图看 `docs/architecture/`。
> 文档、注释、提交信息、CHANGELOG、发布说明一律中文；代码标识符、命令、路径保持原样。
> 引用代码位置用 `文件:函数名`，行号会漂。

## 0 开工之前

- 先读 `docs/architecture/overview.md` 的「关键不变量」那一节（九条，改代码前必读）。
- 先看 `git status`：工作树里可能有使用者未提交的改动，别一起提交，也别覆盖。
- 完成的标准是两条命令都过：`cargo build`（零警告）与 `cargo test --bin velora`（全绿）。
- 提交、推送、打标签、发版都**先拿到明确授权**再做；推送标签是不可逆的公开动作。

## 1 Rust 编码

- 正确性与可读性优先；速度与效率是次级目标，除非任务另有要求。
- 注释只解释「为什么」：写下来的理由是那种不写就会被后人改回去的地方。不要写组织性注释、不要复述代码在做什么。
- 优先在既有文件里实现功能，除非它确实是一块新组件；不要造一堆小文件。
- 新目录别再新增 `mod.rs`：仓库主流写法是同名的 `foo.rs` 加 `foo/` 子目录（既有的 12 处按原样保留）。
- 产品路径不要新引入 `unwrap()` / `expect()` / 直接下标：用 `?` 传播，或显式处理失败。真要用必须写清"为什么这里不可能失败"。索引操作要自己保证边界（渲染期切字符串尤其小心多字节字符）。
- 不要静默吞错：在需要忽略的地方用带可见性的写法（日志）或显式 `match` / `if let Err(..)`，不要 `let _ = …`。既有的静默点不要顺手重写，除非正在改那一段。
- 会失败的异步操作，错误要送到界面层（一律用应用内模态，禁系统原生弹窗——有源码审计测试守着）。
- 模块内 `use gpui::*` 之后，子模块测试里的 `#[test]` 会被 gpui 的同名属性宏遮蔽（`use super::*` 会把外层 glob 带进来），宏展开递归爆栈；测试模块要么精准导入，要么用 `#[gpui::test]`。撞过一次（latex_completion 测试编译爆栈）。
- 变量名写完整单词，不用 `q`、`buf2` 这种缩写。
- 异步里用 shadowing 限定克隆的生命周期：

  ```rust
  executor.spawn({
      let task_ran = task_ran.clone();
      async move {
          *task_ran.borrow_mut() = true;
      }
  });
  ```

- 不加没被要求的功能，不做顺手重构；发现别处的毛病，写进总结提出来，不要静默夹带。
- 构建警告清零：警告当缺陷处理，别留下"反正不挡住"的警告。

## 2 GPUI

### 上下文与实体

- `App` 是根上下文；`Context<T>` 在更新实体时提供（可退化成 `&App`）；`AsyncApp` / `AsyncWindowContext` 出现在 `cx.spawn` 里，可以跨 await 持有。
- 约定：上下文参数名 `cx`，窗口参数名 `window` 且在 `cx` 之前；函数收回调时回调排在 `cx` 之后。
- `Entity<T>`：`read` / `read_with` / `update` / `update_in`，以及 `entity_id()`、`downgrade()`。闭包里的**内层 `cx` 必须用它自己那个**，拿外层 `cx` 会撞借用。
- **同一个实体不能边读边改**：`entity.read(cx)` 取到的引用必须在 `entity.update(cx, …)` 之前落地，否则运行期 panic。
- **禁止重入更新**：实体的 `update` 闭包里不得再更新同一个实体（本仓库历史上因此 coredump）。
- 互相持有句柄时用 `WeakEntity<T>` 防泄漏；它的 `read_with` / `update` 返回 `anyhow::Result`，因为目标可能已经不存在。

### 并发

- 实体与渲染全在同一个前台线程上。`cx.spawn` 在前台，`cx.background_spawn` 交给后台；前台任务常常在等后台结果再回写状态。
- `spawn` / `background_spawn` 返回 `Task<R>`，**任务被丢弃就会取消**。三种保命方式：在别的异步上下文里 await、`detach()` / `detach_and_log_err(cx)`、或存进结构体字段（结构体销毁时一起停）。
- 只要一个现成的值用 `Task::ready(value)`。

### 元素与渲染

- 实现 `Render` 的类型就是视图；`RenderOnce` 用于"造出来就变成元素"的组件，可以用 `#[derive(IntoElement)]` 直接当子元素。
- `SharedString` 用来避免拷贝字符串（`&'static str` 或 `Arc<str>`）；样式方法与 Tailwind 类似。
- 条件属性/子元素用 `.when(条件, |this| …)` 与 `.when_some(选项, |this, 值| …)`。

### 输入、动作、通知

- 事件处理器：`.on_click(|事件, window, cx| …)`；需要改当前实体的用 `cx.listener(|this, 事件, window, cx| …)`。
- 动作：`actions!(命名空间, [名字])` 定义无数据动作，`#[derive(Action)]` 定义带数据动作（动作上的文档注释会显示给用户）；派发用 `window.dispatch_action(..)` / `focus_handle.dispatch_action(..)`；处理用 `.on_action(..)`。
- 状态变了可能影响渲染就 `cx.notify()`；本仓库的缓存键是 `document_revision`，渲染期跨实体写字段是安全的（不会自动通知），要重绘必须显式通知。
- 实体事件：`cx.emit(事件)` + `impl EventEmitter<事件类型>`，别处 `cx.subscribe(..)` 拿到的 `Subscription` 要存进字段，丢了就自动退订。

### 本仓库踩过的 GPUI 限制

- svg 不继承父 `div` 的文字颜色。
- `.id()` 之后元素类型变成 Stateful，`if` / `else` 两个分支要 `into_any_element` 统一类型。
- `TextRun` 的逐段字号是本地补丁，Linux 未接。
- 一个动作只有一处实现：新增命令要键位、正文右键菜单、选中工具栏、命令面板四条入口一起有（见 `docs/architecture/editor-core.md` 第八节）。
- 提示一律应用内模态，不用系统原生弹窗。

## 3 测试

- 没有顶层 Rust `tests/` 目标：全部是 bin crate 内的 `#[cfg(test)]` 模块；命令是 `cargo test --bin velora`（这个包没有 lib 目标），单条跑 `cargo test --bin velora <测试名> -- --nocapture`。
- `cargo build` 与 `cargo test` **都要跑**：仓库内自带的 gpui（`vendor/gpui`）里只在测试构建存在的代码，会让只跑测试漏掉生产构建错误。`cargo test -p gpui` 跑不通，别当门禁。两条都在收尾各跑一次，中途只跑相关那组（见 §4）。
- 无窗口测试的文本系统是等宽模拟（`NoopTextSystem`）：那里只断言结构与偏移，别断言像素宽度与字形。要真实宽度就在窗口里用 `window.text_system().shape_text` 量。
- 断言几何之前先 `redraw`（`window.draw(cx).clear()` 加 `run_until_parked`）：命中测试读的是上一帧写下的 `last_bounds` / `last_layout`，没画过就是 0。
- 要计时用 GPUI 自己的 `cx.background_executor().timer(..)`，不要用 `smol::Timer::after(..)`——后者不被 GPUI 的调度器跟踪，`run_until_parked` 会以为没事可做。
- 夹具：`tests/fixtures/perf/*.md` 在忽略列表里，用 `node scripts/generate-fixtures.mjs tests/fixtures/perf` 生成。
- 已知假红：墙钟预算类的性能闸门在并发抢 CPU 时会假红（单跑能过、整跑重跑也过就按假红处理，不要放宽预算）；`autosave_does_not_overwrite_external_file_changes` 历史上有偶发失败。
- 修 bug 一律先写会红的测试再动实现，流程见技能 `velora-gpui-bugfix`。

## 4 工具链

- 本机是 `stable` 1.88：不要用比它更新的标准库接口（踩过的例子：`str::floor_char_boundary` 未稳定，`cargo test` 直接编不过）。
- `.cargo/config.toml` 挂了 `sccache`；`tests/fixtures/perf/` 与 `target/` 不进版本库。
- 门禁是构建与测试两条；clippy 不作门禁，但 `Cargo.toml` 里 `[lints.clippy]` 放开过哪些要心里有数。
- **门禁只在收尾跑，不在改一版跑一版**：中途改动用 `cargo test --bin velora <关键词>`（单条/单组，秒级）；`cargo build` 与全量 `cargo test --bin velora` 各只在收尾跑一次（全量实测 100–130 秒，反复跑纯磨时间，也把调试迭代拖成分钟级）。
- 只看某个测试的打印时，若 `--nocapture` 的输出被工具链包装吃掉，就直接跑 `target/debug/deps/velora-*`（编译产物里的测试二进制）加测试名，比重新链接一遍 cargo 命令快。
- 不动代码的改动（文档、注释、AGENTS.md、CHANGELOG）不跑构建与测试：只有代码路径才可能连带影响。

## 5 提交与发版

- 提交信息格式见技能 `velora-commit-message`：标题 `类型(范围): 简短描述`，正文回答背景/实现/影响/测试，脚注写破坏性变更与关联 issue。
- 一笔一个主题：测试基建修复与业务修复分开；数字当场实测，不沿用记忆。
- 发版流程见技能 `velora-release`：发布说明必须是 `docs/releases/<标签名>.md`，一笔 `chore(release)` 只动 `Cargo.toml`、`Cargo.lock`、`CHANGELOG.md`、`docs/releases/vX.Y.Z.md`，推送标签之前先请人确认。
- `CHANGELOG.md` 按 Keep a Changelog 的中文小节写；小节名与发布说明不完全同名，别互相抄错。

## 6 这份文件怎么维护

- 只放**陷阱**，不放架构地图：地图过期得快，读代码就能得到。
- 新增一条要同时满足三点：非显然（熟手不看就会写错）、反复撞到（一次会话里撞到多次也算）、能直接照着做。
- 不要在功能或修复的提交里顺手加规则：另开一笔，并在提交正文里写清它为什么存在、撞到过几次。
- 与代码冲突时以代码和 `docs/architecture/` 为准，然后回来改这份文件。
