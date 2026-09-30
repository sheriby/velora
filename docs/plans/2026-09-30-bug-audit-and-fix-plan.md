# 2026-09-30 Bug 审查结果与修复计划（含 TDD 协议与进度表）

状态：执行中（按单元逐条修，每单元一个提交，标题 `fix(scope): 描述`）
用途：**唯一事实来源**。上下文压缩/换会话后，先读本文件，再看文末「进度表」和 `git log`，从第一个非 ✅ 单元继续。

## 0. 背景与方法

- 起因：用户报「搜索经常不跳转、不高亮字符串」，要求系统性 bug/性能审查后开始修。
- 方法：4 个只读审计代理（性能 / 数据安全 / 工作区搜索索引 / 浮层输入）+ 本人对搜索路径实测复现。
- 证据规则：所有条目带 `file:line`。标 **[实测]** 的条目已用 gpui 测试复现过（测试代码会随对应修复单元固化为回归用例）；其余为读代码核实。
- 评审时仓库状态：`main` = `76940b9`，测试 1036 过 / 3 条环境性失败（macOS 红绿灯预留、无剪贴板/IME 容器，干净树上同样失败，非产品缺陷）。
- 旧 roadmap/handoff（`2026-09-27-dev-iteration-roadmap.md`、`2026-09-27-dev-branch-handoff.md`）用户已宣布作废，不引用、不更新，只作历史。

## 1. 问题清单

### P0（数据会被改错 / 核心功能坏）

| # | 问题 | 证据 |
| --- | --- | --- |
| 1 | **⌘F 跳转后焦点被抢进正文，接着敲字直接改写文档** [实测]：回车跳到命中 → 搜索框失焦、正文块获得焦点 → 下一个键替换掉选中命中（`# Alpha` → 敲 `X` → `# X`，查询仍是 alpha） | `src/editor/history.rs:228` 设 `pending_focus`；`src/editor/render.rs:753` `apply_pending_focus` 无条件 focus 块；`src/editor/workspace.rs:2402` 只有打开面板时设 `search_focus_pending` |
| 2 | **打完查询立刻回车/⌘G = 什么都不做** [实测]：`schedule_workspace_search` 一进来清空 `document_search_source`/`document_active_range`，新结果要 120ms 去抖+后台搜索才落地；期间 `find_next_document_match` 直接 return。改一个字也会触发同样重排 | `src/editor/workspace.rs:2136-2138`、`:2160-2161`、`:2518-2521`、`:2490` |
| 3 | **文件树右键目标被每帧重置**：侧栏每帧 `sync_workspace_models` 把 `workspace.selected` 改回活动文件；新建文件/文件夹、粘贴、重命名、删除都在点击时读它 → 右键 `drafts/` 新建会落到活动文件旁，重命名/删除作用在活动文件上 | `src/editor/workspace.rs:2035-2041`（每帧重置）、`:3922`（每帧调用）、`:1040-1043`（点击时读）、`:1576`（菜单渲染按改后的选择） |
| 4 | **⌘S 后自动保存误报外部修改并把自己关掉**：手动保存只更新 `Editor::file_version`，标签的 `file_version` 不更新（路径未变即跳过）；再打一个字自动保存拿旧版本号校验刚写的盘上内容 → 「检测到外部修改」→ `has_external_autosave_conflict` 停掉整个会话自动保存 | `src/editor/persistence.rs:479-506`、`src/editor/workspace.rs:1765-1768`、`src/editor/persistence.rs:185-189`、`:118` |
| 5 | **文件历史「恢复」不可撤销**：模块注释承诺「可继续编辑或撤销」，实际恢复路径清空 undo/redo → 误按 Enter 丢当前未保存内容且 ⌘Z 无效 | `src/editor/file_history.rs:113` → `replace_document_from_markdown` → `src/editor/file_drop.rs:343-345` |

### P1（搜索/一致性；用户报的症状来源）

