# Velora 编辑器核心统一评审

## 最终完成标准（2026-10-13 更新）

本轮目标覆盖核心修复、主线合并与 `v0.2.8` 发布；单项修复、提交完成或
本地默认测试通过均不代表整轮完成。保留当前分支工作，不新建 worktree，
每个功能点分别提交。用户已明确授权 ff-merge、发布与推送远端。

1. 完成本报告全部必修组、K-1 兼容修复及待专项验证；对实际确认的额外
   核心缺陷补充回归。逐项记录证据，不以局部字段断言代替真实操作验收。
2. 本地 `cargo build` 零警告，`cargo test --bin velora` 全绿；生成性能夹具，
   运行确定性慢用例，墙钟用例按既有约定单独验证，不放宽预算。
3. 确认工作树和提交范围后，以 `--ff-only` 将修复分支合并到 `main`。
4. 按 `velora-release` 更新 `Cargo.toml`、`Cargo.lock`、`CHANGELOG.md` 与
   `docs/releases/v0.2.8.md`，独立提交发布文件，创建注释标签并推送远端。
5. 以最终发布提交为准，观察 CI 的 macOS ARM64、macOS Intel、Windows x64
   构建、默认测试与确定性慢用例全部成功；观察 Release 工作流三平台
   打包与发布任务全部成功。
6. 核对 GitHub Release `v0.2.8` 实际包含三个非空安装包：
   `velora-0.2.8-macos-arm64.pkg`、`velora-0.2.8-macos-x64.pkg`、
   `velora-0.2.8-windows-x64-setup.exe`；记录工作流链接与最终提交。

只有以上各项均有当前状态证据，才将目标标为完成。

- 固定点：`7146668760031fb0dc14cccd8734e74c564c2791`，当前 `dev`，2026-10-12。
- 模式：当前核心实现的全量链路评审，不限于 `v0.2.7` 之后的 diff。
- 按用户要求直接审当前分支，不创建 worktree。初审时只读；后续实现与验收见修复进度。原有 `.review/` 未动。
- 本报告统一前两轮发现并补查解析、历史、IME、标签和持久化；[核心初审](editor-core-7146668.md) 与 [渲染补审](render-pipeline-7146668.md) 保留为分轮证据，本份作为统一结论。
- 验证分为：**静态确认**（主线程复核可达调用链、状态和守卫）、**探针实测**（复用原实现或复现明确的系统调用协议）、**待专项复现**。静态确认不等同于已经做过完整 GUI 操作。

## 结论与覆盖

**现在值得做核心重构。** 已确认的问题集中在几项共享协议：原文与显示位置、逻辑选择与输入事务、编辑前后状态捕获、文档/任务身份、渲染结果有效期、保存提交。多个入口各自维护这些规则，使同一缺口在编辑、选择、撤销和保存中重复出现。

可保留的基础包括 `TextBuffer` 落笔入口、增量 `AppliedEdit` 历史、块树投影、既有解析器、shaping/列宽/公式缓存与视口虚拟化。证据支持收口这些边界，不支持无边界地推倒全项目重写。

覆盖了 12 块：Buffer/编码；解析/文档树/渐进加载；源码映射；块输入/IME；结构事件；选区/表格；Editor 渲染/虚拟化；Block 排版/富媒体几何；撤销/视图恢复；粘贴/格式/段落命令；保存/恢复；标签/关闭/外部重载。对 HTML 粘贴与导出只核了相关一致性和已有清洗入口。

未做全量审计：网络、更新、安装器、所有工作区文件操作、全部导出安全路径、vendored GPUI。GPUI 只读了与事件时序相关的契约实现。

| 维度 | 必修根因组 | 建议 | 存疑 | 主要影响 |
| --- | ---: | ---: | ---: | --- |
| 不变量与 GPUI 契约 | 6 | 0 | 0 | 改错源码、选择消费者分裂、快照时序错误、旧任务污染新文档 |
| 正确性、边界与并发 | 7 | 0 | 0 | 替换遗漏、组合态未收尾、编码丢失、异步提交覆盖、滚动不终止 |
| 安全 | 1 | 0 | 0 | 私有文档保存后权限放宽 |
| 性能与效率 | 3 | 1 | 0 | 分块随编辑历史增长、前台连续加载、平方解析 |

共 17 组核心必修，多个同源表现合并计数；另有附录 K-1 的 CJK 导出兼容回归。等级依据具体错误结果或性能劣化，不依据文件大小、命名或“看起来复杂”。各维度不合成严重度总排名。

## 维度一 · 仓库不变量与 GPUI 契约

### I-1【必修】重新序列化的位置被当作原文位置

**位置：** `src/editor/source_mapping.rs:collect_single_block_source_mappings`、`build_line_prefixed_content_mapping`、`push_root_source_mappings`；`src/editor/selection.rs:mapping_source_range_endpoint`；`src/editor/events/paste.rs:paste_multiline_through_buffer`。

映射从 `record.title.markdown_offset_map()` 出发，逐行核心是 `content_start + position.min(file_len)`。它能处理前缀和行长，却不能恢复同一行内部被解析/序列化改变的原始写法。

