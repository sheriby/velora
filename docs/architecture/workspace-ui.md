# Workspace & UI：工作区、浮层、命令与配置

> 基于 `dev`，2026-10-14 重写。引用用 `文件:函数名`。
> 相关：[overview.md](./overview.md) · [editor-core.md](./editor-core.md) · [render-pipeline.md](./render-pipeline.md)

## 1. 启动（src/main.rs）

1. 解析参数（`-v/-h/-d(--detach)` + 文件/目录）；macOS detach 会重新拉起自身。
2. `Application::new().with_assets(VeloraAssets)`——SVG 图标按精确路径 `include_bytes!` 内嵌。
3. `app.run` 体内顺序：Dock 图标 → `config::load_or_create_app_preferences` → `I18nManager::init_with_language_id` → `ThemeManager::init_with_theme_id` → `EditorSettings::init` → `net::install_http_client` → `components::init_with_keybindings`。**菜单延迟安装**（首帧后计时器），首帧只画窗口。
4. 首窗口：无参数 → `restore_last_session`（session.json；活动标签同步开，其余每帧间隔开一个）；有参数 → `open_startup_window`。随后 `restore_recovery_windows`（内容与磁盘一致的快照跳过）。
5. `open_editor_window`（app_menu.rs）：恢复记住的窗口 frame（不在任何屏上就搬回主屏、尺寸保留）→ `Editor::from_file_source`；目录参数 → `open_workspace_window` + `set_workspace_root`。开窗后装两个监听：关窗前落盘 frame、拖动/缩放防抖落盘 frame（`editor/close.rs`）。

`VELORA_STARTUP_TIMING=1` 把分阶段耗时打到 stderr。

## 2. Workspace 是内嵌状态，不是实体

**没有单独的 Workspace GPUI 实体**。`Editor.workspace: WorkspaceState`（根在 `src/editor/workspace.rs`，功能代码在 `workspace/` 子模块）是普通结构体字段：

- 一切读写都经 `impl Editor` 的方法；**重绘靠对 Editor 实体 `cx.notify()`**——WorkspaceState 自己没有通知能力，忘 notify 就不重画。
- 渲染帧内允许直接改状态（`render_workspace_panel` 里就调 `sync_workspace_models`），这是本仓既有口径。
- 渲染入口：`render_workspace_panel` / `render_document_tabs` / `render_activity_rail` / `render_status_bar`。

状态分几组：面板（开关、活动页签、宽度、滚动把手）· 树（根、节点树、展开集合、选择、扫描任务与代数、错误红字、外部改动冲突）· 走盘文件名单（与树**分离**，搜索/⌘P/全部替换/双链共用）· 标签（`open_documents` + `active_document`）· 大纲（树、`[TOC]` 条目、增量段缓存）· 搜索（查询、范围、结果、代数、四个开关、文档内命中表、各自的输入态与焦点）· 浮层（树右键菜单、标签右键菜单、树内名称编辑、树焦点）。

## 3. 标签与阅读现场

- `WorkspaceDocumentTab`：路径、恢复快照 id、文件版本号、文本快照、`FileShape`（仅脏标签有——后台标签没有缓冲区）、脏标记、预览标记、`view: Option<DocumentView>`。
- **标签是快照不是 Editor 实体**：单个 Editor 在切换激活标签时换入换出内容。`snapshot_current_document` 把**缓冲区文本**存回标签（取块树序列化的话，一份没编辑过的文件进快照就已经被洗过一遍）。
- `DocumentView` 是阅读现场：视图模式、正文版本指纹、滚动偏移、**行锚点**（无有效行高度量时按行换算，与行高估值无关）、逐行高度量、`editing_state`（undo/redo 栈 + 每块的折叠/长行展开/表格列宽备忘，按可见下标 + 块类型对位恢复，不持有旧实体）、选区。
- 交还判据：前缀版本对不上 → 丢行高度量与编辑状态；视图模式相同才逐块恢复折叠与列宽；**完整正文长度一致才恢复 undo/redo**（恢复时把最后一组连续键入改成不可合并——切标签结束连续键入组）。列宽恢复还要求宽度与主题一致，否则按当前视口重测。
- **现场只活在进程里，不写进会话文件**。本次会话没读过的标签是 `None`，按新文档从顶部与渲染态起步。
- **预览（临时）标签**：入口分浏览型（工作区搜索结果、文档内查找、⌘P、wikilink、树单击）与固定型（树双击/键盘、常规打开）。转正点只有一个：`finish_dirty` 里脏标记由 false 变 true 那次调 `pin_active_preview_tab`。销毁旧预览的名单**必须在 `snapshot_current_document` 之后算**（活动标签的脏标记只在那一步写回，早算会把刚改脏的预览当干净销毁），真正删除放在打开流程末尾（防旧活动文档被推回标签集时复活）。
- 脏标签标题后画实心圆点，判定与自动保存开关无关，只看有没有落盘。
- 标签拖放按目标左右半区插到前面或后面；右半区允许把标签放到末尾，重排不切换活动文档。

