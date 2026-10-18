# GPUI Kit 与本地兼容补丁

Velora 使用 `gpui-kit 0.7.3`。其底层为 `gpui-pre 0.3.9`，发布包记录上游 Zed 提交 `93d745668805029476e6d7fbb19652116fed51de`，上游清单中的 GPUI 版本仍为 `0.2.2`。`gpui-pre` 的包版本属于快照发布序列，不能与 Zed 官方发布包的版本号直接比较。

本地副本来自对应的 crates.io 发布包，保留许可证和上游说明；根目录 `Cargo.toml` 的 `[patch.crates-io]` 统一替换消费方。旧 `vendor/gpui` 基于 `gpui 0.2.2` 的发布包，已由下面的快照基线替代。

| 目录 | 发布包基线 | 本地修改范围 |
| --- | --- | --- |
| `gpui-pre` | `0.3.9` | 焦点清理、无追踪读取、逐段字号、换行与视觉间距、图片缩放、HTML 载荷、测试观察接口、宿主 manifest |
| `gpui-pre-apple` | `0.3.9` | CoreText 逐段字号、字号构造兼容 |
| `gpui-pre-macos` | `0.3.9` | 起始目录、HTML 写入、延后原生拖窗 |
| `gpui-pre-windows` | `0.3.9` | DirectWrite 逐段字号、IME 按键归属、起始目录、HTML 读取、内嵌调试着色器 |
| `gpui-pre-linux` | `0.3.9` | portal 起始目录、Wayland HTML 字段兼容 |
| `gpui-pre-wgpu` | `0.3.9` | CosmicText 字号结果占位、FontRun 裁切与测试构造字段兼容 |
| `gpui-base` | `0.7.3` | 补齐新增文本字段，保留原有组件行为 |
| `gpui-component` | `0.7.3` | Diff 等文本构造器补齐字号缺省字段 |

`gpui-kit`、图标资源和其他未修改的依赖由 Cargo 获取。没有引入 `gpui-shell` 或 JavaScript 运行时。应用通过 Kit 的同一套 GPUI 类型交互，不同时加载旧框架。

## 迁移状态（2026-10-18）

下表按旧 `vendor/gpui/README.md` 的 15 个主题逐项核对；旧文件可从 Git 提交 `8c6f84e` 查阅。首次接入遗漏的 Windows 日志、文件选择器目录回退与 GUID，以及 Linux 兼容字段已补齐。源码移植、上游替代和测试方式替换分开记录；源码覆盖完整不表示所有平台均已通过原生验收。

