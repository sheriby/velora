# GPUI 0.2.2 本地补丁说明

本目录基于 crates.io 发布包 `gpui 0.2.2`，上游为 [Zed GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)。Apache-2.0 许可文本原样保留在 [LICENSE-APACHE](LICENSE-APACHE)。Velora 通过根目录 `Cargo.toml` 的 `[patch.crates-io]` 使用此副本。

本文按发布包与当前目录的实际差异核对，覆盖截至 2026-10-17 的 **27 个新增或修改的源码文件**。不能仅依赖“本地补丁”注释或引入 vendor 之后的提交记录：有些修改没有标注，首次引入时也已经带有补丁。

## 基线与裁剪

- 对比基线是原始 `gpui-0.2.2.crate`，不是当前上游主分支。发布包的 `.cargo_vcs_info.json` 记录提交 `69e2130295c2649963eb639fc70b4f2ee8ea1624`，同时标有 `dirty: true`，因此该 Git 提交不能代替发布包做逐字节基线。
- 未复制 `examples/`、`docs/`、独立的 `tests/action_macros.rs`，以及包内的 `Cargo.lock`、`Cargo.toml.orig`、`.cargo_vcs_info.json`。依赖解析使用仓库根目录的 `Cargo.lock`。
- 本地 `Cargo.toml` 删除了对应的 30 个 `[[example]]` 和 1 个 `[[test]]` 目标；与发布包相比，依赖和特性声明没有改动。
- `build.rs` 和保留下来的构建资源与发布包一致；源码包含下列补丁，原 README 替换为本文。

## 补丁及适用范围

### 1. 焦点句柄按释放事件清理

`src/window.rs:FocusMap` 增加 `dropped: AtomicBool`；`FocusHandle::drop` 只在最后一个引用释放时置位。`src/app.rs:release_dropped_focus_handles` 先交换并检查标志，没有释放事件时直接返回。

原版在更新冲刷效果时反复扫描焦点表，长文档的每个编辑块都有焦点句柄，空扫描会随块数增长。补丁保留实际释放时的清理与失焦处理，不改变焦点归属规则，适用于所有平台。

### 2. 不登记渲染访问的实体读取

`src/app/entity_map.rs` 增加 `EntityMap::read_untracked` 和 `Entity::read_untracked`，跳过帧内 accessed 集合登记，供状态栏选区统计等只读热路径使用。

它仍检查上下文及实体借用，但不会为读取建立渲染失效依赖；调用方必须自行保证状态更新会通知重绘，不能普遍替换普通 `read`。

### 3. 同一行内逐段字号

`TextRun`、`FontRun`、`ShapedRun` 增加 `font_size: Option<Pixels>`，`None` 沿用整行字号。相同字体但字号不同的 run 不再合并；字号按浮点位模式进入 `FontRun` 的相等判断及缓存键。字形绘制与包围盒也使用段字号，避免外观变小而占宽不变。

- 公共链路：`src/text_system.rs`、`src/text_system/line_layout.rs`、`src/text_system/line.rs`；`src/style.rs` 的默认 run 初始化为 `None`。
- macOS：`src/platform/mac/text_system.rs:layout_line` 向 CoreText 写入逐段字号，并由 UTF-16 区间回查 shaping 结果的字号。
- Windows：`src/platform/windows/direct_write.rs:layout_line` 设置每个文本区间的字号，绘制回调保留 DirectWrite 回报的字号。
- 测试：`src/platform.rs:NoopTextSystem::layout_line` 按段字号缩放等宽步进，不能用于断言真实字体宽度。
- Linux：`src/platform/linux/text_system.rs` 仅补齐 `ShapedRun` 字段，仍写 `None`，尚未实现字号覆盖。Velora 不将 Linux 作为发布和验收平台。

### 4. 中文禁则与应用提供的换行边界

`src/text_system/line_wrapper.rs` 定义共用的 `cannot_start_line` / `cannot_end_line` 判据；普通行包装与 `src/text_system/line_layout.rs:compute_wrap_boundaries` 都使用它，避免句末标点落到下一行行首、开括号留在上一行行末。字符表是内置禁则，并非完整排版标准实现。

`src/text_system/line.rs:WrappedLine::wrap_at_boundaries` 接收应用分词器提供的合法断点和紧急断点，参数使用升序 UTF-8 字节偏移。`compute_wrap_boundaries` 将这些边界与禁则一起判断，默认调用方仍可不传边界。没有合法断点时允许局部超宽，不以任意拆开标点或字素簇兜底。绘制、光标和命中继续共用同一份布局。

