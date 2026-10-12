# Velora 编辑器核心实现评审

> 本份保留为第一轮记录。统一评审与新增解析、历史、持久化发现见 [编辑器核心统一评审](editor-unified-7146668.md)。

- 固定点：`7146668760031fb0dc14cccd8734e74c564c2791`，分支 `dev`，2026-10-12。
- 范围：当前编辑器核心实现，而非仅 `v0.2.7..HEAD` 的提交差异。近期修复作为追踪根因的入口。
- 按用户要求直接审查当前分支，未创建 worktree；未修改产品代码，未提交。原有未跟踪 `.review/` 未动。
- 覆盖四条核心链路：原文解析与坐标映射 → 编辑回写与撤销；块内/跨块/跨格选择 → 输入、复制和删除；焦点投影 → 高亮与文字布局；挂载行计划 → 分组、表格和指针命中。同时检查 Buffer 的分块维护。
- 未覆盖：工作区、网络、更新、所有导出安全路径以及 vendored GPUI 的全量实现。本报告不是全仓质量认证。
- 验证口径：逐条复核调用链与既有守卫；部分用临时探针复用现有实现验证。没有为每条发现新增 GUI 回归测试，下文明确区分静态确认、探针实测与待复现。

## 总体判断

**值得现在做有边界的核心重构。问题集中在原文坐标、选区与输入协议、帧几何三个交界处；另有 Buffer 分块随编辑次数增长的结构性性能问题。** 这些不是单纯缺几个块类型分支：同一根因已经出现在多个消费者，继续逐处补判断会留下相邻路径。

已有基础仍可保留：保存读取 `TextBuffer`，撤销存增量 `AppliedEdit`，文档树承担渲染投影，局部重投影和虚拟化已有可用实现。此次证据不足以支持推倒编辑器重写。但“Buffer 是唯一事实源”目前没有贯穿所有定位与写入协议：部分写路径仍用树的重建 Markdown 坐标定位原文，部分输入路径仍用高亮字段判断逻辑选区。

四个维度的结果：

| 维度 | 必修 | 建议 | 存疑 | 最严重的问题 |
| --- | ---: | ---: | ---: | --- |
| 不变量与 GPUI 契约 | 4 | 0 | 0 | 重建 Markdown 的坐标被用于原文编辑，可写进标签内部 |
| 正确性、边界与并发 | 6 | 0 | 0 | 输入路由、粘贴、表格事务没有共享同一份逻辑选区协议 |
| 安全 | 0 | 0 | 0 | 所审路径未确认安全漏洞；未做全仓安全审计 |
| 性能与效率 | 1 | 1 | 0 | Buffer 无小块合并，约 10 KB 文本经一万次插入变成一万个分块 |

计数按下文根因组计；同组的多种触发不重复计数。维度之间不作统一严重度排名。

## 维度一 · 仓库既有不变量与 GPUI 契约

### I-1【必修】原文位置与序列化位置没有分开

**位置：** `src/editor/source_mapping.rs:collect_single_block_source_mappings`、`build_line_prefixed_content_mapping`、`push_root_source_mappings`；`src/editor/selection.rs:mapping_source_range_endpoint`；`src/editor/events/paste.rs:paste_multiline_through_buffer`。

映射从 `record.title.markdown_offset_map()` 出发；逐行映射的核心为：

```rust
content_to_source[markdown_offset + position] =
    content_start + position.min(file_len);
```

这能校正行前缀和跨行长度，却不能恢复**同一行内部**原文与序列化之间的变化。解析器消费 HTML 强调后，树可重新序列化成星号强调；原文还保存在 Buffer，两者已经不是同一把尺。

**触发：** 原文 `a <strong>b</strong> c`，在可见 `c` 前粘贴 `X\nY`。原文 `c` 位于字节 21，树序列化成 `a **b** c`，映射却定位到字节 8。复用当前解析和实际映射 helper 的探针输出：

```text
C_BEFORE_MAP=8 REAL_C=21
SPLICE="a <stronX\n\nYg>b</strong> c"
```

**预期与实际：** 应在 `c` 前编辑；实际写进 `<strong>` 标签。跨块选区的源码端点也使用此映射，存在错误复制/删除风险。