| 旧编号 | 补丁主题 | 当前状态 | 当前实现或差异 |
| --- | --- | --- | --- |
| 1 | 焦点句柄按释放事件清理 | 已移植 | 核心 `FocusMap`、`FocusHandle::drop`、`release_dropped_focus_handles` |
| 2 | 无追踪实体读取 | 已移植 | 核心 `EntityMap::read_untracked` / `Entity::read_untracked` |
| 3 | 同一行内逐段字号 | 已移植 | 核心字段、合并、缓存、切片和绘制；Apple、Windows、Noop 排版；CosmicText 保留旧版仅补字段、不处理覆盖字号的边界 |
| 4 | 中文禁则及外部换行边界 | 已移植并适配新版 | 核心 `LineWrapper`、`compute_wrap_boundaries`、`wrap_at_boundaries`，保留新版续行缩进 |
| 5 | 视觉横向间距 | 已移植 | 核心 `WrappedLine::add_horizontal_spacing` / `spacing_before` |
| 6 | 图片固有尺寸缩放及百分比宽度 | 已移植 | 核心 `Img::image_scale` 与宽高比推导 |
| 7 | HTML 剪贴板 | 已移植（支持平台） | 核心 HTML 载荷与 CF_HTML；macOS 写入、Windows 读取；Windows 锁定空指针保护已有于新版上游 |
| 8 | Windows IME 按键归属及诊断 | 已移植 | 组合态按键归属、普通派发结果、字符、组合串及结果串、缺失输入处理器诊断均保留 |
| 9 | Windows 前台任务批次预算 | 新版上游已提供 | 上游 `run_foreground_task` 已有 10 ms 预算，并在让出前处理输入和重绘；不重放旧实现 |
| 10 | 文件选择器起始目录及 STA 线程 | 已移植，线程管理由上游替代 | macOS / Windows / Linux portal 的起始目录；Windows 目录失败回退及原 client GUID 保留；STA 与 owner 管理由上游提供 |
| 11 | Windows 调试着色器内嵌 | 已移植 | Windows `shader_resources::build_shader_blob` 从内存编译，并展开 include |
| 12 | macOS 标签与显式拖窗 | 已移植 | 内容视图禁用隐式拖窗，显式拖窗保留事件并延后执行；新版标题栏命中处理继续保留 |
| 13 | 外部文件拖拽数据构造 | 已移植 | 核心 `ExternalPaths::new` |
| 14 | 测试观察接口 | 已移植并替换部分验证方式 | 悬停、标题、拖窗计数及控件命中保留；真实 shaping 改用公开 `AppleTextSystem`；帧 bounds 清理由新版上游提供 |
| 15 | 布局泛型显式 `f32` | 新版上游已包含 | 干净 `gpui-pre 0.3.9` 的 `taffy.rs:to_grid_repeat` 已有 `0.0_f32` / `1.0_f32`，没有额外本地 diff |

### Windows 迁移细节

- **IME 诊断日志**：`gpui-pre-windows/src/events.rs:ime_debug_log` 记录组合态或 `VK_PROCESSKEY` 交回消息循环的决策、普通按键派发结果、`WM_CHAR`、`WM_IME_COMPOSITION`、`GCS_COMPSTR` / `GCS_RESULTSTR` 内容，以及缺失输入处理器的原因。`VELORA_IME_DEBUG=1` 开启，追加到临时目录的 `velora-ime.log`；开关在进程内缓存，默认关闭。日志包含输入内容。
- **文件选择器目录失败回退**：打开与保存共用 `shell_folder_item`；目录为空时不指定，规范化或 shell item 创建失败时记录日志、回退系统默认目录。`SetFolder` 失败也只记录日志，不中止对话框。
- **client GUID**：打开与保存继续共用原值 `7d1c2f5e-9a44-4b3c-8f61-5e0a9b6c4d27`，保留系统已记住的选择器状态。首次适配擅自更换的两个 GUID 已撤回。

Windows 的组合按键现在返回给外层消息循环，由它调用 `TranslateMessage`，不是照搬旧版在处理器内直接调用的方式；否则可能翻译两次。上游 `dialog.rs:show_dialog` 已提供专用 STA 线程、owner 生命周期保护、取消及同一 owner 的请求串行化，替代旧 `run_file_dialog_in_thread`。用户取消仍返回 `None`，其他 `Show` 错误仍向上传播。

### 平台与测试边界

Velora 的发布与验收平台仍为 macOS ARM64、macOS Intel、Windows x64。旧 Linux 的 portal 起始目录与 Wayland 剪贴板 HTML 字段迁到 `gpui-pre-linux`；文本系统在新版上游已拆至 `gpui-pre-wgpu`，其 `CosmicTextSystem` 补齐 `ShapedRun`、FontRun 裁切和测试构造字段。与旧版相同，CosmicText 尚不实现逐段字号覆盖；保留这些兼容改动不代表新增 Linux 产品支持。

旧核心的 `shape_line_with_platform_text_system` 测试入口与 macOS 内部的逐段字号单测没有原样复制。应用门禁中的 `src/components/block/element/tests.rs:mac_platform_text_system_applies_per_run_font_size` 直接使用公开 `AppleTextSystem` 验证真实 CoreText 的字号与步进；它替代原验证路径，不让核心反向依赖平台包。

