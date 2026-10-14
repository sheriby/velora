# 测试与构建基建

> 基于 `dev`，2026-10-14 重写。引用用 `文件:函数名`。
> 相关：[overview.md](./overview.md) · [performance.md](./performance.md) · 流程见技能 `velora-tdd`

## 1. 测试组织

- **没有顶层 `tests/` 目标**：全部是 bin crate 内的 `#[cfg(test)]` 模块。命令是 `cargo test --bin velora`（这个包没有 lib 目标），单条跑 `cargo test --bin velora <测试名> -- --nocapture`。
- **测试与源码分离**：内联测试模块已迁出为同级文件——`editor/tests.rs` + `editor/tests/`（40+ 主题文件 + `common.rs` 脚手架）、`editor/workspace/tests/`、`editor/document/tests/`、`editor/events/tests/`、`components/block/runtime/tests/`，以及各模块的 `<name>/tests.rs`。新测试照这个放法，不要内联在源文件尾部。
- **脚手架**：`editor/tests/common.rs:init_editor_test_app` 在 `TestAppContext` 里初始化 i18n、主题与组件；窗口用 vendored 辅助 `cx.add_window_view(...)`。
- **重绘惯用法**：`redraw()` = `window.draw(cx).clear()` + `run_until_parked`。**断言几何之前必须先重绘**（命中测试读上一帧写下的 `last_bounds`/`last_layout`）。断言真实布局用 `debug_bounds("名字")` + `simulate_click`（vendored gpui 的 test-only 通道）。
- **配置隔离**：`override_test_config_root` / `TestConfigRootGuard`（`src/config/mod.rs`，线程局部 RAII）。读写 `config.toml` 的用例必须独占目录。
- **源码审计测试**：`app_source_never_uses_native_prompts`（禁系统原生弹窗）、命令注册表守卫（每条注册命令有处理者、面板 id 与键位表同源）。
- 计时用 GPUI 自己的 `cx.background_executor().timer(..)`，**不要**用 `smol::Timer::after`（后者不被 GPUI 调度器跟踪，`run_until_parked` 会以为没事可做）。
- 无窗口测试的文本系统是等宽模拟（`NoopTextSystem`）：只断言结构与偏移，别断言像素宽度与字形。后台执行器跑在调用线程上，**线程号不能当判据**。需要 unix 专属夹具的用例要 `cfg(unix)`。

## 2. 门禁与跑法

```bash
cargo build                 # 零警告
cargo test --bin velora     # 全绿
```

- **两条都要跑**：vendored gpui 里有只在测试构建存在的代码（`#[cfg(any(test, feature = "test-support"))]`），只跑测试会漏掉生产构建的编译错误。
- `cargo test -p gpui` / 在 `vendor/gpui` 里单独跑 gpui 测试**跑不通**（非 workspace 成员 + 本地补丁），不要当门禁。
- **门禁只在收尾跑，不要改一版跑一版**：中途用 `cargo test --bin velora <关键词>`（单条/单组，秒级）；`cargo build` 是分钟级，反复跑纯磨时间。只看某个测试的打印时，若 `--nocapture` 的输出被工具链包装吃掉，直接跑 `target/debug/deps/velora-*` 加测试名比重链接快。
- 不动代码的改动（文档、注释、AGENTS.md、CHANGELOG）不跑构建与测试。
- 构建警告清零：警告当缺陷处理。clippy 不作门禁，但 `Cargo.toml` 的 `[lints.clippy]` 放开过哪些要心里有数。

## 3. 两族 `#[ignore]`，别混

| 族 | 理由串写法 | 本地 | CI |
|---|---|---|---|
| **慢用例**（>1s，断言全是确定性的） | `慢用例（>1s）：本地默认跳过，CI 跑` | 默认跳过 | `Test (慢用例)` 步骤跑 |
| **墙钟预算闸门**（断言里量真实耗时） | `墙钟预算闸门…` | 手动跑 | **永不跑**（弱机器假红） |

- 墙钟族集中在 `editor/tests/perf_budgets.rs`，另有四处原地标：`workspace/tests/search_perf.rs`、`buffer/tests.rs`、`loading_chunks.rs` 的两条手动探针。**新增墙钟闸门若落在这些路径之外，要同步补 CI 里的 `--skip`。**
- 两族的跑法各只有一处：CI 看 `.github/workflows/ci.yml` 的两个 `cargo test` 步骤，本地看 `.config/nextest.toml`。
- 墙钟闸门即便手动跑，并发抢 CPU 下仍会假红：单跑能过、整跑重跑也过就按假红处理，**不要放宽预算**。
- `autosave_does_not_overwrite_external_file_changes` 历史上有偶发失败。