### 5. 不改原文的视觉横向间距

`src/text_system/line.rs` 增加 `WrappedLine::add_horizontal_spacing`、`spacing_before` 及间距记录；`src/text_system.rs` 初始化该记录。应用可按升序字节偏移插入视觉间距，用于中西文自动留白、行内代码两侧留白。

补丁移动字形位置并调整布局宽度，不往原文插空格，也不改变文本偏移。背景范围的消费者要通过 `spacing_before` 排除不属于正文的尾侧留白；应用侧位于 `src/components/block/element.rs` 和 `element/text_element.rs`。

### 6. 图片固有尺寸缩放及百分比宽度

`src/elements/img.rs:Img::image_scale` 同时缩放图片固有宽高，使小图片也能实际缩放，而不只是改变最大尺寸限制。宽度为容器百分比时，高度交给布局根据宽高比推导，避免提前套用旧高度而拉伸图片。

补丁只影响显示布局，不重解码或修改原图；缩放范围由应用控制。正文图片与独立图片预览共用此能力。

### 7. HTML 剪贴板载荷及平台读写边界

`src/platform.rs:ClipboardItem` 增加 HTML 载荷、`new_string_with_html` 和 `html`；既有纯文本、图片构造器将该字段初始化为 `None`。

- macOS 写入：`src/platform/mac/platform.rs:write_to_clipboard` 同时写入纯文本与 `public.html`。macOS 的 HTML **读取**仍由应用的 `src/components/markdown/html_paste.rs:clipboard_html` 完成，不是这份 GPUI 补丁提供的读取接口。
- Windows 读取：`src/platform/windows/clipboard.rs` 注册 `HTML Format`，为读出的 `ClipboardItem` 附上 HTML 片段。新增 `src/platform/cf_html.rs:cf_html_fragment` 按 `StartFragment` / `EndFragment` 字节偏移截取，缺失时退回 `StartHTML` / `EndHTML` 或正文范围；片段不是合法 UTF-8 或为空时不返回 HTML。
- Windows 读取入口在复制载荷前检查 **2 MiB** 上限，超过则放弃 HTML 味道，沿用同次复制可用的纯文本。上限不在纯解析函数中，也不代表 macOS 读取入口的上限。`with_clipboard_data` 检查 `GlobalLock` 空指针，锁定失败不构造切片、不调用解锁。
- Windows 写入函数没有增加 HTML 味道写入，不能将新增构造器理解为全平台富文本写入。`src/platform/linux/wayland/clipboard.rs` 仅补齐图片载荷的 `html: None` 字段。

### 8. Windows 组合输入的按键归属及诊断

`src/platform/windows/events.rs:handle_keydown_msg` 收到 `VK_PROCESSKEY` 时直接交给 `TranslateMessage`，不将还原后的按键派发到应用快捷键。已有 marked text 时同样交给输入法，防止组合过程中的回车、退格同时改动文档。

同文件保留可选诊断：设置 `VELORA_IME_DEBUG=1` 后，将按键派发、组合串与候选结果追加到临时目录的 `velora-ime.log`。开关通过 `OnceLock` 在进程内缓存，调整后需重启；日志含输入内容，仅在排障时开启。组合/候选消息没有输入处理器时也会记录原因。

### 9. Windows 前台任务批次让出消息循环

`src/platform/windows/platform.rs:WindowsPlatformInner::run_foreground_task` 不再一次 drain 整个队列，而是检查 10 ms 批次预算，耗尽后重投主线程唤醒消息，使消息循环有机会处理输入和重绘。构造器因此保存平台消息窗口的 `HWND`。

这是批次间让出，不会抢占已经运行的单个长任务。历史上的 vsync 补足刷新间隔方案已经撤销，不在当前发布包差异中，升级时不要按旧提交重新带回。

### 10. 文件选择器的起始目录与 Windows 专用线程

`src/platform.rs:PathPromptOptions` 增加 `directory: Option<PathBuf>`，调用方构造该结构体时需要补齐字段。

- macOS：`src/platform/mac/platform.rs:prompt_for_paths` 设置 `NSOpenPanel.directoryURL`。
- Windows：`src/platform/windows/platform.rs:file_open_dialog` 和 `file_save_dialog` 通过共用的 `shell_folder_item` 设置 `SetFolder`，处理规范化路径的 verbatim 前缀；无可用目录时退回平台默认位置。打开对话框现在也支持指定目录，不能沿用“只有保存对话框支持”的旧说明。
- Linux：`src/platform/linux/platform.rs:prompt_for_paths` 向 portal 请求设置 `current_folder`，属于接口兼容修改，未作 Velora Linux 验收。