旧包的示例、独立测试与清单裁剪不再重放，新副本沿用各发布包的文件和目标声明。这不表示这些依赖包的全部测试目标已在应用门禁中运行。

## 已接入能力的实现边界

1. **焦点与读取成本**：`gpui-pre/src/window.rs:FocusMap` 记录最后引用释放事件；`app.rs:release_dropped_focus_handles` 无释放时跳过扫描。`app/entity_map.rs:Entity::read_untracked` 保留借用检查，跳过渲染依赖登记，仅供已有的只读热路径使用。
2. **逐段字号**：`TextRun`、`FontRun`、`ShapedRun` 的 `font_size` 贯穿字体段合并、缓存键、布局、切片和绘制。Apple 的文本系统已独立到 `gpui-pre-apple`，Windows 在 DirectWrite 的文本区间设置字号。Noop 实现仍仅用于结构与偏移测试。
3. **中文排版与视觉留白**：`text_system/line_wrapper.rs` 集中禁则判据；`WrappedLine::wrap_at_boundaries` 使用应用提供的合法、紧急断点；`add_horizontal_spacing` 与 `spacing_before` 改变显示几何，不修改原文与字节坐标。新版的续行缩进能力继续保留。
4. **图片布局**：`elements/img.rs:Img::image_scale` 同步缩放固有宽高，百分比宽度的高度由宽高比推导；不修改原始图片。
5. **剪贴板**：核心 `ClipboardItem` 保留可选 HTML；共用 `cf_html_fragment` 解析 Windows 字节偏移。Windows 读取入口限制 HTML 载荷为 2 MiB；空指针保护使用新上游的 `LockedGlobal`。macOS 的 HTML 写入迁到 `pasteboard.rs:Pasteboard::write`；应用层继续负责 macOS HTML 读取。
6. **文件选择器**：`PathPromptOptions::directory` 接入 macOS、Windows 与 Linux portal 的打开对话框。Windows 保留目录失败回退与原 client GUID，线程隔离由新版上游提供。
7. **Windows 输入与调度**：组合态和 `VK_PROCESSKEY` 返回给新消息循环，由 `TranslateMessage` 处理，不进入应用快捷键；完整诊断入口保留。前台任务的 10 ms 预算已存在于新上游 `run_foreground_task`，无需重放旧实现。
8. **Windows 构建**：调试着色器通过 `include_str!` 内嵌并展开 `alpha_correction.hlsl`，不再寻找构建机路径。核心本地默认开启 `windows-manifest-provided-by-host`，即使 Kit 的依赖合并开启 `windows-manifest`，仍由应用 `build.rs` 唯一嵌入资源。
9. **拖放与拖窗**：`ExternalPaths::new` 保留测试构造入口。macOS 内容视图显式禁用隐式原生拖窗，保留新版的标题栏命中处理；显式 `start_window_move` 仍延后执行并保留事件，避免原生嵌套消息循环重入 GPUI。
10. **测试观察**：保留窗口移动计数、控件命中与实际悬停查询；清理并复用新版帧缓存对应的观察数据。真实 CoreText 验证改用公开的 `AppleTextSystem`，移除了旧核心对 macOS 文本实现的测试专用耦合。

## 旧补丁的源码去向

旧表的 **27 个源码文件**逐项对应如下。路径均相对 `vendor/`；目录改变由新版上游拆包造成，文件消失不代表补丁已被上游吸收。

