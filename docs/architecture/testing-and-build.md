# 测试与构建基建

> 面向后续 agent 的代码导览。写作于 2026-09-28，基于 `perf` 分支。
> **行号会漂移，函数名不会**——引用以 `文件:函数名` 为主。
> 相关文档：[overview.md](./overview.md)、[editor-core.md](./editor-core.md)、[render-pipeline.md](./render-pipeline.md)、[workspace-ui.md](./workspace-ui.md)

## 1. 测试组织

- **没有顶层 Rust `tests/` target**；测试夹具统一放在根目录 `fixtures/`。全部 Rust 测试是 bin crate 内的 `#[cfg(test)]` 模块（1069 项）。
- **2026-09-30 起测试与源码分离**：内联测试模块已全部迁出为同级文件——`editor/tests.rs`+`editor/tests/`（20 个主题文件 + common.rs 脚手架）、`workspace/tests/`、`document/tests/`、`events/tests/`、`block/runtime/tests/`、以及各文件的 `<name>/tests.rs`。引导与共享助手集中在 `editor/tests/common.rs`。
- **引导**：`init_editor_test_app`（src/editor/tests/common.rs）在 `TestAppContext` 里初始化 `I18nManager`/`ThemeManager`/`components::init`。窗口用 vendored 辅助 `cx.add_window_view(...)`（vendor/gpui/src/app/test_context.rs）。
- **重绘惯用法**：`redraw()`（editor/tests/common.rs）= `window.draw(cx).clear()` + `run_until_parked`。断言元素真实布局用 `debug_bounds("名字")` + `simulate_click`（vendored gpui 的 test-only 通道）。
- **配置隔离**：`override_test_config_root`/`TestConfigRootGuard`（src/config/mod.rs，线程局部 RAII）。
- 主要测试位置：src/editor/tests/（gpui 端到端：滚动条几何/渲染窗口裁剪、保存/IME/冲突、表格运行时、图片运行时、undo/模式切换、大文档 G8、崩溃恢复、wikilink、命令注册表守卫等）、workspace/tests/（文件树/搜索/侧栏/标签）、document/tests/（导入与往返）、events/tests/（块事件）、block/runtime/tests/（块运行时）。
- **源码审计测试**：`app_source_never_uses_native_prompts`（禁系统原生弹窗）、命令注册表守卫（每条注册命令必须有处理者）。
- 夹具：fixtures/markdown-baseline.md（`include_str!` 进单元测试）；fixtures/perf/*.md 是 **gitignored**，用 `node scripts/generate-fixtures.mjs fixtures/perf` 生成。

## 2. 跑测试的注意点

```bash
cargo build        # 必须与 cargo test 都跑（见下）
cargo test         # 全量；大文档预算测试需要先生成 perf 夹具
```

- vendored gpui 中 `#[cfg(any(test, feature = "test-support"))]` 的代码只在测试构建存在——**只跑 `cargo test` 会漏掉生产构建编译错误**。
- `cargo test -p gpui`/在 vendor/gpui 里单独跑 gpui 测试**跑不通**（非 workspace 成员 + 本地补丁），不要当门禁。
- 基线偶发：`autosave_does_not_overwrite_external_file_changes` 历史上偶发失败，勿误判为回归。

## 3. 性能探针与预算断言

- **`manual_markdown_load_probe`**（src/editor/tests/loading_chunks.rs，`#[ignore]`）：
  ```bash
  VELORA_PERF_FILE=<utf8文件> cargo test manual_markdown_load_probe -- --ignored --nocapture
  ```
  输出 `construct_ms / first_draw_ms / steady_p95_ms / edit_update_ms / serialize_ms`（12 次重绘取 p95）。走 `Editor::from_markdown`——**测代码文件路径需要另设入口**。
- **`VELORA_STARTUP_TIMING=1`**（src/main.rs `startup_timing_enabled`/`log_startup_phase`）：分阶段启动耗时打 stderr。
- **大文档预算断言**：`large_document_opens_within_budget`（src/editor/tests/import_perf.rs）——首开块数、`open_elapsed*5 < total_elapsed`（渐进导入比例性）、分块导入与单遍逐字节一致、**每块成本 ≤ 400µs**、180s 续建死线；预算 `[1,2,3,5,8]` 等价性用例。

## 4. 构建与 profile

- **build.rs**：仅 Windows——由应用通过 embed-resource 唯一嵌入图标与 Common-Controls v6 manifest（`TaskDialogIndirect` 运行时硬依赖）。生产和测试依赖都关闭 GPUI 的 `windows-manifest` 特性，其余默认特性保持启用；否则原生 MSVC 会因两个 `RT_MANIFEST / name 1` 报 CVT1100。
- **.cargo/config.toml**：`rustc-wrapper = "sccache"`。
- **profiles**（Cargo.toml）：
  - `release`：codegen-units=1 + lto + opt-level=3 + panic=abort + strip（macOS 与 Windows 发布共用，默认关闭 debug-assertions）；Windows 在原生 MSVC 环境中用 Windows SDK 的 `fxc.exe` 预编译 DirectX 着色器。
  - `dev`：opt-level=0、256 codegen-units；**`[profile.dev.package]` 对 ~40 个热 crate（gpui/taffy/cosmic-text/rustybuzz/lyon/tree-sitter/ratex/pulldown-cmark…）单独 opt-level=3**——本地代码保持 O0 可调试，框架热路径保持性能。新增重依赖若在每帧路径上，记得加进这张表。
- **vendoring**：`[patch.crates-io] gpui = { path = "vendor/gpui" }`（gpui 0.2.2 + `runtime_shaders`；dev 依赖带 `test-support`）。
- **features**：`code-highlight-core/official/config`（tree-sitter 16 语言语法树高亮）。
- **CI**：macOS ARM64（`macos-15`）、macOS Intel（`macos-15-intel`）、Windows x64（`windows-2025`，MSVC）各自原生构建并运行默认测试与确定性慢用例；墙钟预算闸门继续排除。Release 在相同三个原生 runner 上出包，保留默认 CPU 指令集兼容性。
- **验证出包**：在 GitHub Actions 的 Release 工作流中点 `Run workflow`，选择待验证分支与 `action=package`。`tag` 留空使用该分支的 `Cargo.toml` 版本，也可填写 `vX.Y.Z` 覆盖安装包版本（无需创建标签）。完成后下载 `release-macos-arm64`、`release-macos-x64`、`release-windows-x64` 三份 artifact，分别实机安装并启动验证。手动出包仅上传 artifact；推送 `v*` 标签才创建 GitHub Release。手动 `action=update-notes` 保留更新既有发布说明的功能，必须填写现有标签。

## 5. vendor/gpui 本地补丁清单（重要！升级 gpui 必须重放）

权威说明见 `vendor/gpui/README.md`（注意其 README 低估了改动面，以代码内 `本地补丁` 标注为准）：

1. **Focus-handle 清理**（最大补丁）：`window.rs` FocusMap 加 `dropped: AtomicBool`，最后一个句柄释放时置位；`app.rs release_dropped_focus_handles` 仅在置位时清理——原版每个事件全表扫描焦点句柄，大文档建块时是主要瓶颈。
2. **逐 run 字号**：`TextRun::font_size: Option<Pixels>`（text_system.rs/line_layout.rs；macOS CoreText、Windows DirectWrite、NoopFont 测试端已接；**Linux cosmic-text 未接**，恒 None）。
3. **debug_bounds 逐帧清理**：`Frame::clear` 清 debug_bounds（test 门控），防断言读到陈旧边界。
4. **测试用真实平台 shaping**：`shape_line_with_platform_text_system`（macOS+test-support）。
5. **Windows 着色器内嵌**：HLSL `include_str!` 进二进制，修交叉编译产物启动失败。
6. **Windows 主线程任务泵限时**：`WindowsPlatformInner::run_foreground_task` 一次唤醒最多跑 10ms 主线程任务，跑满就把 `WM_GPUI_TASK_DISPATCHED_ON_MAIN_THREAD` 重投一次（对齐上游 zed#43678）。原版用 `main_receiver.drain()` 把队列一次跑完，任务积压时 Windows 消息循环拿不到处理机会——原生文件对话框的模态循环靠它转，表现就是对话框卡住不响应。`WindowsPlatformInner` 因此多持一个 `platform_window_handle`（构造签名多一个 `HWND`）。
7. **Windows 原生文件对话框走专用 STA 线程**：`prompt_for_paths`/`prompt_for_new_path`（`platform/windows/platform.rs`）在名为 `velora-file-dialog` 的线程上创建并 `Show` 对话框（该线程自己 `OleInitialize`/`OleUninitialize`），结果经 oneshot 发回，不再占用 gpui 的 UI 线程。原因：同一进程里第 2 次开对话框时，壳层把窗口建好、甚至让它先成为前台窗口，却十几秒不 `ShowWindow`（本机实测第 1 次 0.6s、第 2 次 10.5s，更早一次 62s，切到别的应用后它才冒出来）。机制：gpui 的 UI 线程启动时就 `OleInitialize`（`platform.rs:96`），对话框原先就住在同一个 STA 公寓里，而刚关掉的模态会在这个线程队列里留下激活/焦点消息（`WM_ACTIVATE`/`WM_SETFOCUS`）——同类先例 AvaloniaUI/Avalonia#21266，其 #21433 描述的症状与本机几乎相同（「连着开两次、点取消就卡住，不是必现」），修法也是把选择器挪到专用 STA 线程。`HWND` 不是 `Send`，用 `SendHwnd` 包一层，到了对话框线程再用 `IsWindow` 复验。
   两个对话框都设了固定 `SetClientGuid`：按 MSDN，状态默认按可执行文件名持久化，换 key 就把旧状态（可能记着一个已不可达的网络位置）一次性退役。**注意这是一次性的**，状态会重新攒起来。
   这条修复管不到的情形：壳层在显示窗口前访问不可达位置（断开的映射网络盘、Quick access 里失联项、慢的第三方命名空间扩展）。那类只能在打开前用 `SetFolder` 显式指定目录压掉（保存对话框已有，打开对话框没有——`PathPromptOptions` 目前也传不了目录）。

## 6. 脚本与资源

- scripts/generate-fixtures.mjs（perf 夹具生成，1MiB/10MiB 重复中文段落单元）+ .test.mjs 自测。
- scripts/package-macos.sh：release 构建 → .app（Info.plist/图标）→ pkgbuild/productbuild → dist/。
- scripts/package-windows.ps1 + .nsi：PowerShell 7 在 Windows x64 MSVC 环境中原生 `--release` 构建，自动定位 Windows SDK 的 `fxc.exe`，断言 PE32+ 且内嵌 manifest 字符串（回归守卫），生成 NSIS 安装包。
- resources/：macOS Info.plist/pkg Distribution、windows .rc/.manifest（build.rs 内嵌）。
- assets/icon/：应用图标 + 标题栏 chrome SVG（main.rs 内嵌）+ 侧栏 SVG 图标集。
- **i18n 是代码不是数据文件**：src/i18n/mod.rs 内置 zh-CN/en-US 两套；外部 JSON(C) 语言包经 `from_json` 加载（用户 languages 目录直放即用）。新增 UI 字符串要同时登记两处内置表。
