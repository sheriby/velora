# Render Pipeline：渲染管线

> 基于 `dev`，2026-10-14 重写。引用用 `文件:函数名`。
> 相关：[overview.md](./overview.md) · [editor-core.md](./editor-core.md) · [markdown-model.md](./markdown-model.md) · [performance.md](./performance.md)

## 1. 每帧入口

`impl Render for Editor` 在 `src/editor/render/paint.rs`（本仓少数几个允许超长的单函数之一）。行计划与菜单几何的类型在 `render.rs`，标题栏/焦点/滚动同步在 `render/sync.rs`，窗口内菜单在 `render/menu_render.rs`，欢迎页与浮层在 `render/overlays.rs`。

每帧顺序（简化）：关闭守卫与外观观察者 → 挂起的焦点/滚动入视 → `sync_scroll_viewport` + 大纲跟随滚动 → 派生本帧主题（共享，只在度量输入变化时失效）→ 侧栏与标签条 → 正文滚动区 → 浮层。

**滚动是手动列表 + 原生滚动 div，不是 `uniform_list`**：`div().id("editor-scroll-inner").overflow_y_scroll().track_scroll(&scroll_handle)`；`scroll_handle.offset().y`（负值）是滚动位置，翻页与跳转经 `set_offset`（`events/scroll_mouse.rs`）。滚动条是自绘覆盖层 + canvas 鼠标事件（几何在 `window_state.rs:scrollbar_geometry`）。

**切视图模式要保住阅读位置**：切换前记下视口顶部那个块的**缓冲区字节锚点**（`window_state.rs:capture_viewport_source_anchor` → `ViewModeScrollAnchor`），切换后按锚点重算滚动偏移并在若干帧内收敛（`apply_pending_view_mode_scroll`，帧数有上限）；任何换现场的动作要先 `cancel_pending_view_mode_scroll`，否则挂起的锚点会把用户已经滚到的位置拽回去。两种模式的行高与块划分不同，所以锚点必须记在缓冲区坐标上，记滚动偏移或行下标都跨不过去。

## 2. 可见列表与行计划

- 可见块 = `DocumentTree::visible_blocks()`（缓存的 DFS 快照 `VisibleTreeSnapshot`，结构变更时 `rebuild_metadata_and_snapshot` 重建）+ `apply_heading_fold_filter`（折叠过滤，同时填 `foldable` 与 `[TOC]` 条目）。
- 快照里有**两块对齐的 vec**：`visible`（可见序 + entity→索引）与 `row_spacing`（分组锚点、层级、列表标记这些**结构量**）。它们由 `sync_block_list` 同一趟算出，目的是让行计划重建**不必逐块读实体**（大文档上"读几个块实体"就是随文档增长的系数）。折叠过滤只剩标题需要碰实体。
- 行分组：callout 组 / 脚注组合并为 `RowElement::{Group, Ordinary}`；行计划按**结构版本**缓存（`row_meta_version`，只在可见列表重排、增量追加、`is_toc` 翻面时动），**不按文档修订缓存**——否则正文里打字会白白重排整份计划。
- `[TOC]` 是**文本形状**，而整棵同步只在结构变化时重跑：文本一变就当场刷新那一条（`DocumentTree::refresh_row_spacing_for`），折叠过滤写完条目之后再刷一次。守卫 `typing_a_toc_marker_fills_its_entries_on_that_frame`。

## 3. 视口虚拟化

- 状态在 Editor：`row_stride_cache`（每行 footprint，键 = 行首块）、上一帧挂载集合 `prev_mounted_run`、`prev_visible_block_ids`；类型 `RenderWindow` / `MountedRun` / `FocusIsland`（`src/editor/mod.rs`）。
- **stride 学习**：从仍可寻址的上帧挂载集合读 `scroll_handle.bounds_for_item(child)` 回收真实高度；列宽或派生主题度量变化时清缓存；未测行用块最小高度做下界估计。
- **窗口选取**：`window_state.rs:rendered_window_from_prefix`——用行高**前缀索引**二分定位与 `[scroll_y - overdraw, scroll_y + viewport + overdraw]` 相交的连续行段；估计不足时回退尾段。**焦点行恒挂载**为独立 focus island（自带前导 spacer）。稳定帧不扫描全文行高。
- 屏外行渲染成等高 spacer（丢掉行自身外边距）；只有窗口内 + focus island 的行创建真实元素。
- 冷启动补帧有预算上限（单帧预挂载行数、续挂帧数），不依赖正文光标闪烁去驱动。

