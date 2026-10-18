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
git submodule update --init --recursive
cargo build                 # 零警告
cargo test --bin velora     # 全绿
```

- **两条都要跑**：vendored gpui 里有只在测试构建存在的代码（`#[cfg(any(test, feature = "test-support"))]`），只跑测试会漏掉生产构建的编译错误。
- `cargo test -p gpui` 不是主仓命令。`vendor/gpui` 是独立工作区，包名为 `gpui-pre`；依赖包自身的完整测试另有资源约束，不能代替应用门禁，见子仓 README。
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

### 更新链路自动演练

无需新版本、网络或真实安装，在仓库目录执行：

```sh
cargo test --bin velora update_flow_ -- --test-threads=1
```

`editor/tests/update_flow.rs` 驱动菜单检查、更新按钮及保存／取消按钮，使用内存发布信息和少量夹具字节，复用正式发布解析、大小／格式／SHA-256 校验、保存守卫及安装计划生成。安装器由只记录调用的测试桩代替，GPUI 测试平台不退出实际应用；夹具和文档仅写入独立临时目录。

五条用例覆盖干净文档更新、未保存文档保存后继续、取消后保留编辑、校验失败后重试、安装器启动失败后重试。正式构建固定使用系统更新后端，替换入口只在测试构建存在。系统安装器执行、权限、签名及更新后真实应用启动仍需独立验收，这条命令验证应用内部链路。

## 7. GPUI / Kit 本地兼容补丁索引

完整说明以 [vendor/README.md](../../vendor/README.md) 为准，涵盖发布包基线、修改原因、平台差异、全部源码文件及升级验证。旧补丁已逐项移植或由新版上游替代；Windows 完整 IME 日志、选择器目录失败回退与原 client GUID，以及 Linux 兼容字段均已补齐。具体迁移状态及原生验证边界只维护在该 README。不能只依赖“本地补丁”注释或 vendor 引入后的提交记录。下列编号保留代码注释中的引用关系（例如 CF_HTML 指向本节第 9 条），细节不再维护第二份。

1. **焦点句柄清理**：`window.rs:FocusMap` 与 `app.rs:release_dropped_focus_handles`，只在发生最后引用释放时清理焦点表。
2. **逐段字号**：普通 `TextRun` 保持上游结构，显式 `FontSizedTextRun` 与后端 `FontRun` / `ShapedRun` 的字号覆盖贯穿 shaping、缓存与绘制；macOS、Windows、Noop 已接入；CosmicText 保留旧版字段兼容，尚不处理字号覆盖，项目不支持 Linux。
3. **测试帧数据**：`Frame::clear` 清理 `debug_bounds` 与 `debug_hovered`；Div 记录悬停事实，`VisualTestContext::debug_hovered` 提供查询，均受测试门控。
4. **真实平台 shaping 测试入口**：macOS 测试直接使用 `gpui-pre-apple` 的公开 `AppleTextSystem`，不再从核心反向引用平台实现。
5. **Windows 着色器内嵌**：`directx_renderer.rs:build_shader_blob` 在 `debug_assertions` 路径从内存编译 HLSL，并展开 include；非调试断言路径仍使用既有预编译字节。
6. **Windows 前台任务预算**：`WindowsPlatformInner::run_foreground_task` 新版上游已有 10 ms 批次预算，本地不再重放旧调度补丁。
7. **文件选择器**：Windows 打开/保存使用新版上游的 STA 线程与 owner 保护，保留目录失败回退和原 client GUID；`PathPromptOptions::directory` 在 Windows、macOS、Linux portal 接入起始目录。固定 client GUID 只隔离壳层旧状态，不会永久消除不可达目录的等待。
8. **无追踪实体读**：`Entity::read_untracked` 跳过 accessed 登记，调用方自行保证必要的重绘通知。
9. **HTML 剪贴板**：完整移至应用的 `components/markdown/html_paste.rs`，`ClipboardItem` 恢复原版结构；Windows 读取入口保留 2 MiB 上限、内存锁定保护和剪贴板代数检查，macOS 支持读写。Windows 未增加 HTML 写入。
10. **macOS 标签拖动与拖窗**：内容视图禁用默认鼠标拖窗，`MacWindow::start_window_move` 保留事件并延后发起原生拖窗，临时允许移动后恢复，避免嵌套事件循环重入。
11. **中文排版**：`LineWrapper` 共用行首/行末禁则，`WrappedLine::wrap_at_boundaries` 接收合法及紧急断点；`add_horizontal_spacing` / `spacing_before` 提供不修改原文的视觉留白。
12. **图片布局**：缩放由应用的尺寸和容器比例实现；底层仅保留百分比宽度时高度按宽高比推导的修复。
13. **外部拖放载荷**：直接使用新版上游公开的 `ExternalPaths` 元组构造器，不再保留底层构造补丁。
14. **测试窗口观察**：TestWindow 支持读取标题、记录移动请求与调用控件命中回调；`VisualTestContext` 提供对应查询。
15. **构建兼容与裁剪**：使用新版发布包清单；核心的宿主 manifest 特性避免与应用资源重复嵌入。

## 8. 脚本与资源

- `scripts/generate-fixtures.mjs`（perf 夹具：1MiB/10MiB 重复中文段落单元）+ `.test.mjs` 自测。
- `scripts/package-macos.sh`：release 构建 → `.app`（Info.plist/图标）→ pkgbuild/productbuild → `dist/`。
- `scripts/package-windows.ps1` + `.nsi`：PowerShell 7 在 Windows x64 MSVC 环境原生 `--release` 构建，自动定位 `fxc.exe`，断言 PE32+ 且内嵌 manifest 字符串（回归守卫），生成 NSIS 安装包。
- `resources/`：macOS Info.plist 与 pkg Distribution、Windows `.rc`/`.manifest`（build.rs 内嵌）。