Windows 的 `prompt_for_paths` / `prompt_for_new_path` 另经 `run_file_dialog_in_thread` 放到专用 STA 线程：在线程内初始化 COM、复验 owner 窗口、运行对话框，返回普通路径数据后再释放 COM。`SendHwnd` 只跨线程携带句柄标识，不携带 GPUI 实体或 COM 接口。线程创建失败记录日志并关闭结果通道。

打开和保存对话框使用固定 client GUID 隔离旧的壳层状态；它只更换状态存储的 key，后续状态仍会重新积累。`Show` 的用户取消返回 `None`，其他错误向上传播。专用线程不能消除壳层访问不可达目录的等待，显式目录与线程隔离是两项不同措施。

### 11. Windows 调试构建内嵌着色器源码

`src/platform/windows/directx_renderer.rs:shader_resources::build_shader_blob` 在 `debug_assertions` 路径用 `include_str!` 内嵌 HLSL，再用 `D3DCompile` 从内存编译，避免运行时寻找编译机上的源码路径。

调用前就地展开 `alpha_correction.hlsl`；若仍有未处理的 `#include` 就报错，不退回磁盘。关闭 `debug_assertions` 时仍走既有的预编译着色器字节路径。本补丁没有修改 `build.rs`，也不是 macOS 的 `runtime_shaders` 特性实现。

### 12. macOS 标题栏标签与显式拖窗

`src/platform/mac/window.rs` 将内容视图的 `mouseDownCanMoveWindow` 设为 NO，避免拖动标签时 AppKit 同时拖动窗口。应用还需关闭窗口默认移动，并在标题或空白区域显式调用 `start_window_move`。

`MacWindow::start_window_move` 保留当次鼠标事件和原生窗口，在前台任务中延后执行 `performWindowDragWithEvent:`，仅在该调用期间临时允许移动，结束后恢复原值。延后是为了先归还 GPUI 借用，避开原生拖窗嵌套事件循环的重入。

### 13. 外部文件拖拽数据构造

`src/interactive.rs:ExternalPaths::new` 允许用本地路径迭代器构造原生文件拖放载荷，应用回归可直接驱动 `FileDropEvent`。补丁只增加构造入口，不负责复制、移动或打开文件；这些行为仍在应用层。

### 14. 测试观察接口

这些接口用于检验真实事件、布局和状态，不代表新增产品功能：

- `src/window.rs:Frame::clear` 逐帧清理 `debug_bounds` 与新增的 `debug_hovered`；`src/elements/div.rs` 绘制带 selector 的元素时记录悬停事实，`src/app/test_context.rs:VisualTestContext::debug_hovered` 提供查询。它们受 `test` / `test-support` 门控，防止已消失或被遮挡的元素仍让断言通过。
- `src/platform.rs:shape_line_with_platform_text_system` 仅在 macOS 且启用 `test` / `test-support` 时暴露真实 CoreText shaping，补充等宽 Noop 测试系统不能验证的字号与字形宽度。
- `src/platform/test/window.rs:TestWindow::get_title` 返回已设置的标题；`start_window_move` 记录请求次数；控件命中查询会先取出回调、释放锁再调用，避免回调重读窗口状态时死锁。
- `src/app/test_context.rs` 提供 `window_move_requests` 和 `hit_test_window_control`，验证标题栏空白区与溢出标签后的拖窗入口。

### 15. 布局泛型的数值类型标注

`src/taffy.rs:to_grid_repeat` 中的 `length(0.0_f32)` / `fr(1.0_f32)` 显式限定为 `f32`，保留 `repeat(..., minmax(0, 1fr))` 的布局语义，避免依赖 API 的泛型推导歧义。这项修改没有“本地补丁”标记，仍需在升级对比时核对。

## 源码文件覆盖表

下表逐项对应发布包差异；纯字段补齐和测试端修改也列入，避免升级时只重放显眼的修复。