**触发：** 原文 `a <strong>b</strong> c`，在可见 `c` 前粘贴 `X\nY`。原文 `c` 位于字节 21，重建的 `a **b** c` 将它定位到字节 8。探针复用当前行内解析与实际映射 helper，得到：

```text
C_BEFORE_MAP=8 REAL_C=21
SPLICE="a <stronX\n\nYg>b</strong> c"
```

**预期与实际：** 应在 `c` 前编辑；实际插进标签。跨块源码端点也使用同一映射。块区间钳制拦不住块内错位；单字插入的上下文核对不能保护直接 buffer 写入的粘贴/选择路径。

**建议：** 解析保留原文→可见文本映射；序列化映射只服务序列化，所有原文编辑共用前者。明确原文、clean、display、UTF-16 坐标，避免裸偏移混用。

**置信度/验证：** high；映射与错误拼接探针实测，完整 GUI 粘贴未实测。

### I-2【必修】局部高亮字段代替逻辑 Selection，消费者没有共享选择协议

**位置：** `src/components/block/input.rs:replace_text_in_range`、`replace_and_mark_text_in_range`；`src/components/block/interactions/keys.rs:paste_from_clipboard`；`src/editor/selection.rs:sync_cross_block_selection_visuals`；`src/editor/history.rs:capture_source_selection_snapshot`、`apply_selection_snapshot_in_current_mode`；`src/editor/selection/pointer.rs:extend_cross_block_selection_at_point`。

已确认四种表现：

1. `abc\n\ndef` 从 `abc` 尾拖到 `de` 后，焦点块切段为空，高亮字段被置 None。输入依据该字段判断全局选择存在性，应替换成 `abcXf`，实际可在起点局部插入成 `abcX\n\ndef`，随后清掉全局选择。
2. Source 快照跨 512 行分块恢复，只给各块写 `selected_range`，未建完整 `cross_block_selection` / `editor_selection_range`。复制、删除、输入与非焦点块绘制仍只消费起点那部分；再次捕获也丢另一半。现有测试只检查各块字段。
3. 跨格选择不进入历史快照，快照退回活动格局部范围。跨格删除后撤销或切视图不能恢复原逻辑选择。
4. A 中落锚 → Shift 点 C → C 聚焦 → Shift 点回 A：Editor 因同块早退，重新落锚并清全局选择；A 失焦，块只 `move_to`，应有的 A 内收缩选择消失。

**建议：** 统一逻辑 Selection 和状态转换，块内、跨块、跨格可以有不同表达，但输入路由、历史和视图转换必须消费同一协议。高亮只是派生结果；扩选始终保留原始锚点。

**置信度/验证：** high；可达状态与消费者静态确认，未将四组操作全部 GUI 复现。

### I-3【必修】“编辑前快照”通过延迟事件在编辑后读取

**位置：** `src/components/block/runtime/cursor.rs:prepare_undo_capture`；`src/components/block/input.rs:replace_text_in_range`；`src/editor/events/block_event.rs:on_block_event`；`src/editor/history.rs:begin_history_group`；`vendor/gpui/src/app/context.rs:Context::emit`。

Block 发 `PrepareUndo` 后立即修改 title 和 selection。GPUI `emit` 只是 `pending_effects.push_back(Effect::Emit { ... })`，订阅之后才执行；Editor 到时现读选择，得到编辑后的块状态与编辑前 Buffer。

**触发：** `abc` 光标在 `a` 后，输入 `X` 再撤销。文字可回到 `abc`，快照却可把光标放到旧文本 offset 2，而非原 offset 1；替换原选区时也无法准确恢复它。

**建议：** 在修改之前捕获明确的逻辑选择随事务传递，或由 Editor 先处理事务再改投影。先发事件不能视为已同步完成准备。

**置信度/验证：** high；GPUI 队列与快照调用顺序静态确认，光标操作未 GUI 实测。

### I-4【必修】文档替换没有作废全部待加载状态和任务身份

**位置：** `src/editor/tree.rs:replace_roots`、`flush_pending_source`；`src/editor/file_drop.rs:replace_document_content`；`src/editor/mod.rs:materialize_next_pending_source_chunk`；`src/app_menu/build_menus.rs:prompt_for_paths`。

`replace_roots` 清普通 `pending`，没有清 `pending_source`。新文档仅在自己有代码尾部时覆盖它，旧加载任务也没有校验文档身份。

**确定入口：** 菜单多选文件在一次同步 `cx.update` 内依次打开。先打开超过 512 行的旧 `.log`，再打开短新 `.log`；旧任务尚不能 poll，新文档无 tail。回调结束，旧任务按当前 `code_document=true` 把旧文件尾部追加到新树。若第二个是 Markdown，旧 source 尾可在后续结构操作 flush 时进入新树。

**影响：** 新文档投影混入旧文档正文，后续整篇写回可污染 Buffer 和保存内容。

**建议：** 文档替换同时作废两类 pending 和加载任务；任务携带文档身份/加载代次，回写前验证。不能仅按当前 `code_document` 决定旧任务写向哪里。