### 编辑器组与分屏

`workspace/split.rs` 管理窗口内的编辑器组：第一次分屏时，窗口根 `Editor` 保留标题栏、侧栏入口、状态栏和关闭守卫，文档与标签迁到子 `Editor`。`GroupNode` 用递归二叉树记录左右/上下布局与分隔比例，节点与尺寸包装都建立 flex 容器，避免交叉嵌套时块布局把百分比列宽压成零；每组独立持有标签集、块树、选择、滚动和视图模式。所有分组标签始终放在唯一的窗口标题栏内，同组每个标签左侧均绘制同色的 2×18px 居中短线，不同组错开色相，浅色与深色主题分别调整色线亮度；同组标签紧挨排列，组间保留 12px 间距，不绘制分隔符。标签使用常规背景，活动标签保留底部高亮，不增加顶部彩线、独立色块或编号；正文不增加分组装饰或标签行，也不绘制整圈活动边框。所有标签共用窗口级横向滚动区，组内不再另起滚动区或挤压标签；活动标签变化时滚动到可见位置，并在首帧视口尺寸稳定后复核。

- 标签右键、正文右键、选中工具栏、命令面板与动作 `SplitEditorLeft/Right/Up/Down` 共用 `split_active_document`。向右默认键为 `Cmd+\` / `Ctrl+\`；原「清除格式」移到 `Cmd+Option+\` / `Ctrl+Alt+\`。
- `EditorFileDrag` 统一携带文件路径；标签拖拽另带来源编辑器，文件树拖拽没有来源视图。拖拽到正文边缘时显示半屏落区和方向文字，松手创建组，中心落点移到已有组；普通拖拽移动，macOS Option / Windows Ctrl 拖拽复制。文件树中的文件也可拖到同一落区：边缘新建分屏，中央在落点组打开并激活；松手前不打开文件，拖入后固定为标签，不移走其他组的视图，已打开文件复用未保存内容。未命名文档以 `Untitled` 留在原组，迁移时保留内存正文与编辑现场；恢复快照和关窗检查覆盖它，保存并关闭先选择文件路径，再处理其他组的脏文件，取消保存保留窗口。目录与大纲条目不参与文件拖拽。标签左右半区继续表示前后插入。移空或关闭最后一个标签后折叠组，分隔条可拖动。新分屏和分隔条拖动都保护每个叶组至少 320px 宽、180px 高；空间不足的拖拽显示提示并拒绝拆分，连续操作按布局树计算尺寸，不依赖新组绘制完成。
- 同一文件的各组使用同一份 `TextBuffer` 内容，保留各自投影。增量日志带实际写入视图的身份；通知先后不能代替写入来源，否则后台组的通知会把未刷新投影误记为最新。普通叶块原位更新，跨根结构变化才走重建；引用、表格运行时仍由既有解析与初始化入口维护。
- 共享文档的恢复 id、脏位、磁盘版本一致。活动视图接管共享 undo/redo，关闭一个视图前把记录留给其他视图；切回后台标签从共享缓冲区恢复最新内容。
- 自动保存由窗口根统一收集所有组并按路径去重；关闭窗口也汇总所有组。提交前校验共享缓冲区当前内容，不能只验汇总时的标签快照。原生菜单与打开文件操作路由到活动组。
- `SessionState.editor_groups` 保存分屏树、比例、各组标签和活动组；旧会话缺少字段时保留原恢复流程。分组里的窗口级浮层换算到窗口坐标，根模态位于所有组的浮层上方。

## 4. 文件树与监听

- 节点 id 是 `"file:{path}"`，标题节点另有命名空间（`is_outline_node_id` 区分——展开集合与树/大纲共用）。`children_loaded` 区分"空目录"与"未扫目录"。
- **扫描是分层的**：默认扫固定层数（`WORKSPACE_SCAN_DEPTH`），更深层留占位，展开时懒扫一层（任务按节点 id 去重并存进 map 防 drop 取消）。后台扫描带**代数计数器**，过期结果丢弃；旧树保留到新树落地（防闪空）。
- 跳过目录的唯一判据是 `SKIPPED_DIR_NAMES`，侧栏扫描与走盘名单必须逐条一致，否则"侧栏看得见、搜索搜不到"。
- 走盘名单 `collect_workspace_files_on_disk` 用 ripgrep 的并行 walker（关掉 gitignore/hidden 规则，只按上面那张表过滤），换根与 watcher 刷新后后台重走。
- 排序：目录恒在前，组内按偏好（名称/修改时间/类型），`sort_by_cached_key` 每节点只算一次键。
- 面板列表超过阈值行就**开窗虚拟化**（固定行高，上下 overdraw，上下垫等高空白保滚动条几何）。所有行类型必须等高，否则开窗切错边界。
- **树内名称编辑**（`workspace/tree_edit.rs`）：新建文件/新建 Markdown（默认名选中主干）/新建文件夹/重命名共用单行输入，自带焦点句柄、**blur 即确认**、挡重入；I/O 在后台；重名只在输入行提示红字，其它错误送应用内模态。重命名成功用 `remap_moved_path` 重映射所有标签路径、活动路径与图片基目录，**保留未保存内容与恢复快照 id**。
- **树的选择与正文焦点独立**：右键先保存点击目标进菜单再聚焦树；重绘与其它文档激活不能把菜单操作转移到活动文档（`selected` 非 None 时不被重绘改写）。F2 只在树焦点下生效。
- **监听**（`editor/watcher.rs` + `workspace/session_watcher.rs`）：notify 递归监听根，Modify/Create/Remove 经 mpsc 泵进前台循环 → 重载外部改动的文档 + 防抖刷新树（重扫树、重走名单、重跑搜索）。同根不重启 watcher；测试构建只记账不起 OS watcher。
  - 重载链路：**脏标签直接跳过**（未保存编辑优先，冲突由保存路径的版本校验兜住）；读盘失败记红字并**补一次**复查（只允许一趟）；读出的内容按规范化文本哈希比对，相同就不算改动；不同才重载——干净标签换文本快照与版本号，活动文档走 `ImportKind::Restore`（模式、视口、光标都不动）并 `attach_file_origin(raw)` 接回原始字节与形状（漏这一步，第一次保存会把 CRLF/GB18030 洗成 LF/UTF-8）。
  - 图片改动先失效 GPUI 资产缓存。

## 5. 侧栏三面板

外壳 `render_workspace_panel`：**收起时直接返回 None——大纲在收起时一帧都不算**（`[TOC]` 的读者在自己需要的那一帧调 `sync_workspace_outline`）。开着则每帧同步树、大纲、标签与懒扫层。左列活动图标：已开在该页再点 = 收起。收起时必须 `refocus_document_after_sidebar_collapse` 把悬空焦点交还正文（否则按键派发回退根节点，见 §9 的派发机制）。宽度按视口 clamp，按工作区落盘。

- **文件**：见 §4。
- **大纲**（`workspace/tree_sync.rs:sync_workspace_outline`）：块树的一份投影，**按根块缓存行摘要**（`OutlineSegment`：这段的标题 + 段字节数 + 进出两头的围栏状态），摘要键是"块实体 + 走进这块时的围栏状态"——围栏状态必须进键，同样的字节在栏里/栏外扫出来的东西完全不同。判断"这块动过没有"有三个信号，缺一个就错：① 缓冲区脏区间与这块相交；② 段的**字节数**变了（拆块合块把接缝重新分配，块自己的字节可以一个字没动却指着另一段）；③ 区间起点没落在行首（退回整篇重扫）。懒导入未完时一帧不动，且这个早退必须在取走脏区间**之前**。只有文字变了就原地换标签（不动 id/展开态/选中），否则重拼标题树。滚动跟随只在大纲页开着时跑，读当前挂载行、按块自己的区间 + `buffer.line_of` 换算。成本接在 `outline_rebuilds` / `outline_lines_scanned` / `outline_full_rescans` 三个计数器上。
- **搜索**：两条链，别混。
  - **工作区范围**：`schedule_workspace_search`（代数 +1、正则编译失败显示诊断并**停止**、去抖、去抖期保留旧结果防闪空）→ 输入是走盘名单 → 按并行度分片后台扫描，每片 `catch_unwind` 隔离 → 全局结果上限、每文件"一行一条"。文件内容经 mtime+长度校验的缓存（有总量与单文件上限），解码走与打开文档同一份漏斗。
  - **文档内查找**（⌘F）：**前台**一次算出 `DocumentMatchTable`（键 = 文档身份 + 修订 + 查询 + 选项），**列表、高亮、跳转、全部替换四个消费方只准读这一张表**——任何一处另起炉灶重扫都会重新引入"列表说第 7 个、跳转落别处"这类漂移（随机一致性测试守着）。
  - 命中画进块：`sync_document_search_highlights` 按 `SearchHighlightKey`（含身份、修订、查询、选项、视图、折叠版本、**根块数**）判全量重算还是只挪活动命中；渲染态只为与命中相交的根块重建它自己的映射，跨块命中按块裁段。根块数不可省：懒导入续建不动缓冲区版本，少了它新块永远不高亮。
  - 跳转前：`flush_pending_materialization` → 展开覆盖该范围的折叠章节 → 焦点交还查询框（同帧稍后生效）。工作区命中先按预览模式开文件，再按"第 k 个含命中行"对位（行号会因未保存编辑漂移，序不灭）。
  - 全部替换：文档内从后往前，能换算到块的走块编辑器（保撤销），换算不核对不过的走缓冲区字节写回，整批一次撤销捕获；工作区版逐文件重写 + 原子写，活动文档仍走块编辑器。确认模态回调要核对范围/查询/替换文本/选项/根/路径六项未变才执行。

⌘P 快速切换（`quick_open.rs`）过滤走盘名单里的文本与图片，结果有条数上限，输入经 Editor 的浮层 IME 路由。⇧⌘P 命令面板（`command_palette.rs`）条目来自 `commands::commands()`，回车与点行同一条收尾 → `window.dispatch_action`。

## 6. 状态栏（editor/status_bar.rs）

左：面包屑（点击 reveal 到树）。右：行列号（**仅 Source 视图**，在缓冲区坐标算，不做整篇序列化）→ 三选一（图片预览显示尺寸 / 代码文档显示行数 / 否则字数）→ 视图模式切换钮 → 超长块提示 → 自定义按钮。

字数与长块提示的缓存键是文档修订 **加一个静默窗口**：修订变了先沿用旧值，后台任务等文档安静下来再补算并通知（打字期间不付整篇分词的钱）。

## 7. 浮层：层级、互斥与收起

**层级 = 窗口根 children 顺序**（`render/paint.rs` 末段，后进树者画在上）：选中工具栏 → 正文右键菜单 → 树右键菜单 → 标签右键菜单 → 表格插入对话框 → wikilink 补全 → LaTeX 补全 → 公式编辑器 → 文件历史 → 更新通知 → 互斥五选一（信息框 | 模态 | 拖放替换 | 未保存 | 目录选择）。

**⌘P 与 ⇧⌘P 不在窗口根**：它们叠在正文区（content_area）内，压正文而不是顶替正文。图片预览直接替换正文区。

- **模态**（`editor/modal.rs`）：所有提示走应用内模态，同一时刻只一个；`modal_key_interceptor` 在 keymap 之前拦截按键；按钮/Enter（默认位）/Esc（取消位）。有源码审计测试禁系统原生弹窗。
- **选中工具栏**（`selection_toolbar.rs`）：**没有"打开"这一步**——每帧由 `selection_toolbar_anchor` 现算锚点，条件之一是不存在任何别的浮层（右键菜单、表格对话框、模态、信息框、菜单栏、两种补全、公式编辑器、⌘P、⇧⌘P 全算），还要排除跨块拖拽进行中与"搜索跳转产生的选区"。锚点不成立这一帧就不画。**新增浮层必须同步加进这串互斥清单**。几何每帧现算（选区外接框，上方优先、放不下改下方、都不行按视口夹回；滚出视口的选区不再拖着一条指向空白的工具栏）。
- **正文右键菜单**（`editor/context_menu/`）：状态存在 Editor 上（渲染时不新建实体），行数据、置灰判定、二级面板悬停状态机（离开父行只排一个延迟关闭任务，期间进子面板就取消）、几何测量都在 `document_menu.rs`。菜单与工具栏的**可用判定同源**（`has_text_selection`、`writes_through_the_buffer`、各命令的 `*_is_available`），工具栏下拉取的是同一份行数据。置灰那一行的悬停说明要写清"为什么没反应"。
- **收起只有一条路径**：`context_menu.rs:dismiss_contextual_overlays` 关树菜单、两种补全、公式编辑器、正文菜单、表格对话框。**不含**模态、信息框、⌘P、⇧⌘P、文件历史、选中工具栏。规矩是：任何"开新浮层"或"换上下文"的入口先调它（打开面板、开始树内改名、开右键菜单、展开侧栏、批量关标签、正文按下起选区、切视图模式、拖放替换、光标历史跳转）。Esc 总入口 `on_dismiss_transient_ui` 按 模态 → 信息框 → 菜单栏 → 树内改名 → dismiss_contextual_overlays → ⌘P/⇧⌘P 逐级处理并 stop_propagation。

## 8. 事件路由（三层，时序从前到后）

1. **`cx.intercept_keystrokes`**（`render/paint.rs` 注册，先于 keymap 解析）：依次问模态 → 文件历史 → wikilink 补全 → LaTeX 补全，谁消费谁 stop_propagation。
2. **窗口根 capture**：`events.rs:on_editor_key_down_capture`——只管两件事：shift+方向/Home/End 清"搜索跳转选区"、裸 Tab 在表格格/列表项/段落/代码块上转成缩进命令。
3. **焦点派发**：keymap 动作按焦点链走。正文块各有自己的 FocusHandle 与 InputHandler；侧栏树焦点在树滚动容器上；**浮层单行输入的文本与 IME 走 Editor 自己**——canvas 在 paint 里 `window.handle_input`，Editor 的 `impl EntityInputHandler`（`workspace/input_handler.rs`）按 `active_overlay_input` 判定归属（树内改名 > 公式编辑器 > 命令面板 > ⌘P > 替换 > 查询，谁的 FocusHandle 聚焦归谁），UTF-16 ↔ UTF-8 在这一层换算。

## 9. 命令、动作与键位

- 动作声明：`actions!(velora, [...])`（`components/actions.rs`）；带载荷的动作（选主题/语言/标签页/最近文件）用 `#[action(namespace = velora)]`。
- `SHORTCUT_DEFINITIONS`：每条命令的 id、分类、默认键、**`context`**。`context` 语义：`Some("BlockEditor")` 只在焦点链经过块元素时匹配；`None` 是全局绑定（焦点在对话框/侧栏也生效）。可绑定命令是动作的子集；没有默认键的命令（Heading1-6、导出、部分视图开关）不在表里，靠菜单/命令面板/输入前缀触发。
- `resolved_keybindings(config)`：全表经 `key_binding_for`（命令 → 具体动作的一对一 match，**新增可绑定命令必须在这里补一行**）展开，合并 `config.toml [keybindings]` 的用户覆盖（校验 + 规范化 + 去重；**同 context 且共享任一键**算冲突，冲突的自定义方退回默认，循环至稳定），再追加**固定绑定**（⌘P、⇧⌘P、复制为 HTML、光标历史、缩放、⌘1-9 切标签）。
- `install_keybindings` 装键位的同时写一份 `EffectiveShortcuts` Global——**菜单与工具栏显示的快捷键只读它**，用户改绑后显示跟随。
- **命令注册表** `COMMANDS`（`src/commands.rs`）是系统菜单与命令面板的**唯一事实源**（`CommandSpec { id, 归属菜单, label 闭包读 i18n, action }`），有守卫测试。命令面板专有两档（Edit/Format）只进面板不进菜单栏，其 id 必须与键位表同名。
- 文件菜单的「打开文件 / 打开文件夹 / 打开最近」连续排列。Windows 的偏好设置和检查更新由 `hamburger_root_commands` 提供一级动作；macOS 保留应用菜单，帮助菜单只列关于。欢迎页保留两个主按钮，最近目录显示路径并支持单条移除，移除只更新历史记录。
- **快捷键的触发范围**：要让某个键"窗口有焦点就能按"，需要键位 `context: None` **且**动作有 App 级全局监听（`cx.on_action`）——两条缺一，焦点不在正文块上时按键会静默丢失。判据仍只有处理函数一处（如 ⌘/Ctrl-B 的"空选区开合侧栏 / 有选区加粗"全在 `Editor::on_bold_capture`）。
- **派发机制三条**（vendor/gpui，改键盘/面板相关代码前先读）：
  1. **焦点句柄不在当前渲染帧内时，按键派发回退到根节点**——挂在编辑器根元素上的 `on_action`/`capture_action` 整组收不到。所以收起侧栏要把焦点交还正文。
  2. **`App::on_action` 全局监听只在冒泡阶段回调**，且窗口 capture/bubble 任一阶段 stop_propagation 后就不再执行；它是唯一不受焦点状态影响的动作通道。
  3. **派发中的窗口还在 `window_update_stack` 上**，`WindowHandle::update` 与走 `cx.active_window()` 的助手取不到它；全局监听里要操作发键窗口必须 `cx.defer` 延后一拍（退出流程同此手法）。
  另外：`Window::dispatch_action` 读的是**上一帧**渲染出的派发树——面板输入框持有窗口焦点时块那一层不在派发路径上，所以面板里执行块级动作要靠编辑器层的捕获处理者接手（判定 `runtime_context.rs:block_focus_is_live`）。当场把焦点指回块不管用（块节点要等一次重绘才可寻），`window.on_next_frame` 在本仓测试里不触发。