**守卫核查：** `push_root_source_mappings` 只钳块区间，错误位置仍在块内；单字插入的 `insertion_context_matches_source` 可使部分输入回退，但多行粘贴没有这道核对，不能据此判安全。

**修法：** 解析时保留原始源码到可见文本的映射，所有原文编辑共用它；序列化映射只服务序列化。块前缀账本仍可使用，但不能代替行内原文映射。把所有偏移裸 `usize` 改成不同坐标类型可作为此边界内的辅助手段。

**状态：** 遗留；置信度 high；映射和错误拼接已由临时探针实测，GUI 粘贴序列未实测。

### I-2【必修】高亮提前存成显示坐标，焦点投影改变后未重算

**位置：** `src/editor/selection.rs:sync_cross_block_selection_visuals`；`src/components/block/render/paint_parts.rs:Render::render`；`src/components/block/runtime/display_cache.rs:sync_inline_projection_for_focus`、`clear_inline_projection`；`src/components/block/element/text_element.rs:Element::prepaint`。

Editor 将 clean range 提前换成 display range 存到 `editor_selection_range`，而块在随后渲染时才按新焦点展开/收起定界符。投影切换仅重映射块自己的 `selected_range` 和 `marked_range`，没有重映射 `editor_selection_range`；prepaint 直接使用后者。

**触发：** `**abc**\n\n**def**`，先在第一段 `b` 前落光标，再 Shift 点击第二段 `e` 后。capture 按旧投影铺高亮，bubble 请求第二段焦点，下帧两段投影切换。第一段可能使用超出新显示文本的旧范围，第二段可能高亮定界符；复制/删除仍按 clean 坐标覆盖 `bc`、`de`。

**预期与实际：** 高亮应准确对应将被复制/删除的内容；实际逻辑范围与画面脱钩。即使没有数据越界，用户也可能删掉画面没有准确提示的文字。

**修法：** Block 保存逻辑 clean range，在当前投影确定后派生 display range；必要的派生缓存必须带投影代数。不要靠每次焦点改变再从 Editor 扫一遍补同步。

**状态：** 底层遗留，近期 Shift 扩选直接暴露；置信度 high；静态调用顺序确认，具体像素高亮未 GUI 实测。

### I-3【必修】几何没有统一的有效期与真实成员集合

**位置：** `src/editor/selection/pointer.rs:cross_block_endpoint_for_point`、`place_caret_below_document_end`；`src/editor/render.rs:build_rendered_row_plan`；`src/editor/table_edit.rs:table_cell_at_point`；`src/components/block/render/paint_parts.rs:Render::render`。

近期 pointer 修复改读挂载行，方向正确，但其他消费者仍直接读块/格子的 `last_bounds`；分组行内部也没有可靠的成员级几何。

已确认的三种触发：

1. **无标题 callout 吞掉组内正文命中。** `> [!NOTE]` 的默认标题只画静态 div，没有文本元素写 `last_bounds`。pointer 给该首成员 `last_bounds.unwrap_or(row_bounds)`，于是整组任何位置都可先命中空标题。上文拖入 callout 正文时，落点错误，正文漏选。
2. **隐藏成员重新进入命中扫描。** `Group.members` 来自折叠后的 `kept`，可能不连续；pointer 却遍历 `visible_start..visible_end`。有标题 callout 内先画完整，再折叠其中第一节，隐藏正文的旧边界可抢走上移后的第二节命中。真实成员集合与文档跨度被混为一谈。
3. **离屏表格仍能命中。** `table_cell_at_point` 对所有 `table_cells.values()` 扫旧 `last_bounds`，不检查当前帧或宿主表格是否挂载。画过 T1 后滚到 T2，同屏位置的旧 T1 格子可被 HashMap `.find` 优先返回，拖选屏外表格或阻断正文 Shift 扩选。

另有同源路径：`place_caret_below_document_end` 优先读全文末块旧边界。曾到过文末再向前滚动，在当前正文两侧留白点击，可能触发错误文末焦点及滚动请求；这一表现需专项 GUI 复现，未单列必修。