**置信度/验证：** high；同步多选入口与状态残留静态确认，未做文件选择面板实测。

### I-5【必修】渲染派生缓存的输入签名不完整且彼此不同

**位置：** `src/components/block/element/text_element.rs:Element::request_layout`；`src/components/block/runtime/mod.rs:refresh_cached_display_text`；`src/components/block/element.rs:build_text_runs`、`build_code_text_runs`；`src/components/block/render/parts.rs:cached_table_column_layout`；`src/editor/render/paint.rs:Editor::render`。

三处共享缺口：

- shape key 有文本/代码高亮代数，无普通行内样式或 `marked_range`。可见文字相同的 IME 提交、未聚焦块格式修改，可以复用旧 runs 的字体或下划线。
- 表格列宽 key 无字体家族，而测量读取正文/代码字体。仅改字体时格子重排，列宽沿用旧结果。
- 文档 stride 只在列宽或正文字号变化时清。仅改代码字号、字体、主题行高/内边距时，屏外占位继续旧高度，逐屏访问才学习新高度。

**建议：** 以实际依赖建立共享排版签名，明确文字、runs、字体、宽度、主题度量各影响哪些缓存。只跟踪字符串变化，不能代表排版输入没变。

**置信度/验证：** high；键和偏好刷新守卫静态确认，像素结果未逐项 GUI 实测。

### I-6【必修】几何与显示范围没有绑定当前呈现和完整挂载集合

**位置：** `src/editor/selection.rs:sync_cross_block_selection_visuals`；`src/components/block/runtime/display_cache.rs:sync_inline_projection_for_focus`；`src/components/block/render/inline_visuals.rs:block_bounds_recorder`；`src/editor/selection/pointer.rs:cross_block_endpoint_for_point`；`src/editor/table_edit.rs:table_cell_at_point`；`src/editor/render.rs:build_rendered_row_plan`；`src/editor/render/paint.rs:Editor::render`。

同一根因在五类消费者中出现：

1. clean 选择提前变成 display range 存下，之后焦点切换改变投影，但该范围未重映射。粗体两段之间 Shift 扩选时，高亮可能落在旧位置/定界符，复制删除仍按 clean 范围。
2. 混合公式段落/格子失焦后保留旧 `last_layout`，新 recorder 仅更新 bounds；`has_text_layout` 将缓存存在性当作当前能力。曾聚焦过与首次呈现的同格选区行为不同；公式格起拖、聚焦再向前拖时锚点还可被规范化为 `0..0` 而漏选。
3. 无标题 callout 首成员没有文字 bounds，却回退成整组 bounds，吞掉正文命中；折叠后的 Group.members 不连续，pointer 扫连续跨度又把隐藏旧成员纳入。
4. `table_cell_at_point` 全量扫格子旧 bounds，无当前挂载守卫；离屏旧表格可抢走当前表格命中。文末留白优先用末块旧 bounds 是同源待专项表现。
5. focus island 被绘制却不参与主 run 的量高回收。缩放后它的 stride 清成最低高度，真实长块仍很高；spacer 扣估计高度、元素插实际高度，主窗口可能被推入视口下方，后续仅学习主 run 无法消除岛的误差。

**建议：** 一份带版本的帧结果覆盖真实分组成员、表格格子、主窗口与焦点岛，并区分块壳与文字几何。逻辑 clean 范围在当前投影确定后派生；所有实际挂载行都参与量高回收。

**置信度/验证：** high；读写集合、阶段顺序和 spacer 算术静态确认，未将所有滚动/缩放状态 GUI 复现。

## 维度二 · 正确性、边界与并发

### C-1【必修】替换事务的区间、上下文和 IME 参数在入口之间丢失

**位置：** `src/components/block/interactions/keys.rs:paste_from_clipboard`；`src/editor/events/paste.rs:paste_multiline_through_buffer`；`src/editor/selection.rs:replace_cross_block_selection_with_text`；`src/editor/selection/table.rs:replace_table_text_selection_with_text`；`src/editor/table_edit.rs:write_back_table_cell_source`。

- `abcde` 选中 `bc` 后粘贴 `X\nY`，入口拆出排除原选择的 leading/trailing，但缓冲区快速路径只 `edit(at..at, inserted)`。应按既有分段口径得到 `aX\n\nYde`，实际保留 `bc` 成 `aX\n\nYbcde`。
- 跨格替换丢弃 `selected_range_relative` / `mark_inserted_text`，缺组合标记与正确选择恢复；又原样插 `new_text`，绕过单格换行折平及 `serialize_table_cell_markdown` 的竖线转义。输入 `|` 或多行粘贴能改变表格结构。
- 跨格替换整棵重建后仍用旧 `table_block_id` 找目标格，新树实体已更换，目标恢复失败。

**建议：** 事务携带真正的源码替换范围列表、插入文本、输入上下文、选择及组合态目标。重投影后的恢复用可恢复位置，避免旧 EntityId；表格继续复用已有单元格规则。

**置信度/验证：** high；完整入参与写侧静态确认，系统 IME/粘贴未逐项实测。