| # | 问题 | 证据 |
| --- | --- | --- |
| 6 | **切渲染/源码模式后文档内高亮全丢**（Ctrl+Tab 或工具按钮），直到改查询或编辑才回来 | `src/editor/window_state.rs:415-471` 只重建 blocks 不重算；高亮唯一入口 `src/editor/workspace.rs:439` 只在搜索落地时调 |
| 7 | **折叠标题里的命中永不显示**：⌘G 找到 range 但块被折叠过滤、不挂载 → 不滚动不高亮；大纲跳转有展开逻辑，搜索跳转没有 | 过滤 `src/editor/workspace.rs:601-611`；大纲展开 `:2962-2969`；`jump_to_document_search_range`/`find_next_document_match` 无展开 |
| 8 | **对「有未保存修改」的文件点搜索结果跳错位置或静默失败**：搜索读盘，跳转把盘上行列偏移套到内存文本 | `src/editor/workspace.rs:6100-6117`（读盘）、`:5153-5185`（套 `current_document_source`） |
| 9 | 关侧边栏后正文搜索高亮残留 | `src/editor/workspace.rs:1697-1703` |
| 10 | **watcher 不刷新文件树、丢掉 Remove**：外部 git checkout / rm / 新建后，树、⌘P、工作区搜索文件列表永远旧（点进去「无法预览」） | `src/editor/watcher.rs:20-27`（只收 Modify/Create）、`:56-63`（只重载+索引） |
| 11 | 只打开单个文件（不打开文件夹）不启动 watcher → 外部修改不重载 | `src/editor/workspace.rs:898-900` vs `:2031`、`:1825` |
| 12 | 反链/标签面板只按 `document_revision` 失效 → 别的文件外部改了 `[[A]]`，A 的面板不刷新 | `src/editor/workspace_index.rs:382-384` |

### P2（性能；长文档明显卡）

| # | 热点 | 证据 |
| --- | --- | --- |
| 13 | **每次按键整篇序列化+克隆+比较**（10 MiB 文档 = 每键 ~10 MiB 分配，UI 线程） | `src/editor/history.rs:112`、`:66`、`:152` |
| 14 | **每帧从第 0 块算到光标的 source mapping**（光标靠后时每帧全量重建） | `src/editor/render.rs:2412` → `src/editor/history.rs:37` → `src/editor/source_mapping.rs:695-755` |
| 15 | **每次按键重建整篇行计划 + 折叠过滤全扫**（plan key 含每键自增的 `document_revision`） | `src/editor/render.rs:2452-2478`、`src/editor/window_state.rs:465-466`、`src/editor/workspace.rs:539-611` |
| 16 | 状态栏字数/长块提示按 `document_revision` 失效但输入是稳定快照 → 每键全文扫描 | `src/editor/status_bar.rs:20-29`、`:128-138` |
| 17 | 滚动窗口每帧 O(全部行) 前缀和；找焦点块/选区统计每帧遍历全部可见块 | `src/editor/window_state.rs:117,144,194`；`src/editor/render.rs:2493` → `src/editor/tree.rs:149`；`src/editor/status_bar.rs:67` → `src/editor/selection.rs:928` |
| 18 | 每个挂载块每帧深拷贝 Theme；滚轮每 tick 起一个定时任务；跨块拖拽每 mousemove O(全部块) | `src/components/block/render.rs:2200`；`src/editor/events.rs:987-1004`；`src/editor/selection.rs:378,479-511` |

### P2（其它正确性/工程）

| # | 问题 | 证据 |
| --- | --- | --- |
| 19 | 命令面板无 IME/非 ASCII 输入（只吃 `is_ascii_graphic`，没接 `ElementInputHandler`），退格删标量不删字素 | `src/editor/command_palette.rs:128`、`:116` |
| 20 | 关掉 ⌘P/⇧⌘P 不恢复焦点，之后敲字丢失（已有 restore helper 未用） | `src/editor/quick_open.rs:226`、`src/editor/command_palette.rs:63`；`src/editor/close.rs:115` |
| 21 | Esc 关不掉标题栏菜单、关不掉 info 弹窗 | `src/editor/context_menu.rs:341`（未调 `close_menu_bar`）、`src/editor/render.rs:2289` |
| 22 | 搜索内容缓存只按 mtime，同秒改写返回旧内容 | `src/editor/workspace.rs:6108-6117` |
| 23 | `session.json` / `config.toml` 非原子写 + 多窗口互相覆盖；损坏时静默重置为默认 | `src/config/session.rs:55-58`、`src/config/preferences.rs:1378,1537-1542,1014-1016` |
| 24 | 表格插入对话框不进 undo（其它表格操作都进） | `src/editor/context_menu.rs:453-508` |
| 25 | 自动保存冲突记错路径（取第一个有 path 的）→ 冲突标记清不掉 | `src/editor/persistence.rs:222-228` |
| 26 | 链接索引增量重扫跳过代码文件，全量却索引 → 不一致 | `src/editor/workspace_index.rs:191` vs `src/editor/workspace.rs:724` |
| 27 | 只打开文件时无 watcher 时，外部修改不重载（同 #11） | — |