## 4. 块渲染分派（src/components/block/render/）

`impl Render for Block` 在 `render/paint_parts.rs`。先命中先返回：

1. `renders_nothing()`（空 HTML 文档）→ 空 div。
2. 每帧公共副作用：同步图片焦点态 → `sync_inline_projection_for_focus` → 按 `input_active`（**焦点 ∧ 窗口活动**）启停光标闪烁任务。
3. **表格单元格** → 格子外观 + 内容。
4. **source-raw 早退**：源码视图/代码文档/raw 类块未聚焦 → 纯 `BlockTextElement`（可选行号槽、等宽字体）。聚焦的数学块放行到 kind 分支（ƒx 按钮挂那里）。
5. 空段落未聚焦 → 只留块间距高度的壳（`collapses_to_blank_gap`，影响 stride 估计）。
6. **按 kind 分派**：分隔线 / 标题（带折叠 chevron）/ 三种列表项（标记列 + 内容）/ 引用 / callout 头行 / 脚注定义（徽章 + 回跳）/ 代码块面板（红绿灯、复制按钮、语言输入框）/ frontmatter（键值卡片，聚焦退 YAML 源码）/ 表格网格 / HTML 块递归渲染 / 数学与 mermaid（未聚焦画 SVG，聚焦退回文本编辑）/ `[TOC]` 目录 / 兜底文本。
7. 出口统一 `wrap_with_quote_guides`（引用竖线）。

数学块的预览与编辑都走统一出口，避免预览提前返回时丢掉引用竖线。块公式在右侧按文档顺序显示 `(1)`、`(2)` 编号，引用中的公式参与排序，行内公式不参与；序号由块树快照维护，结构变更与流式续建时更新，不写入 Markdown。编辑卡片上方显示固定高度、可滚动的实时公式预览，与阅读态共用分段解析和 SVG 缓存。未知命令由 RaTeX 报告后按红色字面量参与原公式排版，不改源码；其他未完成语法沿用上一次成功结果，没有成功结果则保留空白占位，不插入报错条或改变源码区位置。公式块内单独输入 `{` 自动补齐 `}`，已有闭括号可跳过，空括号对可一起退格；转义、粘贴和显式范围替换保留原文。预览不记录输入几何，点击预览不移动源码光标；失焦后恢复正常公式展示与错误提示。没有正文、只承载子块的空引用容器在未聚焦时为零高；聚焦后恢复可编辑行，文档结构与源码不变。

兜底那一路 `render_text_or_mixed_inline_visuals`（`render/inline_visuals.rs`）再分三档：未聚焦且超长块 → 纯 div 源码文本（**长块护栏**）；聚焦或无混合视觉 → `BlockTextElement`；未聚焦但含混合视觉（行内公式/上下标/行内图片）→ flex_wrap 词段 div + `block_bounds_recorder`。

公共壳 `render/shell.rs:render_shell` 挂 `key_context("BlockEditor")`、`track_focus`、全部块级动作与鼠标处理。

**`EditMode` 三档**（`runtime/mod.rs:EditMode::for_kind`）：`RenderedRich`（属性树编辑，唯一支持投影）、`SourceRaw`（raw 字节编辑：RawMarkdown/Comment/HtmlBlock/MathBlock/MermaidBlock 与整体源码视图）、`CodeBlockRaw`（代码块与 frontmatter；frontmatter 归 SourceRaw 会绕过代码外观）。

## 5. BlockTextElement 与 shape 备忘