### C-2【必修】组合生命周期依赖文字 Changed，保存守卫又漏掉格子

**位置：** `src/components/block/input.rs:replace_text_in_range`、`unmark_text`；`src/components/block/runtime/normalize.rs:apply_title_edit`；`src/editor/history.rs:finalize_pending_undo_capture`；`src/editor/persistence.rs:has_marked_document_text`、`schedule_autosave`；`src/editor/events/table_nav.rs:on_table_cell_event`。

**同文本提交：** 最后预编辑为“中”，提交同一个“中”。marked 已清，但 title/kind 相同，不发 Changed。已准备的空 `ImeCompositionCommit` 组无法正常结算；后续输入进入旧组，可能与上一组合合并。即使显式 finalize，`edits.is_empty()` 早退仍不能封口上一组合。组合期间被守卫阻止的 autosave，也没有此次 Changed 来重新调度。

**单格预编辑：** `has_marked_document_text` 只扫 roots/children 的 visible snapshot，格子在 table runtime 中。格内预编辑已写 Buffer，守卫却看不到 marks，停留超过防抖、手动保存或保存关闭可把中间串落盘。活动字节快照处没有补查。

**建议：** 提交、取消、unmark、焦点变化是独立的组合态事件，文字不变也必须封口并恢复保存调度。组合状态属于文档编辑会话，覆盖格子与代码语言输入，不能仅扫块树推断。

**置信度/验证：** high；事件/历史/保存调用链静态确认，未使用系统输入法实测。手动 pending_save 有 render 重试，未将它误报为普通正文永久不能保存。

### C-3【必修】块类型变化绕过相邻解析边界处理

**位置：** `src/components/block/interactions.rs:on_delete_back`；`src/components/block/runtime/text_ops.rs:convert_to_paragraph`；`src/editor/events/block_event.rs:on_block_event`；`src/editor/document/blocks.rs:collect_paragraph_block`；`src/editor/paragraph_ops.rs:write_back_block_kind_region`。

**触发：** `alpha\n# beta\n# gamma`，在 beta 标题正文开头退格。该入口就地降级成 Paragraph，Changed 的 `structure_unchanged` 只比实体 ID，随后仅写 beta 自己的 span。

Buffer 变成 `alpha\nbeta\n# gamma`，运行时仍有两个独立段落 alpha/beta；全量导入同一 Buffer 会合成一个段落。当前 UI/选择边界与重开、切视图后的结构不同。

**建议：** 块类型变化共用接缝与失效区域处理。段落菜单已有带前根的写回逻辑，不能让退格降级另走只改本 span 的协议。验收局部更新结果与全量解析等价。

**置信度/验证：** high；降级入口与段落收集规则静态确认，未 GUI 实测。

### C-4【必修】切回脏标签丢失编码、BOM 和行尾形状

**位置：** `src/editor/workspace/tabs.rs:open_workspace_file_in_mode`；`src/editor/file_drop.rs:replace_document_content`；`src/editor/mod.rs:reset_buffer_for_text`、`attach_file_origin`；`src/editor/persistence.rs:document_bytes_for_save`。

**触发：** UTF-16/CRLF 文件 A 编辑 → 切 B → autosave 前切回 A。脏缓存返回 `(tab.markdown, Vec::new(), true, ...)`；新 Buffer 不继承 B 的形状，空 raw 又使 attach 直接返回。随后 A 的活动字节变成 UTF-8/LF，手动和自动保存均可改写编码。

复用实际 Buffer/FileShape 的状态探针确认：

```text
ORIGIN_BYTES=[255, 254, 89, 78, 50, 117, 13, 0, 10, 0]
DIRTY_REACTIVATED_BYTES=[228, 185, 153, 231, 148, 178, 10]
```

后台标签按磁盘形状编码的修复不能保护重新变为活动标签的无形状 Buffer。

**建议：** 标签/文档会话保存 FileShape 等必要状态，激活不应从仅有 LF 文本的快照丢失它；不必为此额外复制每篇原始全文字节。

**置信度/验证：** high；真实 Buffer 状态探针实测，完整标签操作未 GUI 实测。

### C-5【必修】异步保存提交缺少每篇文档身份与最终版本约束

**位置：** `src/editor/persistence.rs:schedule_autosave`、`save_dirty_workspace_documents_and_close`、`save_document_via_prompt`、`apply_successful_save`；`src/editor/workspace/tabs.rs:finish_close_workspace_tabs`、`reload_externally_changed_document_with_recheck`。

三种可达时序：

1. **外部修改被覆盖：** 后台 A verify→stage→verify 后继续 stage 大 B；外部改 A；watcher 对脏 A 早退，不改 revision。批处理回前台后只查全局 revision，再 rename A，覆盖外部内容。校验到提交之间包含其他全部文档的 I/O 和调度。
2. **放弃后台标签仍写盘：** autosave 已抓取 A，关闭非活动脏 A 并选择放弃。删除标签不使 revision/该篇提交失效；晚到 rename 仍写 A，提交之后才发现标签已不存在。
3. **另存为快照过期：** 干净 A 打开面板时抓旧正文；外部更新 A 被 watcher 重载；确认 B 后写旧快照，再无身份/revision 检查地把新 Buffer 绑定 B、标成干净。保存成功不代表当前 Buffer 已保存。