## 2. TDD 协议（每单元必须遵守）

1. **RED**：先写一个小的失败测试，跑，确认它是因为**目标行为缺失**而失败（不是拼写/环境问题）。
2. **GREEN**：写最少代码让它过。不加投机功能、不顺手重构。
3. **REFACTOR**：绿了再清理；测试保持绿。
4. 一个垂直切片一个循环：一个测试 → 一次实现 → 下一个测试。**不要**先写一整套测试。
5. Bug 修复**必须**先有失败测试（复现即证明，也是回归护栏）。**没见过红的测试不算数**。
6. 绿 = 全部绿。别的测试炸了算你头上。
7. **绝不为过测而放松断言**——改代码或当面重谈需求。
8. 测试放在公开边界（Editor 的公开方法 / gpui 事件入口），只测行为不测内部实现。
9. 允许的例外（需一句话说明）：纯布局/样式、一行委托、配置/声明式改动、难以搭车的遗留代码（先在能到达的最近接缝加特征测试）。
10. 每单元完成门槛：`cargo test --bin velora <相关过滤>` 全绿 + `cargo build` 零新警告 + 该单元测试确实红过。

## 3. 修复单元与顺序

> 顺序原则：先数据正确性（P0），再一致性（P1），性能单独一批。每单元独立提交。

| 单元 | 覆盖 | 做法要点 | 验收（TDD） |
| --- | --- | --- | --- |
| **F1** 查找跳转 | #1、#2 | 去抖窗口不清 `document_search_source`（保留上次结果，与新结果同策略）；`find_next_document_match` 在 source 为 None 时回退 `current_document_source(cx)`；跳转后**不把焦点交给正文块**（保留搜索框焦点），活动命中改用 `editor_selection_range`（可脱离焦点绘制）或跳转后重设 `search_focus_pending` | ①「打完立刻回车会跳」测试；②「跳转后焦点仍在搜索框、敲字进查询不进正文」测试（两条都会先红） |
| **F2** 高亮完整 | #6、#7、#9 | `toggle_view_mode` 末尾在高亮有效时重算高亮；`jump_to_document_search_range` 前展开命中块的所有折叠祖先（复用大纲展开逻辑并 bump `fold_state_version`）；`toggle_workspace_drawer` 收起后清高亮 | ①切模式后高亮仍在；②折叠章节内 ⌘G 会展开并高亮；③关侧栏后无残留高亮 |
| **F3** 树右键目标 | #3 | `sync_workspace_file_tree_inner` 只在 `workspace.selected.is_none()` 时跟随活动文件；或把右键目标存进 `WorkspaceContextMenu` 状态由动作读取 | 「右键目录 → 菜单渲染后 → 新建文件的目标目录是右键目录」测试 |
| **F4** 保存与冲突 | #4、#25 | `apply_successful_save` 同步刷新同路径标签的 `file_version`/`markdown`/`dirty`；冲突上报携带**真正失败的文件路径** | ①⌘S 后再编辑，自动保存不报冲突（红→绿）；②两个脏标签、一个外部改，冲突路径正确 |
| **F5** 文件历史撤销 | #5 | 恢复历史版本时先捕获当前内容进 undo（或走不清历史路径），兑现「可撤销」承诺 | 「恢复历史版本后 ⌘Z 能回到恢复前内容」测试 |
| **F6** 脏文件搜索跳转 | #8 | 命中点击时优先用**内存文本**解析行/列（命中行号来自盘上快照，若文件脏则用当前内容重新定位；至少做 char-boundary 校验失败时不静默）；可选：搜索前先把脏标签内容并入搜索源 | 「文件有未保存修改时点搜索结果跳到正确位置」测试 |
| **F7** watcher | #10、#11、#12 | 事件收 `Remove(_)`；Modify/Create/Remove 后合并刷新文件树（防抖）；隐含根（单文件打开）时也 `start_watching`；索引面板失效计数与 `document_revision` 解耦 | ①外部新建/删除文件后树更新；②单文件打开后外部修改会重载；③别的文件改 `[[A]]` 后 A 的反链面板更新 |
| **F8** 性能批次 | #13-#18 | 13：undo 捕获不每键整篇序列化（只在实际入栈时算，或复用稳定快照+delta）；14：光标映射延后到真正需要时算/按块索引缓存；15：结构版本与文本版本分离，纯文本编辑只补受影响行；16：状态栏缓存改挂稳定快照版本；17：前缀和缓存/二分 + 用 `active_entity_id`；18：Theme 用 Arc/只拷贝覆盖项、滚轮防抖任务、拖拽只更新变化块 | 每项一个行为测试或基准断言；10 MiB 夹具下按键/帧开销不随文档增长（至少断言不再每键全量序列化：用一个计数探针） |
| **F9** P2 收尾 | #19-#26 | 逐条小修：命令面板接输入处理器（IME/字素退格）、⌘P/面板关焦点恢复、Esc 关菜单与 info 弹窗、搜索缓存加长度/版本、session/config 原子写+合并、表格插入进 undo、索引重扫与全建谓词一致、单文件 watcher | 每条一个最小行为测试（IME 用组合事件、Esc 用 simulate 键、原子写用写坏文件后仍可读） |