自定义 `Element`，类型定义在 `block/element/code_input.rs`，实现在 `block/element/text_element.rs`；布局与几何工具函数在 `block/element.rs`。

- **request_layout**：取显示文本与换行/字距开关 → 算备忘键 → 在测量闭包里：wrap 宽过小（min-content 测量）直接返回估算下界不 shape；**键命中就复用行表，连 runs 都不建**；未命中才 `build_text_runs`（行内 span + IME marked 合并边界）或 `build_code_text_runs`（高亮 span → 颜色/字形），然后 `window.text_system().shape_text`，再补字距与中西文 autospace，写回备忘。有长行折叠计划时按硬行逐行 shape（折叠行不换行、截断加提示）。
- **prepaint**：插 hitbox；算光标 quad（含闪烁透明度、当前行高亮、括号配对底色）、选区 quad、行内代码圆角背景、搜索高亮、行号。
- **paint**：链接悬停手型；聚焦时 `window.handle_input` 注册 IME；按层序绘制；**末尾把 `last_layout` / `last_bounds` / `last_line_height` / 行号槽宽写回块**——这是命中测试、光标可见性、滚动定位、stride 学习共用的布局缓存。
- **`ShapeMemoKey`**（`runtime/mod.rs`）：文本代数、span 结构代数、marked 区间、高亮代数、换行宽、换行/字距开关、字号、字体指纹、主题指纹（取 Arc 指针）、长行折叠代数。任一不符即重 shape；文本变、spans 变、折叠切换还会主动置 None。**高亮代数单独进键**：源码视图的围栏级联换色时块文本没变，不递增就上不了一屏。
- 软换行的绘制、光标、选区、点击与上下键定位共用 `element.rs:aligned_row_left`（每个视觉行单独算居中留白）。

## 6. 块级缓存与失效

失效枢纽是 `runtime/display_cache.rs:sync_render_cache`：重建行内渲染缓存 → 同步代码高亮 → 同步图片运行时 → 清/重建投影 → 刷新共享显示文本。**所有文本替换路径与 `with_record` 都调它，是编辑后即时重建，不是按帧懒重建。**

| 缓存 | 失效时机 |
|---|---|
| `render_cache`（可见文本 + spans + 链接/脚注命中） | 每次文本变更 |
| `cached_display_text` / `display_generation` | 内容真变了才换 Arc 并递增代数、清 shape 备忘 |
| `projection`（定界符显形） | 焦点/选区/IME 变化；按 `projection_cache_key` 短路光标闪烁帧；失焦与非 RenderedRich 清除 |
| `code_highlight` / `highlight_generation` | 每次文本变更、语言字段变更、源码视图接缝级联；结果变化递增代数 |
| `shape_memo` | 见 §5 |
| `hard_line_ranges_cache` | 按显示代数 |
| `last_layout` / `last_bounds` | 每次 paint 写回；数学与 mermaid 未聚焦时主动置 None |
| `column_layout_memo`（表格列宽） | 键含主题指纹、字号、容器宽、字体、表内容版本 |
| `table_runtime` / `image_runtime` | 文档级重建（`rebuild_table_runtimes` / `rebuild_image_runtimes`，后者只对引用敏感块增量） |

**定界符投影**（`runtime/projection.rs` + `projection/build.rs`）：把光标/选区/IME **触碰到的**行内构造的定界符显形进显示文本，并产出双向偏移表。三套坐标系要分清：**clean**（`record.title` 可见文本）、**current/display**（投影后的显示文本，`selected_range` 与 `marked_range` 都在这一系）、**markdown**（序列化源码）。块内偏移一律是 UTF-8 字节；只有 IME 边界是 UTF-16（`block/input.rs` 换算）。

`config.toml [editor] hide_markdown_markers`（偏好设置「隐藏 Markdown 标记」，默认关闭）禁用富文本行内定界符投影：粗斜体、删除线、高亮、代码、上下标、链接与脚注在聚焦、选中及输入形成样式时仍保持渲染文本。判据统一在 `Block::supports_inline_projection`；块初始化读取运行时设置，每帧同步设置变化，切换时用既有投影映射保留选区方向与 IME 范围。仅改变显示，不修改缓冲区或序列化；源码视图及公式、图表、代码块等独立源码编辑区域沿用现有方式。