| 旧 `gpui/` 内路径 | 当前路径或处理 |
| --- | --- |
| `src/app.rs` | `gpui-pre/src/app.rs`：焦点清理 |
| `src/app/entity_map.rs` | `gpui-pre/src/app/entity_map.rs`：无追踪读取 |
| `src/app/test_context.rs` | `gpui-pre/src/app/test_context.rs`：悬停、拖窗与命中查询 |
| `src/elements/div.rs` | `gpui-pre/src/elements/div.rs`：测试悬停事实 |
| `src/elements/img.rs` | `gpui-pre/src/elements/img.rs`：图片尺寸 |
| `src/interactive.rs` | `gpui-pre/src/interactive.rs`：外部载荷构造 |
| `src/platform.rs` | `gpui-pre/src/platform.rs`：Noop 字号、目录与 HTML 字段、CF_HTML 导出；真实 shaping 测试入口改为应用直接调用 Apple 后端 |
| `src/platform/cf_html.rs` | `gpui-pre/src/platform/cf_html.rs`：解析规则原样保留 |
| `src/platform/linux/platform.rs` | `gpui-pre-linux/src/linux/platform.rs`：portal 起始目录；非法目录参数返回错误，替代旧 `expect` |
| `src/platform/linux/text_system.rs` | `gpui-pre-wgpu/src/cosmic_text_system.rs`：字号字段占位及构造兼容 |
| `src/platform/linux/wayland/clipboard.rs` | `gpui-pre-linux/src/linux/wayland/clipboard.rs`：HTML 字段兼容 |
| `src/platform/mac/platform.rs` | `gpui-pre-macos/src/platform.rs`：目录；`gpui-pre-macos/src/pasteboard.rs`：HTML 写入及构造字段 |
| `src/platform/mac/text_system.rs` | `gpui-pre-apple/src/text_system.rs`：CoreText 字号；旧新增单测由应用真实 shaping 回归替代 |
| `src/platform/mac/window.rs` | `gpui-pre-macos/src/window.rs`：隐式与显式拖窗 |
| `src/platform/test/window.rs` | `gpui-pre/src/platform/test/window.rs`：标题、移动计数、释放锁后调用命中回调 |
| `src/platform/windows/clipboard.rs` | `gpui-pre-windows/src/clipboard.rs`：HTML 读取及 2 MiB 上限；锁定保护由上游提供 |
| `src/platform/windows/direct_write.rs` | `gpui-pre-windows/src/direct_write.rs`：DirectWrite 字号 |
| `src/platform/windows/directx_renderer.rs` | `gpui-pre-windows/src/directx_renderer.rs`：内嵌调试 HLSL |
| `src/platform/windows/events.rs` | `gpui-pre-windows/src/events.rs`：IME 按键归属与完整诊断日志 |
| `src/platform/windows/platform.rs` | `gpui-pre-windows/src/platform.rs`：目录、失败回退与原 GUID；调度与 `src/dialog.rs` 的 STA 管理由上游提供 |
| `src/style.rs` | `gpui-pre/src/style.rs`：默认文本字号字段 |
| `src/taffy.rs` | `gpui-pre/src/taffy.rs`：显式 `f32` 已在干净上游，无本地源码修改 |
| `src/text_system.rs` | `gpui-pre/src/text_system.rs`：字号传递、run 合并与间距初始化 |
| `src/text_system/line.rs` | `gpui-pre/src/text_system/line.rs`：字号绘制、切片、间距和外部断点 |
| `src/text_system/line_layout.rs` | `gpui-pre/src/text_system/line_layout.rs`：字号缓存键、合法及紧急断点 |
| `src/text_system/line_wrapper.rs` | `gpui-pre/src/text_system/line_wrapper.rs`：共用禁则、默认字号字段，并适配新版包装逻辑 |
| `src/window.rs` | `gpui-pre/src/window.rs`：焦点释放、悬停观察及帧缓存复用；bounds 清理由上游提供 |

## 新增的依赖适配

相对各自干净发布包，当前共有 **35 个新增或修改的 Rust 源码文件**：核心 15、Apple 1、macOS 3、Windows 5、Linux 2、WGPU 1、Base 6、Component 2。旧源码去向表已覆盖平台与核心的修改；另有以下 8 个组件源码文件，均因核心增加公开字段而适配，不是重新实现组件行为。