**修法：** 渲染产出统一帧几何快照，记录实际挂载实体、分组真实成员、格子、块壳边界和文字命中能力。指针、表格命中、文末留白共用它。缺成员几何应显式处理，不能把整组边界冒充该成员。`last_bounds` 可作为缓存，但不能无有效期地表示“当前屏幕上的对象”。

**状态：** 分组问题来自近期重构，表格旧边界为遗留；置信度 high；成员集合及缺失写入静态确认，滚动重叠表现未 GUI 实测。

### I-4【必修】历史文字布局被用来决定选区语义

**位置：** `src/components/block/runtime/cursor.rs:has_text_layout`；`src/components/block/render/inline_visuals.rs:render_text_or_mixed_inline_visuals`、`block_bounds_recorder`；`src/editor/selection/table.rs:table_cells_in_text_range`。

`has_text_layout()` 只返回 `last_layout.is_some()`。公式混合段落/格子失焦后切回可视元素，没有清掉旧 `last_layout`；新 recorder 只改 `last_bounds`。于是新图像边界与旧源码文字布局可能被组合使用。

**触发 A：** 先聚焦公式格，再移开焦点，之后向该格拖选。代码按旧文字布局量局部偏移，视觉却铺整格底色；首次没聚焦过的同格会整格选中。选择范围依赖之前是否点过它。

**触发 B：** 初次在未聚焦公式格按下，锚点偏移为 0；聚焦重绘后出现文字布局，再向左/上拖到另一格。规范化后公式锚点成为终点，但代码按当前 `has_text_layout=true` 生成 `0..0`，漏掉起拖公式格。跨块 Drag 的 `anchor_is_whole_block` 冻结了部分语义，表格没有对应协议。

**修法：** 当前呈现方式、命中能力与缓存是否存在分别建模；手势开始时捕获原子端点语义，焦点变化不能重新解释它。与 I-3 共用帧几何，但不要仅新增几个清缓存特例。

**状态：** 近期修复不完整；置信度 high；静态确认。

## 维度二 · 正确性、边界与并发

### C-1【必修】全局选区存在，但焦点块没有高亮时，输入绕过选区

**位置：** `src/components/block/input.rs:replace_text_in_range`、`replace_and_mark_text_in_range`；`src/components/block/interactions/keys.rs:paste_from_clipboard`；`src/editor/selection.rs:sync_cross_block_selection_visuals`。

输入是否请求 Editor 替换选区，以 `self.editor_selection_range.is_some()` 为判据。该字段只是当前块的非空视觉覆盖，不等同于全局逻辑选区。

**触发：** `abc\n\ndef`，从 `abc` 块尾拖到 `de` 后，焦点留在起拖块。该块覆盖范围为空，被设为 `None`，但全局选区非空。输入 `X` 应得到 `abcXf`，实际走块内输入得到 `abcX\n\ndef`，随后 Changed 收掉全局选区。格尾开始的跨格选择也有相同问题。

**修法：** Editor 根据逻辑选区路由输入；高亮字段仅供绘制。统一覆盖普通字符、粘贴、IME 和图片粘贴的入口，不能只补字符处理器。

**状态：** 遗留；置信度 high；静态确认，未实测键入序列。

### C-2【必修】多行粘贴的缓冲区快捷路径丢失替换区间

**位置：** `src/components/block/interactions/keys.rs:paste_from_clipboard`；`src/editor/events/block_event.rs:on_block_event`；`src/editor/events/paste.rs:paste_multiline_through_buffer`。

入口按 clean selection 拆出 `leading` / `trailing`，排除了选中文字，事件却没有携带真正的替换范围；快捷路径始终 `buffer.edit(at..at, &inserted)`，也没有先删除原选区。

**触发：** `abcde` 选中 `bc`，粘贴 `X\nY`。按既有物理行分段口径，预期 `aX\n\nYde`，实际 `aX\n\nYbcde`。普通非引用富文本段落即可到达。

**修法：** 粘贴与普通输入共用替换事务，明确携带替换源码范围、插入文本及目标选择状态；`leading` / `trailing` 可辅助结构决策，不能代替被替换区间。

**状态：** 遗留；置信度 high；入口到 buffer.edit 的静态路径确认。

### C-3【必修】跨格替换绕开了已有的单元格与 IME 契约

