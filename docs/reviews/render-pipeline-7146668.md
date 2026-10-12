# Velora 渲染管线补充评审

> 本份保留为渲染补审记录。统一结论与跨系统重构边界见 [编辑器核心统一评审](editor-unified-7146668.md)。

- 固定点：`7146668760031fb0dc14cccd8734e74c564c2791`，当前 `dev`。
- 范围：Editor 行计划与虚拟化 → Block render → 文字 request_layout / prepaint / paint → 几何回写与滚动调度。
- 用户要求直接读当前分支；未创建 worktree、未修改产品代码、未提交。
- 与 [核心评审](editor-core-7146668.md) 的关系：本报告补查渲染自身，不重复把旧几何命中、显示选区失效列为新发现。
- 验证：主线程复核实际输入、缓存键、失效入口和重试守卫；以下新发现为静态确认，尚未逐条做 GUI 回归复现。不能据此宣称渲染器所有分支已审完。

## 判断

**渲染 pipeline 本身存在需要重构的契约缺口。** 不仅是选区代码消费几何有误：排版缓存没有完整表达样式输入，块内排版与文档量高/列宽的失效条件不同，独立焦点行没有加入量高回收，等待布局的滚动任务也没有完整的终止条件。

现有文字 shaping memo、表格列宽 memo 和公式缓存均存在并有效覆盖部分稳定路径，宜保留。重构重点是统一输入签名、布局结果生命周期和调度状态，而不是废弃缓存或重写绘制器。

## 维度一 · 不变量与 GPUI 契约

### R-1【必修】文字排版缓存只跟踪显示文本，没有完整跟踪 runs

**位置：** `src/components/block/element/text_element.rs:Element::request_layout`；`src/components/block/runtime/mod.rs:refresh_cached_display_text`；`src/components/block/runtime/display_cache.rs:sync_render_cache`；`src/components/block/element.rs:build_text_runs`、`build_code_text_runs`。

`ShapeMemoKey` 包含显示文本代数、代码高亮代数、字体、字号、宽度和主题等；没有行内 span/style 代数，也没有 IME `marked_range`。`refresh_cached_display_text` 仅在显示字符串不同的情况下清 shape memo。

然而 `build_text_runs` 把粗体、斜体、装饰和 IME 标记下划线写入 runs，这些都是缓存结果的一部分，不是单纯在 paint 动态覆盖。

**触发：**

- 普通段落预编辑“中”已经绘制，再提交同一个“中”：文本相同、组合标记已清，但可继续命中包含旧组合下划线的排版缓存。
- 跨块格式命令改变未聚焦段落的粗体/下划线：文字没有变化，样式缓存已更新，shape memo 却仍可能复用旧 runs，视觉格式不及时生效。

**守卫核查：** `highlight_generation` 对应代码高亮，不覆盖普通行内样式；`sync_render_cache` 重建 render cache 不等于清 shape memo。焦点块显形可能改变字符串从而偶然失效，但不能保护未聚焦块或普通 IME 文本。

**修法：** 为实际 runs 输入建立代数/签名，覆盖行内样式与组合态；文字代数只表示文字变化。布局、paint 和命中共用带签名的排版结果。

**置信度：** high；静态确认，未使用真实输入法实测残留下划线。

### R-2【必修】排版、列宽和文档量高采用不同的失效条件

**位置：** `src/editor/render/paint.rs:Editor::render`；`src/components/block/render/parts.rs:cached_table_column_layout`；`src/components/markdown/table.rs:TableColumnLayout::measure`、`measure_cell_preferred_width`；`src/config/preferences/window.rs:apply_saved_preferences`。

两处遗漏已确认：

1. **行高与 spacer 缓存：** 只在 `centered_width` 或 `effective_text_size` 变化时清空。代码字号、正文/代码字体、主题行高和内边距等同样影响实际高度，却没有进入量高签名。先滚遍多屏代码块，让高度被学习，再将代码字号从 14 改为 28：文字 shape 会重排，屏外 `row_stride_cache` / `plan.strides` 仍使用旧高度，直到逐屏挂载重新学习。
2. **表格列宽缓存：** 指纹包含字号、字距、padding、表头开关、内容和宽度，没有字体家族。测量实际使用 `window.text_style().font()` 和 `fonts.code_family`。仅更换字体、字号与宽度不变时，格子 shape 的字体指纹使其重排，而列宽 memo 命中旧字体结果，造成额外换行或错误宽度分配。

**守卫核查：** 偏好应用刷新窗口，没有专门清上述缓存或重建所有表格；列宽指纹是标量组合，派生主题 Arc 变化不会间接使其失效。正文字号缩放已有清 stride 的路径，但不能覆盖仅改代码字号/字体。

**修法：** 建立共享排版签名，明确哪些输入影响文字形状、列宽、行高和 spacer；几种派生缓存按各自实际依赖取签名，确保字体和主题度量变化贯穿到屏外占位。不要只向几个缓存各补一个独立字段而继续各自维护口径。

**置信度：** high；依赖与漏失效入口静态确认，字体切换后的实际像素差未实测。

## 维度二 · 正确性、边界与调度

### R-3【必修】独立焦点行被渲染，却不参与量高回收

**位置：** `src/editor/render/paint.rs:Editor::render`；`src/editor/window_state.rs:rendered_window`；`src/editor/render/menu_render.rs:zoom_by`。

