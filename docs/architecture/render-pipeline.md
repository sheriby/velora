# Render Pipeline（渲染管线）

> 面向后续 agent 的代码导览。写作于 2026-09-28，基于 `perf` 分支。
> **行号会漂移，函数名不会**。相关文档：[overview.md](./overview.md)、[editor-core.md](./editor-core.md)、[workspace-ui.md](./workspace-ui.md)、[testing-and-build.md](./testing-and-build.md)

## 1. 每帧入口与滚动容器（src/editor/render/paint.rs `impl Render for Editor`；行计划/菜单几何类型在 render.rs，标题栏与滚动同步在 render/sync.rs）

- 每帧：关闭守卫/外观观察者 → 挂起焦点/滚动入视 → `sync_scroll_viewport` + 大纲跟随滚动 → clone 主题并按会话缩放套用全部字号。
- **滚动是手动列表 + 原生滚动 div，不是 uniform_list**：`div().id("editor-scroll-inner").overflow_y_scroll().track_scroll(&self.scroll_handle)`；`scroll_handle.offset().y`（负值）为滚动位置，翻页/跳转经 `set_offset`（src/editor/events/scroll_mouse.rs）。自定义滚动条覆盖层 + canvas 鼠标事件（几何在 `scrollbar_geometry`，src/editor/window_state.rs）。
- 可见块 = `document.visible_blocks()`（缓存的 DFS `VisibleTreeSnapshot`，结构变更时 `rebuild_metadata_and_snapshot` 重建）+ `apply_heading_fold_filter`（折叠标题过滤，同时填充 foldable/[TOC]）。
- 行分组：callout 组（`callout_anchor`）/脚注组（`footnote_anchor`）合并为 `RowElement::{Group,Ordinary}`；行间距元数据按行 `read_with` 现读，不整帧缓存 Vec。

## 2. 视口虚拟化（已存在，测试 tests.rs 渲染窗口裁剪组）

- 状态在 Editor：`row_stride_cache: HashMap<EntityId, f32>`（每行 footprint，键=行首块）、`prev_mounted_run: Option<MountedRun>`、`prev_visible_block_ids`；结构体 `RenderWindow`/`MountedRun`/`FocusIsland`（src/editor/mod.rs）。
- **stride 学习**：每帧从滚动容器 `scroll_handle.bounds_for_item(child)` 与上帧 diff 得出；列宽变化清缓存；>2×活行时剪枝；未测行用 `d.block_min_height` 下界估计。
- **窗口选取**：`rendered_window(&strides, scroll_y, viewport_h, RENDER_OVERDRAW_PX=800, focus_row)`（window_state.rs）——跑动和带状扫描取与 `[scroll_y-800, scroll_y+viewport+800]` 相交的连续行段；估计不足时回退尾段。**焦点行恒挂载**为独立 focus island（自带前导 spacer）。
- 屏外行 → `div().h(px(...))` spacer（丢掉行自身 mt 间距）；只有窗口内 + focus island 的行创建真实元素。

## 3. 块渲染与 BlockTextElement（src/components/block/）

- `impl Render for Block`：同步图片焦点态与行内投影（`sync_inline_projection_for_focus`）→ 光标闪烁启停 → 按 kind 分派：表格单元格 / source-raw（纯 `BlockTextElement` + 可选行号槽）/ 独立图片 / 标题(带折叠 chevron) / 列表(标记符+复选框) / CodeBlock(面板+复制按钮+`CodeLanguageInputElement`) / Table / HtmlBlock / Math+Mermaid(未聚焦渲染 SVG，聚焦退回文本) / TOC / 默认文本 → `wrap_with_quote_guides`。
- 公共壳 `render_shell`：`Stateful<Div>` + `key_context("BlockEditor")` + track_focus + 全部动作 + 鼠标处理。
- **`BlockTextElement`**（src/components/block/element/text_element.rs，自定义 `Element`；类型定义在 element.rs）：
  - `request_layout`：读块 → `shared_display_text()`（Arc SharedString）→ `build_text_runs` / `build_code_text_runs` → `window.text_system().shape_text(...)`（`request_measured_layout` 内）→ `Vec<WrappedLine>` 存 `Rc<RefCell>`。
  - `prepaint`：光标 quad、选区 quad、行内代码圆角背景、搜索高亮、行号 ShapedLine、hitbox。
  - `paint`：画 quad/行号/文本行；聚焦时 `window.handle_input` 注册 IME；⌘ 悬停链接变手型；**把 `last_layout`/`last_bounds`/`last_line_height` 写回 Block**——这是命中测试的布局缓存。
  - `build_text_runs`：行内 span 边界 + IME marked 合并排序；逐 run 应用代码字体/逐 run `font_size`（vendored 补丁）/粗斜体/链接下划线/删除线等。
- **关键性能事实：`shape_text` 每帧对每个挂载块重跑，无跨帧 memo**（GPUI 自身缓存之外）。滚动时静态块也在反复 shaping。表格列宽测量（`TableColumnLayout::measure` → 每单元格 `shape_text` no-wrap）**同样每帧重测不缓存**。

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

- `ViewMode::Source` / 代码文件：全文档单块，`EditMode::SourceRaw` 渲染纯 `BlockTextElement` + 行号槽，无行内样式。
- `ViewMode::Rendered`：行内样式 + 投影定界符（编辑中）+ 图片/数学/mermaid/HTML 块/表格/callout/脚注。
- **长块护栏**：未聚焦且超 `LONG_BLOCK_SOURCE_LIMIT` 的块按纯 div 渲染、跳过 span 布局（`render_text_or_mixed_inline_visuals`）。
- 未聚焦含混合视觉（数学/上标/行内图）的块按 `flex_wrap` 词段 div 渲染（`render_inline_text_word_segments`）而非 BlockTextElement。

## 6. 每帧文档级开销清单（优化候选，按嫌疑排序）

1. `shape_text` 每帧重 shape 所有挂载块（element.rs request_layout）。
2. 表格列宽每帧重测所有单元格（markdown/table.rs `measure_preferred_column_widths`）。
3. `visible_blocks().to_vec()` 全量克隆 + 折叠过滤每帧扫描（src/editor/render/paint.rs render 开头）。
4. 主题 clone + 全字号缩放套用每帧执行。
5. 行间距 `RenderedRowSpacingInfo::from_block` 每行 `read_with`（相对小）。