**建议：** 保存请求绑定文档身份、生命周期/内容版本、磁盘预期版本；提交时验证仍允许该篇落笔。最终版本检查与单文档提交紧密衔接；再次读取只能缩小竞态，若承诺不覆盖并发外部写，需要明确平台可提供的协调/提交方案。成功回写仅标记实际保存的版本，关闭放弃必须使该篇请求失效。

**置信度/验证：** high；时序及守卫静态确认，未用后台 I/O 同步点动态放大竞态。

### C-6【必修】多个落盘入口没有共享目标解析与提交协议

**位置：** `src/editor/persistence.rs:write_atomic`、`schedule_autosave`、`save_dirty_workspace_documents_and_close`。

手动 `write_atomic` canonicalize 目标，自动保存和保存全部关闭直接 temp→rename 原路径。编辑软链文件时，前者保留软链；后两者替换软链本身，真实目标不更新。

主线程在临时目录复现相同 rename 协议：

```text
autosave_symlink_preserved=False real_target_updated=False
```

另有可见协议差异：手动保存 sync 文件，批量/自动 staging 仅 `fs::write`；rename 失败后自动保存仅记日志，未向界面呈现原因或安排重试。此处不将未实测的断电丢失另计一项。

**建议：** 共用目标解析、staging、版本检查、权限、持久性和提交/失败协议；自动保存可有不同调度策略，但不能有不同文件身份语义。权限后果单列 S-1。

**置信度/验证：** high；软链系统调用协议实测，其余静态确认。

### C-7【必修】无文字布局的滚动目标可无限等待

**位置：** `src/editor/render/sync.rs:ensure_focused_caret_visible`、`apply_pending_scroll_into_view`、`jump_center_converged`；`src/components/block/runtime/text_ops.rs:active_range_or_cursor_bounds`；`src/editor/workspace/find_replace.rs:jump_to_document_search_range`。

搜索侧栏跳转公式命中，焦点交还查询框。公式未聚焦每帧清 last_layout，只提供图像 bounds；active range 查询却必须有文字 layout，永久返回 None。

`if !has_bounds { schedule_followup_frame; return; }` 位于预算递减之前，绕过 90 帧上限，每约 16 ms 继续排帧。该目标不会通过重绘自然产生所等待的文字布局。

**建议：** 滚动目标区分文字范围与原子块边界，消费相应当前几何；等待布局、暂不可用和该呈现没有文字布局分别处理，所有重试分支共享终止预算。

**置信度/验证：** high；焦点、呈现与预算旁路静态确认，未量 CPU 或连续帧数。

## 维度三 · 安全

### S-1【必修】保存可放宽已有私有文档权限

**位置：** `src/editor/persistence.rs:write_atomic`、`schedule_autosave`、`save_dirty_workspace_documents_and_close`。

新建临时文件没有继承目标 permissions。macOS/POSIX 在 `umask 022` 下，原文档 `0600` 经 temp→rename 后变成 `0644`，同机其他用户可读取。

主线程隔离临时目录，以相同 File.create/write/sync/rename 协议复现：

```text
permission_before=0o600 permission_after=0o644
```

**建议：** 统一提交协议保留既有文件所需的访问权限；新文件策略另行定义。没有在本轮验证 ACL/xattr 或 Windows 权限，不能扩张为全平台权限结论。

**置信度/验证：** high；系统调用协议实测，应用保存入口静态确认。

已核 HTML 粘贴跳过 script/style，导出有 URL 事件过滤和原始 HTML 清洗。没有确认新的 XSS 或代码执行漏洞，但这不是导出安全认证。新增 canvas 的 prepaint 写回模式与现有文字元素相符，未将其误报为 GPUI 重入。

## 维度四 · 性能与效率

### P-1【必修】Buffer 分块随编辑次数增长，无局部合并

**位置：** `src/editor/buffer.rs:edit`、`split_at`、`locate`、`slice`、`chunkify`。

每次插入独立 chunkify 后 splice，没有合并相邻小块。最大块 4096 字节不限制总块数；定位/切片/行号查询线性扫 Vec。

实际 Buffer 探针：

| 连续单字插入 | 文本字节 | 块数 | 同内容重新分块 |
| ---: | ---: | ---: | ---: |
| 100 | 105 | 101 | 1 |
| 1000 | 1005 | 1001 | 1 |
| 10000 | 10005 | 10001 | 3 |

成本随编辑历史增长；K 次尾部输入包含累计 O(K²) 扫描。`same_content` 还比较块边界，相同内容不同分块返回 false，可能误保留 IME 空操作历史，未单列已复现后果。

**建议：** 先在 Buffer 内局部合并小块并验收块数/访问规模，再依据基准决定是否引入索引树。

**置信度/验证：** high；块数实测与循环复杂度确认，未量全 GUI 时延。

### P-2【必修】渐进加载任务没有实际让出前台