## 4. 夹具

- `fixtures/regressions/`：报修带来的原文用例。整篇验收在 `editor/tests/author_report.rs`，测试名按报告条目编号（`item09_…`）。**片段单测全绿不等于作者那种写法没坏**，收尾要跑那一份。
- `fixtures/markdown-baseline.md`：`include_str!` 进单元测试。
- `fixtures/perf/*.md`：**gitignored**，用 `node scripts/generate-fixtures.mjs fixtures/perf` 生成。CI 在慢用例步骤前跑同一条命令现场生成——不生成的话，用夹具的那几条用例会**静默自跳**（找不到夹具就 return），CI 上只剩个绿色的空跑。
- `assets/`：图标、标题栏 chrome SVG（`main.rs` 内嵌）、侧栏图标集、README 截图。**这些图常兼作测试夹具**：换/删之前先 grep 测试引用；可变资产的属性别写死进断言（PNG 宽高去 IHDR 读）。

## 5. 构建与 profile

- `build.rs`：仅 Windows——由应用通过 embed-resource 唯一嵌入图标与 Common-Controls v6 manifest（`TaskDialogIndirect` 是运行时硬依赖）。生产与测试依赖都关闭 gpui 的 `windows-manifest` 特性，其余默认特性保持启用；否则原生 MSVC 会因两个 `RT_MANIFEST / name 1` 报 CVT1100。
- `.cargo/config.toml`：`rustc-wrapper = "sccache"`。
- `release`：codegen-units=1 + lto + opt-level=3 + `panic=abort` + strip（macOS 与 Windows 共用）；Windows 在原生 MSVC 环境用 Windows SDK 的 `fxc.exe` 预编译 DirectX 着色器。**`panic=abort` 意味着任何 panic 都是进程终止**——越界切片与断言失败没有挽回机会。
- `dev`：opt-level=0、256 codegen-units；**`[profile.dev.package]` 对约 40 个热 crate（gpui/taffy/cosmic-text/rustybuzz/lyon/tree-sitter/ratex/pulldown-cmark…）单独 opt-level=3**——本地代码保持 O0 可调试，框架热路径保持性能，dev 构建的性能数字因此有代表性。**新增重依赖若在每帧路径上，记得加进这张表。**
- vendoring：`[patch.crates-io] gpui = { path = "vendor/gpui" }`（gpui 0.2.2 + `runtime_shaders`；dev 依赖带 `test-support`）。
- features：`code-highlight-core` / `-official` / `-config`（tree-sitter 语法高亮的总开关、18 语言 grammar、yaml+toml）。

## 6. CI 与出包

- CI（`.github/workflows/ci.yml`）在每次 push 与 PR 上，用 macOS ARM64、macOS Intel、Windows x64 **原生 runner** 各跑同样两条门禁与确定性慢用例。墙钟闸门排除。
- Release 工作流：推 `v*` 标签 → 三个原生 runner 出包 → 创建 GitHub Release（保留默认 CPU 指令集兼容性）。
- **验证出包**（不建标签）：Release 工作流点 `Run workflow`，选分支与 `action=package`；`tag` 留空用该分支 `Cargo.toml` 的版本，也可填 `vX.Y.Z` 覆盖。产物是 `release-macos-arm64` / `release-macos-x64` / `release-windows-x64` 三份 artifact，分别实机安装启动验证。手动出包只上传 artifact；`action=update-notes` 用于更新既有发布说明，必须填现有标签。
- 发版流程见技能 `velora-release`：发布说明是 `docs/releases/<标签名>.md`，一笔 `chore(release)` 只动 `Cargo.toml`、`Cargo.lock`、`CHANGELOG.md`、发布说明；**tag 只能打在 main**，先 ff-merge dev 到 main 并推送，再打标签、推标签前请人确认。

## 7. vendor/gpui 本地补丁清单（升级 gpui 必须重放）

权威说明见 `vendor/gpui/README.md`（那份 README **低估了改动面**，以代码内 `本地补丁` 标注为准）。