**源码视图的 markdown 高亮**：每块记 `source_fence_entry/exit`（围栏/数学/frontmatter/注释状态），编辑器在整树重建后 `resync_source_fence_states` 全量串联，打字后 `cascade_source_fence_states_after` 增量级联（下一块入口一致就停）。行号槽宽按全文档总行数统一（`source_line_gutter_basis`），分块右对齐。

## 7. 帧几何与命中测试

- `MountedRun` 带文档身份与行计划版本，统一枚举主窗口与焦点岛；命中测试只访问当前计划的真实成员（`bounds_for_item` 对旧帧成员返回 None）。表格以当前格子边界命中，不消费屏外旧坐标。
- **断言几何之前先重绘**：命中测试读的是上一帧写下的 `last_bounds`/`last_layout`，没画过就是 0。
- `last_bounds`/`last_layout` 为空的三种情况：从未 paint；数学/mermaid 未聚焦；只画图片或混合视觉的块（`block_bounds_recorder` 只写 bounds，判据是 `has_text_layout()`）——这类块的命中测试只能整块粒度。表格块自己没有文本元素（可见长度 0），所以跨块落点换算把"上一块底与下一块顶之间"那段空间归给中间量不出文本布局的块。
- `render/sync.rs:scroll_target_bounds` 统一文字范围与原子壳（公式/图片）几何：搜索跳转可以用原子边界，普通光标跟随等待实际光标；所有等待分支共用重试预算。
- 滚动请求带落点口径 `ScrollPlacement`：`IntoView` 只做最小可见调整（初始加载、普通聚焦），`Reveal` 是用户跳转（目标未挂载时按行序号估算接近，一次都没挪过视口就当场收请求），`Top`/`Bottom`/`Center` 是绝对对齐并按偏差收敛。`ensure_focused_caret_visible` 返回「拿到几何 / 这一帧真的挪过视口」，只有稳定帧才清请求；`IntoView` 的校验帧不因移动续期，否则用户手动滚走后会被一直拽回活动块。

## 8. 陷阱

- 没有文本元素的 kind/状态：分隔线、未聚焦的表格与 HTML/数学/mermaid、`[TOC]` 段落、`renders_nothing` 的 HTML、未聚焦的混合视觉块、长块护栏降级的 div。给它们做命中测试或选区之前先确认。
- 长块与长行护栏常量在 `src/components/mod.rs`（未聚焦长块降级阈值、长行折叠阈值与折叠态截断长度）。长行折叠只对带行号槽的代码块/段落启用。
- run 的 `len` 以字节计且必须覆盖全文，否则 `shape_text` 丢弃无 run 覆盖的尾部（截断提示行要复制行尾 run 盖住 marker 字节）。
- 光标闪烁必须要求 `window.is_window_active()`：gpui 的 `is_focused` 判不出窗口切走，否则后台窗口持续闪烁导致全块重绘。
- `.id()` 之后元素类型变 Stateful，if/else 两分支要 `into_any_element` 统一；svg 不继承父 div 的 text_color；`TextRun` 逐段字号是本地补丁（Linux 未接）。
- 无窗口测试的文本系统是等宽模拟（`NoopTextSystem`）：只断言结构与偏移，别断言像素宽度与字形。要真实宽度就在窗口里用 `window.text_system().shape_text`。

## 9. 测试落点

`editor/tests/`（滚动条几何与渲染窗口裁剪、`scroll_window.rs`、长行、`image_runtime.rs`、`latex_tools.rs`）、`block/element/tests.rs`（几何与排版、备忘命中、硬行表）、`block/render/tests.rs`（外观与列宽备忘）、`block/runtime/tests/`（投影、IME、代码语言、图片、链接与选区）、`workspace/tests/sidebar.rs`（面板开窗）。