**位置：** `src/editor/selection.rs:replace_cross_block_selection_with_text`；`src/editor/selection/table.rs:replace_table_text_selection_with_text`；`src/editor/table_edit.rs:write_back_table_cell_source`；`src/components/markdown/table.rs:serialize_table_cell_markdown`。

同一表格的单格输入与跨格替换有两套写入协议：

- 跨格分支仅传 `new_text` 和 `undo_kind`，丢弃 `selected_range_relative`、`mark_inserted_text`。预编辑首次插入后没有 `marked_range`；后续 `replace_and_mark_text(None, ...)` 无法通过标记找到前次预编辑。焦点能否恢复、输入法是否提供显式 replacement range 会影响表现，可能残留预编辑串或中断后续输入；完整 IME 协议已确定缺失。
- 跨格分支将 `new_text` 原样插入 Buffer；单格路径经过既有单元格序列化，处理裸 `|` 与换行。跨格选中表头后输入 `|` 或粘贴 `X\nY`，会改变列分隔符或物理行，可能使整表不再解析为表格。单格粘贴当前有换行折平规则，跨格同样绕过了它。
- 该路径整棵重建文档后，还用旧 `selection.table_block_id` 查找目标格。`rebuild_root_blocks_from_buffer` 创建新实体，旧 EntityId 不再注册，因而这个目标恢复分支无法正常找回格子。编辑事务的目标必须使用可在重投影后恢复的源码位置/逻辑身份，不能继续依赖已替换的实体句柄。

**修法：** 表格可以继续对应多个不连续源码区间，但输入事务参数必须完整；插入起点格的文本必须经过既有格子输入/序列化规则。先明确单格与跨格的粘贴产品语义，再共用实现，不新增第二份转义表。

**状态：** 遗留；置信度 high；静态确认，未使用系统 IME 实测。

### C-4【必修】Shift 选区回缩到锚点所在块时丢失锚点

**位置：** `src/editor/selection/pointer.rs:extend_cross_block_selection_at_point`、`on_editor_capture_mouse_down`；`src/components/block/interactions/keys.rs:on_mouse_down`。

**触发：** 点击 A 中间落锚，Shift 点击 C，重绘后 C 聚焦；再 Shift 点击 A 中另一个位置。

Editor 发现 anchor/focus 同块便返回 false，随后 `begin_cross_block_drag_at_point` 清掉全局选区。A 已失焦，块处理器的 `shift && was_focused` 不成立，仅 `move_to`。应缩成 A 内选区，实际只剩光标。

**修法：** 扩选由一个状态转换保持锚点，跨块回到同块仍是同一次逻辑扩选。不要以焦点或所属块是否相同决定退回另一套锚点规则。

**状态：** 近期 Shift 修复引入；置信度 high；静态确认。现有 Shift 测试只覆盖向另一段扩选，未覆盖返回锚点块。

### C-5【必修】跨格选区没有进入历史与视图切换快照

**位置：** `src/editor/history.rs:capture_source_selection_snapshot`、`apply_selection_snapshot_in_current_mode`；`src/editor/selection/table.rs:replace_table_text_selection_with_text`。

快照识别跨块选区，否则读取活动块 `selected_range`，不读取 `table_text_selection`。单个连续 `UndoSelectionSnapshot.range` 也不足以精确表达跨格逻辑选择。

**触发：** 跨格删除后撤销，文本可恢复，但原跨格选区不恢复；跨格选中后切源码视图，只带走活动格的局部选区。

**修法：** 统一逻辑 Selection 快照，能够表达文本端点、原子对象和跨格选择；源码编辑范围列表从逻辑选择派生。撤销、视图切换共用同一恢复协议，并明确跨格到源码视图的表示策略。

**状态：** 遗留；置信度 high；静态确认。没有证据表明增量文本撤销本身普遍错误，缺的是选区现场。

### C-6【必修】CJK 强调侧翼改动还改变了格式命令的序列化结果

**位置：** `src/components/markdown/inline/stacks.rs:delimiter_can_open`、`delimiter_can_close`、`stack_transition_cost`；`src/export/html.rs:render_browser_html_body`。

这是近期提交的附带发现，不是核心重构的主理由。