屏外焦点行会作为 focus island 单独挂载，但下一帧只回读 `prev_mounted_run` 的连续主窗口高度；`MountedRun` 没有记录 island 的行号/子元素位置，没有另一处学习其真实高度。

`rendered_window` 根据缓存 stride 扣除 island 的估计高度，再创建真实 island 元素。若估计高度与实际高度不同，真实文档高度和主窗口位置就会多出这份差值。

**触发：** 首段是长段落且保留焦点，滚到后面阅读，再执行字号缩放。stride 被清成 `block_min_height`，首段仍作为岛实际渲染很高，却一直按最低高度扣 spacer。若估计 28 px、实际 4000 px，主窗口的真实位置可多出约 3972 px；后续回读只学习主窗口，岛的误差不能自行消除。

**守卫核查：** `zoom_by` 刷新窗口，不必将阅读位置送回焦点；行宽/字号清缓存后该路径可达。focus island 能保证实体挂载，但不保证几何估计正确。

**修法：** 量高回收记录所有实际挂载行，包括主 run 和 island，逐行保留文档行身份及布局子元素映射；读回必须匹配布局代数。主窗口和 island 是同一挂载协议的两种位置，不能只记录前者。

**置信度：** high；挂载、spacer 算术与学习遗漏静态确认，完整缩放阅读操作未 GUI 实测。

### R-4【必修】等待文字布局的滚动重试可以无限排帧

**位置：** `src/editor/render/sync.rs:ensure_focused_caret_visible`、`apply_pending_scroll_into_view`、`jump_center_converged`；`src/components/block/runtime/text_ops.rs:active_range_or_cursor_bounds`；`src/editor/workspace/find_replace.rs:jump_to_document_search_range`；`src/components/block/render/paint_parts.rs:Render::render`。

**触发：** 在搜索侧栏跳转到公式块里的命中，查询框继续持有焦点。公式块未聚焦时只显示图像，每帧清掉 `last_layout`；新的几何 recorder 能写 `last_bounds`，不能生成文字布局。

滚动目标只通过 `active_range_or_cursor_bounds` 查询，此函数依赖 `last_layout`，因此即使图像已经画出来也返回 None。`apply_pending_scroll_into_view` 的分支为：

```rust
if !has_bounds {
    self.schedule_followup_frame(cx);
    return;
}
```

它先返回，绕过后面的 90 帧预算递减与终止。下一帧依旧不可能拿到文字布局，重复约每 16 ms 排帧。未聚焦 Mermaid 等无文字布局目标有同类风险。

**预期与实际：** 应按图像/块壳几何定位，或明确结束无法精确定位的请求；实际只能估算位置并持续重绘，无法完成文字几何收敛。

**修法：** 滚动目标明确区分文字范围与原子块边界，使用对应当前几何；“没有布局”“等待布局”“该呈现永远没有文字布局”不可混为一态。所有重试分支都受统一次数/状态预算约束。

**置信度：** high；焦点交还、布局清除、预算旁路静态确认，未实测持续 CPU 占用。

## 维度三 · 安全

本轮没有确认安全漏洞或 GPUI 实体重入。未覆盖整个图片/SVG/导出安全面。

正常文字路径在 paint 回写文字布局、边界、行高和 gutter；request_layout、prepaint、paint 的阶段划分本身不是缺陷。问题是这些结果没有携带足够的身份和有效期，且不同呈现路径没有共享完整几何协议。

## 维度四 · 性能与效率

R-4 的无终止排帧直接带来持续前台工作；无需再将它重复计为一项性能缺陷。

**【建议】** `src/editor/window_state.rs:rendered_window` 每帧线性扫描全文 strides，focus island 位置另有前缀求和。虚拟化减少绘制元素数量，窗口定位本身仍为 O(全文行数)。应先修正确性与度量回收，再以访问规模和实际帧耗时判断是否需要前缀索引；本轮未做墙钟基准，不据此宣布当前必然卡顿。

## 与核心重构合并的边界

建议管线收敛为：当前逻辑状态与排版签名 → 当前呈现与排版结果 → 带版本的帧几何 → paint、命中和滚动共同消费。

- 原文/clean 选区仍是逻辑状态；display 范围在当前投影确定后派生。
- 排版结果由文字、runs、字体、宽度和主题度量共同决定，不能只代表“字符串没变”。
- 挂载几何覆盖正文、真实分组成员、表格格子、主 run 和 focus island。
- 布局回收和滚动调度明确使用哪一帧的结果；所有等待路径有终止条件。

这可与核心报告的 Selection/帧几何重构合并实施，不宜另造一套并行几何系统。

## 待专项复现与去噪

- `rendered_window` 的 `needs_fill` 在当前带状扫描与裁剪算式下看起来不会变 true；连续空段落真实高度又可能低于 `block_min_height` 估计。滚入未测空行区是否留下 spacer 空白，需要专项 GUI 测试，暂不计必修。
- 旧架构文档的“表格每帧重测”和旧报告的“公式缓存只写不读”不适用于当前代码：现在已有 `column_layout_memo` 和 LaTeX 内存/磁盘缓存，不能重复报告。
- 未新增回归测试、未跑全量构建与测试。应补的测试分别覆盖相同文字的样式/IME变化、仅改字体或代码字号、缩放后的屏外焦点岛、搜索跳转无文字目标与重试终止。
