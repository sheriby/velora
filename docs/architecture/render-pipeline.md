# Render Pipeline（渲染管线）

> 面向后续 agent 的代码导览。写作于 2026-09-28，基于 `perf` 分支；2026-10-13 对照核心修复复核。
> **行号会漂移，函数名不会**。相关文档：[overview.md](./overview.md)、[editor-core.md](./editor-core.md)、[workspace-ui.md](./workspace-ui.md)、[testing-and-build.md](./testing-and-build.md)

## 1. 每帧入口与滚动容器（src/editor/render/paint.rs `impl Render for Editor`；行计划/菜单几何类型在 render.rs，标题栏与滚动同步在 render/sync.rs）

- 每帧：关闭守卫/外观观察者 → 挂起焦点/滚动入视 → `sync_scroll_viewport` + 大纲跟随滚动 → 复用按主题与会话缩放派生的共享主题。
- **滚动是手动列表 + 原生滚动 div，不是 uniform_list**：`div().id("editor-scroll-inner").overflow_y_scroll().track_scroll(&self.scroll_handle)`；`scroll_handle.offset().y`（负值）为滚动位置，翻页/跳转经 `set_offset`（src/editor/events/scroll_mouse.rs）。自定义滚动条覆盖层 + canvas 鼠标事件（几何在 `scrollbar_geometry`，src/editor/window_state.rs）。
- 可见块 = `document.visible_blocks()`（缓存的 DFS `VisibleTreeSnapshot`，结构变更时 `rebuild_metadata_and_snapshot` 重建）+ `apply_heading_fold_filter`（折叠标题过滤，同时填充 foldable/[TOC]）。
- 行分组：callout 组（`callout_anchor`）/脚注组（`footnote_anchor`）合并为 `RowElement::{Group,Ordinary}`；行计划按结构版本缓存分组成员与间距，测量高度独立维护。

## 2. 视口虚拟化（已存在，测试 tests.rs 渲染窗口裁剪组）

- 状态在 Editor：`row_stride_cache: HashMap<EntityId, f32>`（每行 footprint，键=行首块）、`prev_mounted_run: Option<MountedRun>`、`prev_visible_block_ids`；结构体 `RenderWindow`/`MountedRun`/`FocusIsland`（src/editor/mod.rs）。
- **stride 学习**：从仍可寻址的上帧挂载集合读取 `scroll_handle.bounds_for_item(child)`，主窗口与焦点岛都回收高度；列宽或派生主题度量变化清缓存；>2×活行时剪枝；未测行用 `d.block_min_height` 下界估计。
- **窗口选取**：`rendered_window_from_prefix`（window_state.rs）——通过行高前缀索引二分定位与 `[scroll_y-800, scroll_y+viewport+800]` 相交的连续行段；估计不足时回退尾段。**焦点行恒挂载**为独立 focus island（自带前导 spacer）。
- 屏外行 → `div().h(px(...))` spacer（丢掉行自身 mt 间距）；只有窗口内 + focus island 的行创建真实元素。

## 3. 块渲染与 BlockTextElement（src/components/block/）

- `impl Render for Block`：同步图片焦点态与行内投影（`sync_inline_projection_for_focus`）→ 光标闪烁启停 → 按 kind 分派：表格单元格 / source-raw（纯 `BlockTextElement` + 可选行号槽）/ 独立图片 / 标题(带折叠 chevron) / 列表(标记符+复选框) / CodeBlock(面板+复制按钮+`CodeLanguageInputElement`) / Table / HtmlBlock / Math+Mermaid(未聚焦渲染 SVG，聚焦退回文本) / TOC / 默认文本 → `wrap_with_quote_guides`。
- 公共壳 `render_shell`：`Stateful<Div>` + `key_context("BlockEditor")` + track_focus + 全部动作 + 鼠标处理。
- 公共排版：H1/H2 居中、H3 左对齐，普通文本与混合公式/图片路径共用对齐规则；无序、有序、任务列表共用标记列宽与首行对齐，列表项不再叠加上下内边距。正文粗体沿用主题的 `text_link` 强调色，标题保留自身文字色。软换行的绘制、光标、选区、点击与上下键定位共用 `element.rs:aligned_row_left`，每个视觉行单独计算居中留白。
- **`BlockTextElement`**（src/components/block/element/text_element.rs，自定义 `Element`；类型定义在 element.rs）：
  - `request_layout`：读块 → `shared_display_text()`（Arc SharedString）→ `build_text_runs` / `build_code_text_runs` → `window.text_system().shape_text(...)`（`request_measured_layout` 内）→ `Vec<WrappedLine>` 存 `Rc<RefCell>`。
  - `prepaint`：光标 quad、选区 quad、行内代码圆角背景、搜索高亮、行号 ShapedLine、hitbox。
  - `paint`：画 quad/行号/文本行；聚焦时 `window.handle_input` 注册 IME；⌘ 悬停链接变手型；**把 `last_layout`/`last_bounds`/`last_line_height` 写回 Block**——这是命中测试的布局缓存。
  - `build_text_runs`：行内 span 边界 + IME marked 合并排序；逐 run 应用代码字体/逐 run `font_size`（vendored 补丁）/粗斜体/链接下划线/删除线等。