**位置：** `src/editor/mod.rs:start_pending_materialization_task`。

`cx.spawn` 的 loop 只有同步 `WeakEntity::update(materialize_next_pending_chunk)`，没有 await/yield。每批预算结束后立即处理下一批，一次 poll 完成整个剩余文件；notify 只排帧，不让出执行权。

**建议：** 每批真实让出前台，限制连续工作量；仍需要 I-4 的身份验证与取消。仅把预算设小不能提供交互响应。

**置信度/验证：** high；无挂起点与执行器契约静态确认，未量前台冻结时长。

### P-3【必修】解析器在多个边界反复扫描整段前后文

**位置：** `src/editor/document/blocks.rs:collect_paragraph_block`；`src/editor/document/parse.rs:paragraph_can_continue_through_boundary`；`src/editor/document/import.rs:markdown_requires_source_mode_fallback`。

支持的跨空行行内 code span，在每个边界 join+重扫全部前缀，再扫描余下全文找闭合。临时抽取原 helper，仅加入扫描计数：

| 重复正文/空行 N | 输入字节 | 扫描字节合计 |
| ---: | ---: | ---: |
| 100 | 612 | 51,510 |
| 200 | 1,212 | 203,010 |
| 400 | 2,412 | 806,010 |
| 800 | 4,812 | 3,212,010 |