对 `InlineTextTree::plain("测试加粗：测试")` 执行 `toggle_bold(6..15)`：父提交输出 `测试<strong>加粗：</strong>测试`；当前输出 `测试**加粗：**测试`。当前导出解析器输出 `<p>测试**加粗：**测试</p>`，格式命令添加的粗体导出后丢失且露出定界符。

**根因与修法：** UI 解析器的侧翼判据同时参与序列化器回退决策，而导出解析器没有同步该方言。保持既有块事件解析架构，明确导出兼容口径；不能用一个“导出仍按字面”的测试替代格式命令到实际导出的一致性验收。

**状态：** `7146668` 新增；置信度 high；当前与父提交的序列化及当前 pulldown 输出已由临时探针实测。探针未执行完整 GUI 导出。

## 维度三 · 安全

所审核心输入、几何和选择路径未确认安全漏洞。未全量检查导出 XSS、网络和文件路径，不能据此宣称这些部分安全。

GPUI 契约去噪：新增 canvas 在 prepaint 更新 Block，与已有文字元素写回几何的生命周期相符；没有证据支持将它直接报告为同实体 update 重入。任务、焦点和布局调用链中也未确认新的重入崩溃。

## 维度四 · 性能与效率

### P-1【必修】Buffer 分块数量随编辑次数增长，没有合并

**位置：** `src/editor/buffer.rs:edit`、`split_at`、`locate`、`slice`、`chunkify`。

每次插入都把新文本独立 `chunkify` 后 splice；原块被切开后没有合并，相邻小块也不合并。`locate`、`slice`、行号查询按 Vec 线性扫描。所谓 4096 字节分块上限只约束最大块，不能约束总块数。

复用原 Buffer 实现，仅在 `/tmp` 加块数读取探针，连续在尾部插入一个字符：

| 累计插入 | 文本字节数 | 实际块数 | 同内容重新分块的块数 |
| ---: | ---: | ---: | ---: |
| 0 | 5 | 1 | 1 |
| 100 | 105 | 101 | 1 |
| 1000 | 1005 | 1001 | 1 |
| 10000 | 10005 | 10001 | 3 |

**影响：** 后续编辑定位与分块扫描越来越贵；连续 K 次尾部插入至少包含累计 O(K²) 的扫描工作，成本不仅取决于文档长度，还取决于编辑历史。没有测完整 GUI 的毫秒时延，此处结论基于真实块数和可见循环。

**修法：** 先限定在 Buffer 内，插入/删除后局部合并小块，使块数与文本规模保持合理关系；再依据实测决定是否引入带字节/换行汇总的索引树。不要先把整套编辑器换成复杂数据结构。

探针还确认 `same_content` 将块边界纳入内容相等判断：相同字节、不同分块返回 false，可能保留本应丢弃的 IME 空操作历史。该后果未独立复现，不另计必修。

**状态：** 遗留；置信度 high；块数实测与复杂度确认。

### P-2【建议】虚拟化没有覆盖选择派生的全文开销

**位置：** `src/editor/selection.rs:sync_cross_block_selection_visuals`、`clear_cross_block_selection_visuals`；`src/editor/selection/table.rs:cross_block_covered_table_cells`。

每个跨块拖动事件克隆可见块列表，逐块 read/update；表格覆盖随后再扫描全文。点击清视觉也遍历全篇。视口窗口化减少元素创建，没有让这部分成本随挂载数缩小。

**修法：** 逻辑选区保存端点或区间，绘制时仅给挂载对象派生范围；记录上一批被写入高亮的对象用于清理。先用访问次数/更新次数的形状断言验收，再量交互时延。与 I-2、I-3 同期收敛，避免单独加一层全文缓存。

**状态：** 遗留；置信度 high；复杂度确认，未做墙钟基准，因此列建议。

## 重构边界与实施顺序

下面是基于已确认缺陷的建议，不是本次已经实施的修复。