| 路径 | 修改原因 |
| --- | --- |
| `gpui-base/src/input/base/element.rs` | `TextRun` 构造补齐 `font_size: None` |
| `gpui-base/src/input/base/state.rs` | 剪贴板测试构造补齐 `html: None` |
| `gpui-base/src/input/editor/display_map/text_wrapper.rs` | `TextRun` 构造补齐默认字号 |
| `gpui-base/src/input/editor/indent.rs` | 缩进量测构造补齐默认字号 |
| `gpui-base/src/plot/label.rs` | 图表文字构造补齐默认字号 |
| `gpui-base/src/text/inline.rs` | 切片后的 `ShapedRun` 继续传递原 run 字号 |
| `gpui-component/src/diff/mod.rs` | Diff 文字构造补齐默认字号 |
| `gpui-component/src/input/group/tests.rs` | 剪贴板测试构造补齐 `html: None` |

此外修改了 `gpui-pre/Cargo.toml` 与 `gpui-pre/build.rs`：新增并默认开启 `windows-manifest-provided-by-host`，避开 Kit 特性合并导致框架与宿主重复嵌入 manifest。这是本次接入新增的构建适配，不属于旧补丁的移植。各包 README 的本地说明另计，不计入源码文件数量。

## 应用接入与升级

应用的初始化、窗口内容句柄和主题投射集中在 `src/ui.rs`。每个生产窗口以 Kit `Root` 为根，内容实体通过弱索引定位；关闭窗口时删除索引。这样菜单事件可取得编辑器句柄，同时避免在窗口派发栈中重新读取窗口。未包装的独立测试窗口仍可沿用 GPUI 原生句柄。

控件颜色从 Velora 的当前主题投射到组件主题；界面字号映射到组件的相对尺寸基准，正文仍使用自身的阅读字号。自定义资源与 Kit 图标通过同一个 `AssetSource` 提供。

升级时先用对应版本的干净 crates.io 发布包比较本地副本的源码、清单和 `build.rs`，逐项核对迁移状态表与源码去向表，再调整代码。不要把旧 vendor 提交、某个“本地补丁”注释、当前上游主分支或快照包版本号当成完整的源码基线。新字段还须核对所有消费方的结构体构造、run 合并与缓存键。

根目录运行 `cargo build`、`cargo test --bin velora`；定向回归覆盖逐段字号、中文禁则、间距、图片缩放、原生菜单、窗口恢复与关闭、Kit 浮层和设置键盘操作。Windows 仍需原生 CI 与实际系统验证 IME、剪贴板和文件选择器，macOS 通过不能代替 Windows 验收。

2026-10-18 补齐遗漏后，本机 macOS ARM64 的 `cargo build --locked` 成功，`cargo test --locked --bin velora` 为 **2484 通过、0 失败、48 忽略**。新增 3 条迁移源码守卫在补齐前各自断言失败、补齐后通过；它们防止补丁漏带，不替代原生系统行为测试。`cargo check --locked -p gpui-pre-windows --target x86_64-pc-windows-gnu` 通过，验证了 Windows 后端类型与 API；Windows MSVC 原生构建及 IME、对话框操作仍待对应 CI / 系统验收。

此前还以隔离配置实际打开 macOS 编辑器和设置窗口，验证设置取消的键盘操作。没有执行 macOS Intel 或 Windows 原生验收。构建仍提示既有 `block 0.1.6` 的未来兼容问题，应用测试链接仍有既有的 `__eh_frame` 大小提示，没有通过屏蔽警告处理它们。

独立 WGPU 回归 `font_runs_are_clipped_to_segment` 通过，覆盖裁切前后字号字段保持。验证副本的全部 Rust 源码与仓库逐字节相同，仅临时清单补齐上游漏列的 `naga` 测试依赖，并提供同一 Zed 快照的字体资源，未增加应用依赖或资源。

依赖包内的测试不会自动纳入应用这 2484 项测试；WGPU 发布包的测试资源限制另见 [WGPU 本地说明](gpui-pre-wgpu/README.md)。确定性慢用例及平台门禁仍按 [构建与测试约定](../docs/architecture/testing-and-build.md) 执行。