- **一条命令的键位仪式有八处**（漏一处就是改了不生效）：`actions!` 声明、`ShortcutCommand` 枚举、`SHORTCUT_DEFINITIONS` 条目、`key_binding_for` 分支（都在 `components/actions.rs`）· 偏好页那一列的标签（`config/preferences/pages_shortcuts_window.rs`）· 编辑器层处理者（`editor/format_ops.rs` 等）· 渲染里的注册（`editor/render/paint.rs` 那串 `capture_action`）· 菜单行的快捷键（`context_menu/document_menu.rs`）。四处入口（键位、正文右键菜单、选中工具栏、命令面板）汇到同一组函数，可用判定也只有一份。

## 10. 配置 / 主题 / i18n

- **配置根目录**：`VeloraConfigDirs::from_system`（`directories::ProjectDirs`）；子项有语言包目录、主题目录、`.history`、最近列表、`config.toml`、恢复快照目录。**测试必须用 `override_test_config_root` 独占目录**（线程局部 RAII），否则读写 config.toml 的用例互相污染（典型症状：单独跑绿、整套跑红）。
- `config.toml`：启动/语言/主题/导出/快捷键/editor 段。**运行时镜像是 `EditorSettings` Global**（渲染路径零磁盘 IO）；setter 同时更新全局与持久化。窗口 frame 存取也在这里。
- `session.json`：根、标签、活动标签、侧栏宽度、侧栏页签与开关（**页签键是落盘契约，不许改文案**）。结构性标签变化时后台写，带全局锁。
- `recovery/*.json`：恢复快照（id、来源路径、内容），原子写；autosave 周期落盘脏标签。
- **主题**（`src/theme/`）：`Theme = { name, colors, dimensions, typography, placeholders }`，serde 带 fallback 合并（旧字段缺失不炸）。内置主题 + `system`（跟随窗口外观）。`ThemeManager` Global 启动时从主题目录加载自定义主题，`current_arc()` 给渲染热路径 O(1) 拿 Arc。`Editor::render` 每帧 clone 一次并应用字体偏好与缩放（缩放是整套字号乘系数）；派生主题共享，只在度量输入变化时失效。token 全表见 [../theme-tokens.md](../theme-tokens.md)。
- **i18n**（`src/i18n/`）：`I18nStrings` 是**代码不是数据文件**——结构体字段 + `zh_cn()` + `en_us()` 三处都要登记（漏一处编译或解析出错）；外部 JSON(C) 语言包从语言目录加载并 fallback 补全（支持剥注释）。`I18nManager` Global 提供 `strings()` / `strings_arc()`。菜单文案走 `CommandSpec.label` 闭包。