| 源文件 | 补丁主题 |
| --- | --- |
| [src/app.rs](src/app.rs) | 焦点清理按释放事件触发 |
| [src/app/entity_map.rs](src/app/entity_map.rs) | `read_untracked` |
| [src/app/test_context.rs](src/app/test_context.rs) | 悬停、拖窗请求与控件命中查询 |
| [src/elements/div.rs](src/elements/div.rs) | 记录测试用悬停事实 |
| [src/elements/img.rs](src/elements/img.rs) | 固有尺寸缩放、百分比宽度的等比高度 |
| [src/interactive.rs](src/interactive.rs) | `ExternalPaths::new` |
| [src/platform.rs](src/platform.rs) | Noop 字号、真实 shaping 测试入口、目录与 HTML 字段、CF_HTML 导出 |
| [src/platform/cf_html.rs](src/platform/cf_html.rs)（新增） | CF_HTML 片段解析 |
| [src/platform/linux/platform.rs](src/platform/linux/platform.rs) | portal 起始目录 |
| [src/platform/linux/text_system.rs](src/platform/linux/text_system.rs) | 字号字段兼容，尚不处理覆盖值 |
| [src/platform/linux/wayland/clipboard.rs](src/platform/linux/wayland/clipboard.rs) | HTML 字段兼容 |
| [src/platform/mac/platform.rs](src/platform/mac/platform.rs) | 起始目录、HTML 剪贴板写入 |
| [src/platform/mac/text_system.rs](src/platform/mac/text_system.rs) | CoreText 逐段字号及配套单测 |
| [src/platform/mac/window.rs](src/platform/mac/window.rs) | 内容视图拖窗策略与显式原生拖窗 |
| [src/platform/test/window.rs](src/platform/test/window.rs) | 标题读取、拖窗记录、原生命中回调 |
| [src/platform/windows/clipboard.rs](src/platform/windows/clipboard.rs) | HTML 读取、载荷上限、锁定空指针处理 |
| [src/platform/windows/direct_write.rs](src/platform/windows/direct_write.rs) | DirectWrite 逐段字号 |
| [src/platform/windows/directx_renderer.rs](src/platform/windows/directx_renderer.rs) | 调试路径内嵌 HLSL 与 include 展开 |
| [src/platform/windows/events.rs](src/platform/windows/events.rs) | IME 按键归属与可选日志 |
| [src/platform/windows/platform.rs](src/platform/windows/platform.rs) | 前台任务预算、STA 选择器、起始目录与错误处理 |
| [src/style.rs](src/style.rs) | 默认文本 run 的字号字段 |
| [src/taffy.rs](src/taffy.rs) | 显式 `f32` 布局参数 |
| [src/text_system.rs](src/text_system.rs) | 字号传递、合并判据、视觉间距初始化 |
| [src/text_system/line.rs](src/text_system/line.rs) | 字号绘制、视觉间距、外部换行边界 |
| [src/text_system/line_layout.rs](src/text_system/line_layout.rs) | 字号缓存键、合法及紧急断点 |
| [src/text_system/line_wrapper.rs](src/text_system/line_wrapper.rs) | 共用行首/行末禁则、字号字段补齐 |
| [src/window.rs](src/window.rs) | 焦点释放标志、测试帧数据的建立和清理 |

此外还有上文说明的 `Cargo.toml` 目标裁剪和本 README 替换；`src/platform.rs` 的 `AtlasKey` 附近另有纯排版差异，不是独立功能补丁。

## 验证与升级

应用回归分布在 `src/components/block/element/tests.rs`（逐段字号、禁则、间距与命中）、`src/components/markdown/html_paste.rs`（CF_HTML）、`src/editor/tests/image_scale.rs` / `image_runtime.rs`（图片布局）、`src/editor/tests/workspace_shell.rs`（拖窗）、`src/editor/workspace/tests/tree_edit.rs`（原生拖放）等位置。GPUI 源码内的测试不等于都在应用门禁中执行。

升级时：

1. 用干净的发布包逐文件比较源码、`Cargo.toml` 和 `build.rs`，核对上表；先确认上游是否已包含等价修复，再决定保留或移除补丁。不要把旧 vendor 引入提交、当前上游主分支或“本地补丁”注释作为完整差异基线。
2. 新增字段要核对所有平台的结构体构造、run 合并与缓存键；保留 Linux 兼容字段不代表该平台已获得相同功能。
3. 在仓库根目录执行 `cargo build` 与 `cargo test --bin velora`，并沿用 [构建与测试约定](../../docs/architecture/testing-and-build.md) 运行确定性慢用例和大文档诊断。`cargo test -p gpui` 不是本仓门禁：此目录不是 workspace 成员，且示例与独立测试目标已经裁剪。
4. 核对 macOS ARM64、macOS Intel、Windows x64 原生 CI；在对应系统验证 IME、连续打开/取消文件选择器、剪贴板 HTML、标签拖动与标题栏拖窗。一次 macOS 构建不能验证 Windows 原生行为。
5. 同步更新本文、文件覆盖表及架构文档索引，区分仍存在、已被上游替代和已撤销的修改。
