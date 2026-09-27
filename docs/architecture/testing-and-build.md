# Testing, Benches & Build（测试、基准与构建基建）

> 面向后续 agent 的代码导览。写作于 2026-09-28，基于 `perf` 分支。
> **行号会漂移，函数名不会**——引用以 `文件:函数名` 为主。
> 相关文档：[overview.md](./overview.md)、[editor-core.md](./editor-core.md)、[render-pipeline.md](./render-pipeline.md)、[workspace-ui.md](./workspace-ui.md)

## 1. 测试组织

- **没有顶层 Rust `tests/` target**；`tests/` 目录只放夹具。全部 Rust 测试是 bin crate 内的 `#[cfg(test)]` 模块（700+ 项）。
- **引导**：`init_editor_test_app`（src/editor/tests.rs）在 `TestAppContext` 里初始化 `I18nManager`/`ThemeManager`/`components::init`。窗口用 vendored 辅助 `cx.add_window_view(...)`（vendor/gpui/src/app/test_context.rs）。
- **重绘惯用法**：`redraw()`（tests.rs）= `window.draw(cx).clear()` + `run_until_parked`。断言元素真实布局用 `debug_bounds("名字")` + `simulate_click`（vendored gpui 的 test-only 通道）。
- **配置隔离**：`override_test_config_root`/`TestConfigRootGuard`（src/config/mod.rs，线程局部 RAII）。
- 主要测试文件：src/editor/tests.rs（156 个 gpui 测试：滚动条几何/渲染窗口裁剪、保存/IME/冲突、表格运行时、图片运行时、undo/模式切换、大文档 G8、崩溃恢复、wikilink、命令注册表守卫等）、workspace.rs 内嵌测试（文件树/搜索/大纲，用 debug_bounds）、document.rs/events.rs 内嵌测试。
- **源码审计测试**：`app_source_never_uses_native_prompts`（禁系统原生弹窗）、命令注册表守卫（每条注册命令必须有处理者）。
- 夹具：tests/fixtures/markdown-baseline.md（`include_str!` 进单元测试）；tests/fixtures/perf/*.md 是 **gitignored**，用 `node scripts/generate-fixtures.mjs tests/fixtures/perf` 生成。

## 2. 跑测试的注意点

```bash
cargo build        # 必须与 cargo test 都跑（见下）
cargo test         # 全量；大文档预算测试需要先生成 perf 夹具
```

- vendored gpui 中 `#[cfg(any(test, feature = "test-support"))]` 的代码只在测试构建存在——**只跑 `cargo test` 会漏掉生产构建编译错误**。
- `cargo test -p gpui`/在 vendor/gpui 里单独跑 gpui 测试**跑不通**（非 workspace 成员 + 本地补丁），不要当门禁。
- 基线偶发：`autosave_does_not_overwrite_external_file_changes` 历史上偶发失败，勿误判为回归。

## 3. 性能探针与预算断言

- **`manual_markdown_load_probe`**（src/editor/tests.rs，`#[ignore]`）：
  ```bash
  VELORA_PERF_FILE=<utf8文件> cargo test manual_markdown_load_probe -- --ignored --nocapture
  ```
  输出 `construct_ms / first_draw_ms / steady_p95_ms / edit_update_ms / serialize_ms`（12 次重绘取 p95）。走 `Editor::from_markdown`——**测代码文件路径需要另设入口**。
- **`VELORA_STARTUP_TIMING=1`**（src/main.rs `startup_timing_enabled`/`log_startup_phase`）：分阶段启动耗时打 stderr。
- **大文档预算断言**：`large_document_opens_within_budget`（tests.rs）——首开块数、`open_elapsed*5 < total_elapsed`（渐进导入比例性）、分块导入与单遍逐字节一致、**每块成本 ≤ 400µs**、180s 续建死线；预算 `[1,2,3,5,8]` 等价性用例。

## 4. Criterion benches（benches/，`cargo bench --bench <name>`）

每个 bench 对应一次历史性能提交的前后对比；共享 mock 在 benches/common/mod.rs（bin crate 无法引用 prod 类型，mock 对齐真实分配规模）。

| bench | 度量 |
|---|---|
| theme_arc / i18n_arc | Arc 共享 vs 深拷贝 |
| build_text_runs | 单调 span 扫描 vs 每 Cursor find（文本→TextRun 热路径） |
| shared_display_text | 块可见文本 SharedString 缓存 vs 每帧分配 |
| grapheme_cursor | 图形素光标跳跃 vs 全文扫描 |
| blink_throttle | 光标闪烁 0.5s 节流 vs 30Hz notify |
| projection_cache | 投影缓存键短路 vs 全量重建 |
| render_loop | 上述综合：50/200 块模拟每帧成本 |
| 其余（html_attr_parser、table_cells_collect 等） | 各次 review 修复的微基准 |

## 5. 构建与 profile

- **build.rs**：仅 Windows——embed-resource 内嵌 Common-Controls v6 manifest（`TaskDialogIndirect` 运行时硬依赖）。
- **.cargo/config.toml**：`rustc-wrapper = "sccache"`。
- **profiles**（Cargo.toml）：
  - `release`：codegen-units=1 + lto + opt-level=3 + panic=abort + strip（macOS 发布用）
  - `releasewin`：release + debug-assertions（交叉编译无法跑 fxc.exe，打开 debug 断言让 DirectX 走运行时着色器路径）
  - `dev`：opt-level=0、256 codegen-units；**`[profile.dev.package]` 对 ~40 个热 crate（gpui/taffy/cosmic-text/rustybuzz/lyon/tree-sitter/ratex/pulldown-cmark…）单独 opt-level=3**——本地代码保持 O0 可调试，框架热路径保持性能。新增重依赖若在每帧路径上，记得加进这张表。
- **vendoring**：`[patch.crates-io] gpui = { path = "vendor/gpui" }`（gpui 0.2.2 + `runtime_shaders`；dev 依赖带 `test-support`）。
- **features**：`code-highlight-core/official/config`（tree-sitter 16 语言语法树高亮）、`html-native`。

## 6. vendor/gpui 本地补丁清单（重要！升级 gpui 必须重放）

权威说明见 `vendor/gpui/README.md`（注意其 README 低估了改动面，以代码内 `本地补丁` 标注为准）：

1. **Focus-handle 清理**（最大补丁）：`window.rs` FocusMap 加 `dropped: AtomicBool`，最后一个句柄释放时置位；`app.rs release_dropped_focus_handles` 仅在置位时清理——原版每个事件全表扫描焦点句柄，大文档建块时是主要瓶颈。
2. **逐 run 字号**：`TextRun::font_size: Option<Pixels>`（text_system.rs/line_layout.rs；macOS CoreText、Windows DirectWrite、NoopFont 测试端已接；**Linux cosmic-text 未接**，恒 None）。
3. **debug_bounds 逐帧清理**：`Frame::clear` 清 debug_bounds（test 门控），防断言读到陈旧边界。
4. **测试用真实平台 shaping**：`shape_line_with_platform_text_system`（macOS+test-support）。
5. **Windows 着色器内嵌**：HLSL `include_str!` 进二进制，修交叉编译产物启动失败。

## 7. 脚本与资源

- scripts/generate-fixtures.mjs（perf 夹具生成，1MiB/10MiB 重复中文段落单元）+ .test.mjs 自测。
- scripts/package-macos.sh：release 构建 → .app（Info.plist/图标）→ pkgbuild/productbuild → dist/。
- scripts/package-windows.sh + .nsi：x86_64-pc-windows-gnu 交叉编译 `--profile releasewin`，断言 PE32+ 且内嵌 manifest 字符串（回归守卫），NSIS 安装包。
- resources/：macOS Info.plist/pkg Distribution、windows .rc/.manifest（build.rs 内嵌）、linux desktop 文件。
- assets/icon/：应用图标 + 标题栏 chrome SVG（main.rs 内嵌）+ 侧栏 SVG 图标集。
- **i18n 是代码不是数据文件**：src/i18n/mod.rs 内置 zh-CN/en-US 两套；外部 JSON(C) 语言包经 `from_json` 加载（用户 languages 目录直放即用）。新增 UI 字符串要同时登记两处内置表。