1. **Focus-handle 清理**（最大补丁）：`window.rs` 的 FocusMap 加一个"已释放"原子标志，`app.rs:release_dropped_focus_handles` 仅在置位时清理。原版每个事件全表扫描焦点句柄，大文档建块时是主要瓶颈。
2. **逐 run 字号**：`TextRun::font_size: Option<Pixels>`（text_system.rs / line_layout.rs；macOS CoreText、Windows DirectWrite、Noop 测试端已接，**Linux cosmic-text 未接**，恒 None）。
3. **`debug_bounds` 逐帧清理**：`Frame::clear` 清 debug_bounds（test 门控），防断言读到陈旧边界。
4. **测试用真实平台 shaping**：`shape_line_with_platform_text_system`（macOS + test-support）。
5. **Windows 着色器内嵌**：HLSL `include_str!` 进二进制，修交叉编译产物启动失败。
6. **Windows 主线程任务泵限时**：`WindowsPlatformInner::run_foreground_task` 一次唤醒最多跑 10ms 主线程任务，跑满就重投一次自定义消息（对齐上游 zed#43678）。原版把队列一次 drain 完，任务积压时消息循环拿不到处理机会——原生文件对话框的模态循环靠它转，表现是对话框卡住不响应。该结构体因此多持一个窗口句柄（构造签名多一个 `HWND`）。
7. **Windows 原生文件对话框走专用 STA 线程**：`prompt_for_paths` / `prompt_for_new_path` 在专用线程上创建并 `Show`（该线程自己 `OleInitialize`/`OleUninitialize`），结果经 oneshot 发回。原因：gpui 的 UI 线程启动时就 `OleInitialize`，对话框原先住在同一个 STA 公寓里，刚关掉的模态会在这个线程队列里留下激活/焦点消息，导致同进程第 2 次开对话框时壳层把窗口建好却十几秒不 `ShowWindow`（同类先例 AvaloniaUI/Avalonia#21266、#21433）。`HWND` 不是 `Send`，用包装类型带过去，到线程后再 `IsWindow` 复验。两个对话框都设了固定 `SetClientGuid` 把旧状态一次性退役（**注意是一次性的**，状态会重新攒起来）。管不到的情形：壳层在显示窗口前访问不可达位置（断开的映射网络盘、失联的快速访问项、慢的第三方命名空间扩展）——那类只能在打开前用 `SetFolder` 显式指定目录压掉（保存对话框已有，打开对话框受 `PathPromptOptions` 所限还没有）。
8. **`Entity::read_untracked`**：跳过 accessed 集合登记的读。状态栏一类每帧扫描在十万级块文档上曾把大量集合插入变成稳态帧的最大单项。
9. **剪贴板的 HTML 味道**（db8b009）：`platform.rs` 加 `mod cf_html;` 与 `pub use cf_html::*;`；新增 `platform/cf_html.rs`——从 `CF_HTML` 载荷按 `StartFragment`/`EndFragment`（退回 `StartHTML`/`EndHTML`）的**字节**偏移切出选中的那段标记，载荷不是合法 UTF-8 就当这一味道不存在；`platform/windows/clipboard.rs` 加 `CLIPBOARD_HTML_FORMAT` 与 `read_html_from_clipboard`，带 `MAX_CLIPBOARD_HTML_BYTES`（2 MiB）上限，超限交回 `None` 让粘贴回退到同一次复制的纯文本。没有它，从网站复制的**无表头表格**在 Windows 上只会粘成纯文本（c42baff 的消费面）。macOS 侧不经这里，由 `src/components/markdown/html_paste.rs:clipboard_html` 直读 pasteboard 的 `public.html`。

## 8. 脚本与资源

- `scripts/generate-fixtures.mjs`（perf 夹具：1MiB/10MiB 重复中文段落单元）+ `.test.mjs` 自测。
- `scripts/package-macos.sh`：release 构建 → `.app`（Info.plist/图标）→ pkgbuild/productbuild → `dist/`。
- `scripts/package-windows.ps1` + `.nsi`：PowerShell 7 在 Windows x64 MSVC 环境原生 `--release` 构建，自动定位 `fxc.exe`，断言 PE32+ 且内嵌 manifest 字符串（回归守卫），生成 NSIS 安装包。
- `resources/`：macOS Info.plist 与 pkg Distribution、Windows `.rc`/`.manifest`（build.rs 内嵌）。