## 11. 窗口 chrome（src/window_chrome.rs）

`render_custom_titlebar` 是泛型（Editor 与偏好设置窗共用）；macOS 全屏返回 None。macOS 用原生红绿灯 + 一段**不可压缩**的保留空档（否则标签溢出时布局会把空档挤掉、首个标签滑进红绿灯下面）。Windows 自绘三键 + 最左汉堡菜单钮。Linux/FreeBSD 用 client decorations + 自绘按钮，左右分布读 gsettings。拖拽区分两种策略：平台命中测试（Windows）与显式移动请求（macOS、Linux 自绘标题栏）。macOS 同时关闭窗口的默认移动（`is_movable: false`）与内容视图的鼠标透传；仅关闭透传时 AppKit 仍会让标签与窗口同时移动。标题与空白填充区显式发起移动，后端仅在该次原生拖窗期间临时允许移动。**标签条并入标题栏**：有标签时标题让位，条后补一段拖拽填充；无标签走居中的文档标题。

## 12. 陷阱

- 新增浮层要同时过三关：选中工具栏的互斥清单、`dismiss_contextual_overlays`（如属瞬态）、Esc 链。
- `stale_previews` 的计算时机、重载后的 `attach_file_origin`、跳过目录判据单一、大纲缓存键含围栏态、命中表键含根块数——这五处都是"少一步就静默错"的地方，改动前先读对应小节。
- 面板列表行高必须与开窗用的固定行高一致。
- 后台任务存进字段或 map：`Task` 被 drop 就取消。
- 零宽搜索命中出口一律过字符边界闸门，否则替换路径 panic。

## 13. 测试落点

`editor/workspace/tests/`（16 个主题文件）：标签开关与预览与会话恢复、阅读现场交还、文档内查找导航与焦点交还、命中表缓存键与序对位、工作区搜索主行为、跨块命中裁段、搜索封装（零宽/跨行/边界）、四处消费方随机一致性、外部改动重载与冲突、自动保存不覆盖外部改动、面板切换与开窗与拖宽、树内新建与重命名、树文件操作、扫描与排序与名单编码。浮层与命令另有 `editor/tests/document_context_menu.rs`、`selection_toolbar`、`palette_commands`、`commands::tests`（注册表与键位表同源、每条菜单命令有处理者）。