1. **先收口原文映射和编辑事务。** 解析保留实际原文映射，输入携带真实替换区间；先覆盖 I-1、C-2 的会红用例。Buffer 仍是唯一落笔处，撤销继续记录 `AppliedEdit`。切勿只给粘贴加一次上下文核对，因为删除、搜索恢复等消费者仍在用相同映射。
2. **收口逻辑 Selection 与输入路由。** 块内、跨块、跨格可保留不同表达，但共享输入和快照协议；选择存在性由逻辑状态决定。表格事务携带 IME 参数并共用单元格写入规则，Shift 回缩保留原始锚点。处理 C-1、C-3、C-4、C-5。
3. **收口帧几何和渲染派生。** 只消费本帧真实挂载成员；高亮 clean→display 在当前投影确定后派生；呈现能力不能从历史 layout 是否存在推断。处理 I-2、I-3、I-4，并顺带消除 P-2 中的全文高亮派生。
4. **Buffer 内局部合并分块。** 用累计编辑后的块数和访问规模验收 P-1；在实测前不扩大为整套存储重写。CJK 导出兼容作为独立主题处理。

每一步均按 `velora-tdd` 先红后绿，单主题提交；修复完成后按项目规定运行生产构建与 bin 测试。此次仅评审，没有启动这些实现工作。

## 应补的验收边界

- 原文映射：HTML 强调、非规范代码跨度、不同 `<br>` 写法，在语法前/中/后执行插入、选区删除、多行粘贴；未编辑字节不变，UI 与 Buffer 对拍。
- 输入事务：普通选区与格选区、空视觉起点、正反拖动、普通键入、单行/多行粘贴和 IME `None` replacement range，共用相同替换语义。
- 渲染：先选择再切焦点/投影；先聚焦再失焦；先画再离屏再回屏；高亮覆盖必须与实际复制/删除范围对应。
- 分组：无标题 callout、带标题 callout、组内折叠、脚注组、表格、focus island；真实挂载成员以外不得返回命中。
- 历史：跨格编辑后撤销恢复文本和逻辑选择；视图切换有明确选择映射。原有增量历史和原文保真测试继续守住。
- Buffer：在小文档里连续插入/删除数千次，块数不能与按键次数无界线性增长；分块边界不同的相同字节应判相等。

## 本次验证与限制

已运行并通过：

- `cargo test --bin velora shift_click_selects_across_paragraphs -- --nocapture`。
- 直接运行现有测试二进制的 `ime_composition_replaces_cross_block_selection_and_marks_inserted_text`。
- `reprojecting_a_quote_keeps_the_neighbor_roots_untouched`。
- `typing_one_char_only_changes_the_text_at_the_caret --ignored --nocapture`；该确定性慢用例已明确执行，未将默认跳过算作通过。

这些验证说明已有代表性路径可用，不能反证本报告的状态组合缺陷。现有 IME 测试直接调用普通跨块替换函数，没有覆盖焦点块输入路由或跨格分支。

临时实测：`/tmp/velora-core-audit-7146668` 复用 Buffer；`/tmp/velora-markdown-review` 复用行内模块和实际映射 helper。前者只增加块数读取，后者为隔离 GPUI 依赖使用最小支撑模块；均未改仓库产品源码。

未运行 `cargo build` 或全量 `cargo test`，遵循只读评审流程。单条测试构建报告了依赖包未来 Rust 兼容性提示，不是本次已确认的编辑器缺陷。

## 未验证 / 值得一看

- `paste_multiline_through_buffer` 传入的 `leading.visible_len()` 属于 clean 坐标，`caret_source_offset` 接收 display 坐标；焦点显形时可能发生二次去投影。已有 I-1 足以支持收口映射，此项未独立动态确认，不计必修。
- 全文重序列化兜底和 `skip_next_resync` 依赖调用顺序，说明事务边界仍有收敛空间；没有在本轮找到足以单列的普遍漏写/撤销损坏路径，不能仅因结构复杂就宣布它已经坏掉。
- 富媒体其他无文字呈现路径的成员边界和高亮覆盖，需要在统一帧几何验收中逐类检查；本轮没有把每一种块都做 GUI 操作验证。

## 与上一份评审的关系

`docs/reviews/v0.2.6.md` 曾记录选择消费者分散、表格不在块树、光标动作未清全局选区等问题。当前已有动作 capture、剪切失败守卫和格内选择读取修复，不能将旧报告原样重报。本轮重点是这些局部修复之后仍存在的共享协议缺口：视觉范围代替选择状态、序列化位置代替原文位置、旧边界代替当前几何。不同范围的报告不作缺陷总数趋势比较。