- **`shape_text` 有跨帧 memo**（2026-09-28 P3，`ShapeMemoKey` 在 src/components/block/runtime/mod.rs）：键含文本代数、高亮代数、换行宽与 prose 开关、字号、字体指纹、主题指纹、长行展开代数，命中直接复用 `Vec<WrappedLine>`；打字的那一块、改字号、换主题、高亮换代才重新 `shape_text`。组合标记范围和实际样式变化也进入排版签名。表格列宽测量按内容版本、字体与字号缓存，稳定帧复用。

## 4. 块级运行时缓存（src/components/block/runtime/）

失效枢纽 = `sync_render_cache`（runtime/mod.rs）：重建 `InlineRenderCache`、`sync_code_highlight`、`sync_image_runtime`、清/重建投影、刷新 `cached_display_text`。所有文本替换路径与 `with_record` 都会调用（**编辑后即时重建，非按帧懒重建**）。

| 缓存 | 位置 | 失效时机 |
|---|---|---|
| `render_cache: InlineRenderCache` | Block 字段 | 每次文本变更（sync_render_cache） |
| `projection: ExpandedInlineProjection` | Block 字段 | 焦点/选区/IME 变化；键 `projection_cache_key=(supports_projection, selected, marked)` 短路光标闪烁帧；blur/原始模式清除 |
| `code_highlight: CodeHighlightResult` | Block 字段 | 每次文本变更；tree-sitter 配置进程级 `LazyLock` 注册表共享，但**每块每次高亮无跨帧结果缓存** |
| `cached_display_text: SharedString` | Block 字段 | sync_render_cache（Arc 共享，帧间零拷贝） |
| `last_layout`/`last_bounds` | Block 字段 | 每次 paint 写回；命中测试/光标可见性/stride 学习复用；Math/Mermaid/HTML 未聚焦时丢弃 |
| `table_runtime` | Block 字段 | `rebuild_table_runtimes` 文档级重建（含单元格块实体） |
| `image_runtime` | Block 字段 | sync_render_cache + `set_runtime_context`（基目录/引用定义变化）；`rebuild_image_runtimes` 文档级（只对引用敏感块增量，见 runtime_context.rs） |

## 5. Markdown vs 纯文本渲染差异

- **源码视图的 markdown 语法高亮**（2026-10-07）：源码分块（Paragraph +
  `source_language == "markdown"`）的高亮走手写逐行扫描器
  （src/components/markdown/source_highlight.rs），不走 tree-sitter——后者
  逐块解析拿不到跨 512 行接缝的围栏状态，捕获名也与主题色对不上。着色数据
  是 `CodeHighlightSpan`（字节区间 + `CodeHighlightClass`），build 时经
  `sync_code_highlight` 产出、build_text_runs 系的 `build_code_text_runs`
  消费成 TextRun。跨块接缝：每块记 `source_fence_entry/exit`
  （MarkdownSourceState：围栏/公式/frontmatter/HTML 注释），编辑器在整棵
  重建时全量重串、打字后增量级联；高亮结果变化会递增
  `highlight_generation`（进 shape 备忘键）。颜色取主题 `md_syntax_*` 七
  字段，语义对齐 VS Code。

- `ViewMode::Source` / 代码文件：按最多 512 行分块，`EditMode::SourceRaw` 渲染纯 `BlockTextElement` + 行号槽，无行内样式。
- `ViewMode::Rendered`：行内样式 + 投影定界符（编辑中）+ 图片/数学/mermaid/HTML 块/表格/callout/脚注。
- **长块护栏**：未聚焦且超 `LONG_BLOCK_SOURCE_LIMIT` 的块按纯 div 渲染、跳过 span 布局（`render_text_or_mixed_inline_visuals`）。
- 未聚焦含混合视觉（数学/上标/行内图）的块按 `flex_wrap` 词段 div 渲染（`render_inline_text_word_segments`）而非 BlockTextElement。

## 6. 当前帧几何与成本边界

- `MountedRun` 带文档身份与行计划版本，统一枚举主窗口和焦点岛；命中测试只访问当前计划的真实成员。表格以当前格子边界命中，不消费屏外旧坐标。
- 逻辑选择保存干净文本范围，当前显示范围最后由投影派生。公式等原子格的整格端点在起拖时确定，拖回同格仍保留整格语义。
- `RenderedRowPlan:set_stride` / `fill_strides` 统一高度写入，变化只作废对应前缀后缀。窗口高度使用非负权重，滚动映射使用最小为 1 的权重；稳定帧不扫描全文。
- 当前挂载行尚未测量或上一有效帧主窗口未填满视口时，冷启动补帧沿用有限预算；不依赖正文光标闪烁。
- `src/editor/render/sync.rs:scroll_target_bounds` 统一文字范围与原子壳几何。搜索跳转可以使用原子边界，普通光标跟随等待实际光标；所有等待分支共用重试预算。
- `shape_text`、表格列宽与派生主题都有跨帧复用；结构变化仍可能重建行计划，不宣称所有编辑路径均与全文规模无关。