输入翻倍扫描约四倍，最终仍是一根段落，root budget 无法切批。预检查还有 N 行未闭合的 ` ```rust ` 各自扫描剩余行，约 N(N−1)/2 次判定，发生在首批构建前。

**建议：** 维护解析状态或一次性前瞻结果；不能用更小根预算或后台包装掩盖算法问题。

**置信度/验证：** high；code span helper 扫描计数实测，围栏预检查静态复杂度确认。

### P-4【建议】虚拟化只减少元素，部分派生仍每次扫全文

**位置：** `src/editor/selection.rs:sync_cross_block_selection_visuals`、`clear_cross_block_selection_visuals`；`src/editor/selection/table.rs:cross_block_covered_table_cells`；`src/editor/window_state.rs:rendered_window`。

拖动派生高亮反复 clone/read/update 全部可见块，表格覆盖再扫一次；窗口计算每帧扫全文 stride，焦点岛另作前缀求和。成本仍与全文规模有关。

**建议：** 先让当前挂载对象从逻辑选择派生视觉，记录需清理集合；再按访问规模与帧时延判断是否需要前缀索引。本轮未量墙钟，不宣布所有大文档都已卡顿。

**置信度/验证：** high；复杂度确认。

## 建议的重构范围与顺序

### 修复验收进度

目标保持为全部核心问题；以下仅记录已经发生的修复和验证，不把局部通过视为全量完成。

| 条目 | 状态 | 证据 |
| --- | --- | --- |
| I-4 加载生命周期 | 已修，本地全量验收通过 | `84a108a`；两种跨文档尾部泄漏先红后绿，相关组 9 通过 |
| C-4 标签编码形状 | 已修，本地全量验收通过 | `3a6ee30`；UTF-16/CRLF 脏标签切回先红后绿，标签组 22 通过 |
| P-1 Buffer 分块 | 已修，本地全量验收通过 | `c072c8d`；分块增长/内容相等先红后绿，Buffer 组 29 通过 |
| S-1 保存权限 | 已修，本地全量验收通过 | `819f150`、`22deed9`；手动、自动、保存关闭的权限回归通过 |
| C-6 共用落盘协议 | 已修，本地全量验收通过 | `22deed9`、`7f77fad`；软链、权限、最终提交校验及错误显示共用协议 |
| C-5 保存提交身份 | 已修，本地全量验收通过 | `7f77fad`；五条失败回归先红后绿，有效另存正向通过；保存、标签、事务组分别 23/23/5 通过 |
| I-1 原文坐标 | 已修，本地全量验收通过 | `d28b22b`；定位、真实输入与多行粘贴先红后绿，覆盖表格、提示框与脚注；跨块复制、替换、撤销字节验收通过，定向回归 327 通过、8 慢用例默认跳过 |
| I-3 编辑前快照 | 已修，本地全量验收通过 | `3b49700`；普通/反向选择、中文和跨行替换、删除扩选先红后绿；六组窗口回归覆盖代码、表格、源码、引用和脚注；定向回归 331 通过、4 慢用例默认跳过 |
| I-2 逻辑选择消费 | 已修，本地全量验收通过 | `72b881b`；空锚点切片的输入/预编辑/粘贴、源码跨分块复制与替换、跨格撤销及视图恢复、Shift 回缩先红后绿；相关组 410 通过、6 慢用例默认跳过 |
| I-5 排版输入签名 | 已修，本地全量验收通过 | `ef7e8fc`、`bf36b62`、`7e851c1`；样式/marked、表格字体、代码字号与主题行高分别先红后绿，稳态复用通过；渲染快照 10、滚动 32、表格两组 24/33 通过 |
| I-6 帧几何 | 已修，本地全量验收通过 | `930978a`、`1401490`、`53e08bd`；当前投影派生高亮、帧版本与完整挂载集合、分组实际成员、表格命中、焦点岛量高和公式格端点语义分别确认失败后通过 |
| C-2 组合生命周期 | 已修，本地全量验收通过 | `d299331`；七条回归先红后绿，运行时/撤销/保存组分别 119/15/23 通过 |
| C-1 替换事务 | 已修，本地全量验收通过 | `8436249`、`5b0bd57`；多行选择替换、显形偏移、跨格竖线/换行、相对选择、组合态与后续输入先红后绿 |
| C-3 解析接缝 | 已修，本地全量验收通过 | `49a055e`；左右边界与全量解析对拍，非规范邻块原文保留、继续输入和撤销通过，定向 170 条通过 |
| C-7 滚动调度 | 已修，本地全量验收通过 | `4777121`；公式/Mermaid 搜索跳转及缺失几何预算先红后绿，目标实际入视且查询焦点保持，滚动/查找/打字机 53 条通过 |
| P-2 加载调度 | 已修，本地全量验收通过 | `33d967a`；调度穿插和加载中插行先红后绿，加载组 11 通过，渐进导入慢用例单跑通过 |
| P-3 解析复杂度 | 已修，本地全量验收通过 | `26ffd2c`；增量行内状态、共享围栏前瞻，扫描规模线性回归先红后绿 |
| P-4 全文派生 | 已修，本地全量验收通过 | `a0940d0`、`905271d`；选区访问范围受约束，稳定帧行高扫描为零，脏后缀与权重边界回归通过 |
| K-1 导出兼容 | 已修，本地全量验收通过 | `6f7253a`、`e3be8e2`；行内 HTML 清洗保持标签配对，手动中文强调的真实导出先红后绿 |
| 冷启动及粘贴专项 | 已确认并修复，本地全量验收通过 | `ba55c50`、`8436249`；不靠光标重绘的视口覆盖、原选择替换范围回归先红后绿 |
| 额外保存与选择时序 | 已修，本地全量验收通过 | `e784a21`、`8781ad0`、`4d99712`；初次保存外部版本保护、渐进加载格子输入归属、公式拖回同格的输入与撤销回归通过 |

各功能点分别提交。生产构建和全量测试只在全部修复收尾运行；此前定向测试结果不代替最终门禁。

不是按 17 条分别叠分支，建议按下面几笔边界工作收敛；每笔仍要以会红的行为用例起步。

1. **文档会话与异步提交。** 先隔离文档身份、FileShape、加载代次和保存版本，统一替换/激活/关闭的生命周期；消除旧尾污染、编码丢失、放弃后写盘。统一原子保存目标、权限与提交/错误协议。是否引入完整会话对象应结合内存评估，必要状态不宜继续散落在标签文本和 Editor 字段里。
2. **原文映射与编辑事务。** 解析产出真实原文映射；编辑前捕获选择，事务携带替换区间列表、上下文和目标状态。落笔、记账、失效区域、重投影与恢复共用流程，避免 Block 先改、Editor 后猜。
3. **逻辑 Selection 与组合态。** 块/跨块/格子选择统一输入和快照协议，恢复覆盖实际消费者。IME 的提交/取消独立于文字 Changed；保存守卫直接读文档组合状态。
4. **解析边界与调度。** 块类型变化统一处理邻根接缝；局部解析对拍全量解析。清掉平方前瞻，再令分批任务真正让出执行权。
5. **渲染帧结果。** 统一排版签名和带版本几何，覆盖所有挂载对象；display 范围最后派生；滚动消费文字/原子几何且有结束条件。保留现有有效缓存。
6. **Buffer 局部维护。** 合并小块，修内容相等的定义；在访问规模和真实基准支持前不扩大为整套存储重写。

以上是初审建议；已落地范围与证据以修复表为准。修复遵循 `velora-tdd`，逐主题提交；代码修复收尾再跑项目规定的生产构建和 bin 测试。

## 应作为核心门禁的完整操作序列

| 序列 | 验收性质 |
| --- | --- |
| 非规范行内源码 → 选中/粘贴 → 保存/重开 | 改动位置正确，未编辑字节保真，投影与全量解析一致 |
| 选择 → Shift 回缩 → 切视图 → 复制/删除 → 撤销 | 同一逻辑选择贯穿所有消费者，并恢复原始位置 |
| 单格/跨格中文预编辑 → 同文本提交/取消 → 下一次输入 → 保存 | 组合不残留、历史不串组、未提交文本不落盘 |
| 标题/引用/代码退格降级 → 输入 → 切视图/重开 | 块边界与 Buffer 的解析结果一致 |
| 多选打开大代码文件与小文件 → 首次编辑/保存 | 旧尾及旧任务不能进入新文档 |
| UTF-16/CRLF 脏 A → B → 脏 A → 保存 | 保留 A 的编码、BOM 和行尾，不能继承 B 或退回默认 |
| autosave staging → 外部改盘/放弃标签 → 提交 | 旧请求不能覆盖外部版本或已放弃的编辑 |
| 另存面板 → 原文件外部重载 → 确认 | 过期成功结果不能把新 Buffer 标为已保存 |
| 焦点留在长块 → 滚到后文 → 字体/字号变化 | 所有挂载高度回收，窗口与 spacer 不分裂 |
| 搜索跳转公式/图表 → 查询框继续输入 | 按原子几何定位，调度能结束 |
| 小文档累计数千次编辑 → 撤销/继续编辑 | 分块及工作量受文本规模约束，内容相等不依赖块边界 |

测试应断言最终 Buffer、逻辑选择、实际命令结果与当前帧几何，而不只断言局部字段已赋值。针对性能先断扫描/块数/更新次数，再量时延。

## 本次验证

本轮在当前源码运行：

- `cargo test --bin velora author_report -- --nocapture`：**24 passed，0 failed，0 ignored**，覆盖作者编码、公式、代码、表格、转义、图片、标题/目录和真实复制/保存入口。
- `a_multi_update_ime_composition_undoes_cleanly`：通过。
- `a_source_selection_spanning_a_chunk_boundary_lands_on_both_chunks`：通过。

后两条的通过不反证本报告：它们没有覆盖同文本提交封口，以及恢复跨分块选择之后的真实复制/删除。此前普通 Shift、普通跨块 IME、局部引用重解析及保真慢用例结果见分轮报告。

临时探针复核：原文/序列化偏移、Buffer 分块增长、编码形状丢失、软链替换与权限变更、解析扫描次数。探针没有启动完整 GUI；按协议复现的系统调用结果不冒充已执行应用保存操作。

初审阶段未运行 `cargo build` 或全量测试，也未为评审修改产品/测试；本节记录初审证据，后续修复的全量验收另记。单组测试构建出现依赖包未来 Rust 兼容提示，不作为核心逻辑缺陷。

## 未验证、去噪与兼容性附录

- **待专项：** `rendered_window.needs_fill` 的算式看起来不会为 true，连续空行实际高度又可能低于估计，是否留下 spacer 空白需 GUI 测试，不计必修。
- **待专项：** 多行粘贴把 `leading.visible_len()` 的 clean 偏移传入 display 定位函数，可能二次去投影；未独立动态确认，已由 I-1 的映射收口验收覆盖。
- **不报告为缺陷：** `skip_next_resync` 与重序列化兜底复杂，仍需随事务收敛，但本轮没有证实普遍漏写或增量文本撤销损坏。不能只凭复杂度宣布它坏掉。
- **去噪：** 当前已有表格列宽和公式缓存，旧文档“每帧全量重算/缓存只写不读”的描述已过时；不重复旧评审中已经修复的箭头收选区、剪切失败顺序、格内选择读取问题。
- **K-1【兼容性必修，独立主题】** `src/components/markdown/inline/stacks.rs:stack_transition_cost` 使用新的 CJK 侧翼后，手动加粗“测试加粗：测试”的中间部分从 `<strong>` 回退改为 `测试**加粗：**测试`；当前 pulldown 导出按字面显示。父提交与当前输出已由临时探针验证。UI 方言扩展与可导出序列化需明确兼容口径；不据此要求重写块解析器。此项不计入 17 组核心统计。

### 附加确认与修复

- 脚注原文前缀与首行相对位置分别由 `d526a5d`、`1c25723` 修复；包含真实编辑、复制与撤销字节回归。
- 源码“选择整篇文档”的原生输入范围由 `95cf209` 修复；复制包含文末换行。
- 初次版本异步登记前的外部修改保护由 `e784a21` 修复，并验证正常首次保存及晚到回调。
- 渐进加载格子的输入归属由 `8781ad0` 修复，公式起拖再回到同格的整格选择由 `4d99712` 修复；实际输入与撤销通过。
- 冷启动补帧由 `ba55c50` 修复；用例明确主题尺寸，查询框持有焦点时仅按补帧通知重绘，旧实现隔离后在视口覆盖断言失败。
- README、截图、`showcase`、`image_scale` 与构建说明按明确要求合并为 `ae58408`，独立于核心修复。

### 修复收尾（当前状态）

17 组必修、P-4、K-1 和附加确认项均已分别提交，定向回归通过。冷启动与最后一组缓存、选择归属改动已完成只读复审，未发现可确认的新缺陷。

本地验收已完成：

- `cargo build --locked`：通过，Velora 源码零警告。移除本轮测试辅助入口产生的生产死代码警告，独立提交 `3f474b7`。
- `cargo test --locked --bin velora`：2005 通过、0 失败、39 忽略。
- 生成 `fixtures/perf` 后按 CI 的排除名单运行确定性慢用例：23 通过、0 失败。
- 将其余墙钟预算与手动探针逐条、单线程运行：16 通过、0 失败。两条加载探针使用实际 10 MiB 夹具，代码探针使用 `.log` 文件入口。
- `git diff --check`：通过。Cargo 既有依赖未来兼容提示保留为限制，不等同于 Velora 源码警告。

本地全部 2044 项测试均已执行通过。主线合并、发布提交、远端三平台 CI 与三个安装包仍待后续验收，不以本地全绿标记目标完成。