## 4. 进度表（压缩后从第一个非 ✅ 继续）

| 单元 | 状态 | 提交 | 备注 |
| --- | --- | --- | --- |
| F1 查找跳转 | ✅ 完成 | 未提交 | 新增用例：`document_find_enter_right_after_typing_still_jumps`（去抖窗口内回车立刻跳）、`document_find_jump_keeps_the_query_field_focused`（跳转后焦点留在查询框、键入不改文档）。改动：`find_next_document_match` 无快照时回退 `current_document_source`；`jump_to_document_search_range` 面板打开时重设 `search_focus_pending`；`ensure_focused_caret_visible` 无焦点块时用 `active_entity_id` 作滚动目标 |
| F2 高亮完整 | ✅ 完成 | 本次提交 | 新增用例：`document_find_highlights_survive_a_view_mode_switch`、`document_find_jump_unfolds_the_section_containing_the_match`、`closing_the_sidebar_clears_document_find_highlights`。改动：`toggle_view_mode` 末尾重算高亮（`window_state.rs`）；`unfold_sections_covering_source_range` + 跳转前展开折叠章节；`toggle_workspace_drawer` 收起时同步清理高亮 |
| F3 树右键目标 | ✅ 完成 | 未提交 | 新增用例：`workspace_context_menu_keeps_the_right_clicked_directory`。改动：新 helper `follow_active_document_in_workspace_tree`（只在 None/File 选中时跟随活动文件），替换两处无条件重置（每帧同步 + 扫描落地） |
| F4 保存与冲突 | ⬜ 未开始 | — | — |
| F5 文件历史撤销 | ⬜ 未开始 | — | — |
| F6 脏文件搜索跳转 | ⬜ 未开始 | — | — |
| F7 watcher | ⬜ 未开始 | — | — |
| F8 性能批次 | ⬜ 未开始 | — | 可拆分多个提交 |
| F9 P2 收尾 | ⬜ 未开始 | — | 可拆分多个提交 |

已观察到的环境噪声（不要当回归）：
- `workspace_search_accepts_unicode_platform_input`、`quick_open_accepts_ime_text_for_non_ascii_file_names`：容器无剪贴板/IME，干净树同样失败。
- `many_tabs_never_slide_under_the_window_controls`：macOS 红绿灯预留区断言，Linux 上必失败。
- `large_code_document_opens_within_budget`：写入时间预算的计时测试，全量并行跑时偶发失败，单跑稳定通过（导入路径与 F1/F3 改动无关）。

图例：⬜ 未开始 / ⏳ 进行中 / ✅ 完成（附提交号）

## 5. 恢复指南

1. 读本文件第 1 节（问题清单）与第 4 节（进度表）。
2. `git log --oneline -15` 对照「提交」列，确认哪些单元已落地。
3. 从第一个非 ✅ 单元继续，严格按第 2 节 TDD 协议。
4. 每完成一个单元：更新本表状态 + 提交号，然后提交代码。
5. 已知环境噪音（不要当回归）：`many_tabs_never_slide_under_the_window_controls`（macOS 红绿灯预留）、`quick_open_accepts_ime_text_for_non_ascii_file_names`、`workspace_search_accepts_unicode_platform_input`（剪贴板/IME 容器差异）。
