# Workspace & UI（工作区、配置、主题与命令系统）

> 面向后续 agent 的代码导览。写作于 2026-09-28，基于 `perf` 分支。
> **行号会漂移，函数名不会**。相关文档：[overview.md](./overview.md)、[editor-core.md](./editor-core.md)、[render-pipeline.md](./render-pipeline.md)、[testing-and-build.md](./testing-and-build.md)

## 1. 启动（src/main.rs）

- `main()`：解析 `-v/-h/-d(--detach)` + 文件/目录参数；macOS detach 会重新拉起自身。`Application::new().with_assets(VeloraAssets)`——SVG 图标按精确路径 `include_bytes!` 内嵌。
- `app.run` 体内顺序：Dock 图标 → `config::load_or_create_app_preferences` → `I18nManager::init_with_language_id` → `ThemeManager::init_with_theme_id` → `EditorSettings::init` → `net::install_http_client` → `components::init_with_keybindings`。**菜单延迟 16ms 安装**（首帧后计时器，G5）。
- 首窗口：无参数 → `restore_last_session`（session.json；活动标签同步开，其余每 16ms 一个）；否则 `open_startup_window`（尊重「打开上次文件」偏好，否则欢迎页）。随后 `restore_recovery_windows`（磁盘内容一致的快照跳过，否则合并/打开恢复窗口）。
- 文件参数 → `open_editor_window`（app_menu.rs）：`restored_window_bounds` 恢复记住的 frame（并把所在显示器交给平台；不在任何屏上就搬回主屏、尺寸保留）→ `Editor::from_file_source`；目录参数 → `open_workspace_window` + `set_workspace_root`。开窗后装两个监听：`force_install_close_guard`（关窗前落盘 frame）与 `install_window_frame_recorder`（拖动/缩放即记 frame，500ms 防抖后台落盘）。
- `VELORA_STARTUP_TIMING=1`：分阶段耗时到 stderr。

## 2. Workspace 是嵌入状态，不是独立实体

**没有单独的 Workspace GPUI 实体**。`Editor` 内嵌 `workspace: WorkspaceState`（src/editor/workspace.rs 为根，功能代码在 workspace/ 子模块：tabs.rs find_replace.rs search_backend.rs sidebar.rs 等）：

- `active_tab: {Files, Search, Outline}`、`root`、`file_tree: Option<WorkspaceTreeNode>`（递归 children）、`outline_tree`/`toc_entries`、`expanded: HashSet<String>`、`open_documents: Vec<WorkspaceDocumentTab>`、搜索/替换全套状态、`panel_width` 等。
- **标签是快照不是 Editor 实体**：`WorkspaceDocumentTab { path, recovery_id, file_version, markdown, dirty, preview, view }`。单个 Editor 在切换激活标签时换入换出 `DocumentTree` 内容（`snapshot_current_document` 把**缓冲区文本**存回标签——自动保存与恢复快照的内容来源就是它，取块树序列化的话一份没编辑过的文件进快照就已经被洗过一遍）。
- 打开文件流：树节点点击 → `open_workspace_file`：UTF-16 BOM/文本嗅探（`has_utf16_bom`/`is_likely_text_file`）→ 推标签 → `reveal_path_in_tree` 展开祖先 → `restore_document_from_markdown` 或 `restore_document_from_code_source`（分流见 editor-core.md §2）——标签上存着这篇的阅读现场就按它交还，本次会话没读过才按新文档从顶部与渲染态起步（见 editor-core.md §6）→ 调度 autosave + `persist_session`。现场只活在这一进程里，不写进会话文件。
- `set_workspace_root`：canonicalize、按根恢复侧栏宽、剪枝根外标签、启动 watcher、持久化会话。
- **预览（临时）标签**：`WorkspaceDocumentTab.preview` 为 true 时斜体显示，且同一时刻只留一个——`open_workspace_file_in_mode(…, Preview)` 在开新篇后销毁其它**干净**的预览标签。入口分两类：浏览型（工作区搜索结果行、文档内查找、⌘P 快速打开、正文本地链接）走 Preview，文件树用 `tree_click_open_mode`（单击 Preview、双击/键盘 Pinned）。转正点只有一个：`finish_dirty` 里 `document_dirty` 由 false 变 true 的那次调用 `pin_active_preview_tab`，编辑过的预览不再被替换掉，切走也留着。因此 `stale_previews` 必须在 `snapshot_current_document` **之后**算——活动标签的 `dirty` 只在那一步写回，早算刚编辑过的预览仍记为干净，会被当场销毁。
- **未保存标记**：`dirty` 的标签（含活动那一篇）在标题前渲染 7px 实心圆点（`document-tab-dirty-{index}`）；判定与自动保存开关无关，只看有没有落盘。

## 3. 文件树与监听

- `scan_workspace_dir`：递归 `fs::read_dir`，跳过 `.git/target/node_modules/.worktrees/dist`；分类 Markdown/Code/Other；排序（名称/时间/类型，`TreeSortPreference`）。
- **异步扫描**：`sync_workspace_file_tree_inner` 用 `cx.background_spawn` + 代数计数器（`tree_scan_generation`），过期结果丢弃；过滤模式渲染扁平命中列表（≤50）。
- **监听**（src/editor/watcher.rs）：notify 递归 watcher，Modify/Create/Remove 事件经 mpsc 泵到 `on_watched_path_changed` → `reload_externally_changed_document`（只重载干净标签，重载走 `ImportKind::Restore`：模式、视口、光标都不动，见 editor-core.md §6）。
- **树内名称编辑**（workspace/tree_edit.rs）：新建普通文件、Markdown 文件、文件夹与重命名共用单行输入，复用 Editor 的 IME 路由；Markdown 默认 `untitled.md` 并选中文件名部分，后缀可改。Enter 确认、Esc 取消，名称不可包含路径分隔符；I/O 在后台执行，重名在输入行提示，其它错误送应用内模态。重命名同步更新打开标签、活动路径与图片基目录，并保留未保存内容。
- **树的选择与正文焦点独立**：右键直接选中并聚焦文件树，菜单保存点击时的目标，重绘和其它文档激活不能把菜单操作转移到活动文档。文件树菜单提供在文件管理器中打开、复制绝对路径、相对路径和文件名，F2 在树焦点下重命名。

## 4. 命令/动作系统

- 动作声明：`actions!(velora, [...])`（src/components/actions.rs）；带载荷动作（选主题/语言/标签页/最近文件）走 `#[action(namespace = velora)]`。
- `SHORTCUT_DEFINITIONS`：每条命令 id/分类/默认键/`context: "BlockEditor"`；`resolved_keybindings` 合并 config.toml `[keybindings]` 覆盖，再追加固定绑定（⌘P/⇧⌘P/⌘1-9/缩放/光标历史）。
- **命令注册表**（src/commands.rs）：`CommandSpec { id, CommandMenu, label: fn(&I18nStrings), action }`，静态 `COMMANDS` 表是**菜单与命令面板的唯一事实源**（有守卫测试）。
- 菜单（app_menu/）：`build_menus`（app_menu/build_menus.rs，macOS 六菜单；非 macOS App 并入 File）→ `install_menus`（app_menu.rs）；~30 个 `cx.on_action` 处理者 → `dispatch_menu_action`（if/else 链，有源码扫描守卫）。退出走 `cx.defer`（让应用内未保存对话框工作）。

## 5. 配置层（src/config/）

- 根目录：`VeloraConfigDirs::from_system`（`directories::ProjectDirs`，app.velora.velora）；子项 `languages_dir`/`themes_dir`/`.history`/recent 文件/`config.toml`/`recovery_dir`。测试用 `override_test_config_root` 重定向。
- **config.toml**：启动/语言/主题/导出/快捷键/editor 段；运行时镜像是 `EditorSettings` Global（渲染路径零磁盘 IO）； setter 同时更新全局 + 持久化。窗口 frame 存取 `saved_window_frame`/`store_window_frame`。
- **session.json**：`SessionState { root, tabs, active, sidebar_width }`；`persist_session` 在每次结构性标签变化时写；启动恢复 + 按根侧栏宽恢复。
- **recovery/*.json**：`RecoverySnapshot { id, source_path, markdown }`，原子写；autosave 周期落盘脏标签。
- 最近列表：文件 20 条/文件夹 10 条；语言包导入支持 JSONC（剥注释）。

## 6. 主题系统（src/theme/）

- `Theme = { name, colors(61), dimensions(244), typography(482), placeholders }`；serde 带 fallback 合并（旧字段缺失不炸）。内置 6 主题 + `system`（跟随窗口外观，`observe_window_appearance` 驱动）。
- `ThemeManager` Global：启动从 `themes_dir` 加载自定义主题；`current_arc()` O(1) Arc 给渲染热路径。组件直接读 `theme.colors.*`/`dimensions.*`。
- `Editor::render` 每帧 clone 主题一次并应用字体偏好 + 缩放到 typography（缩放是整套字号乘系数）。

## 7. i18n（src/i18n/mod.rs，~3000 行）

- `I18nStrings`（~137 个 String 字段）内置 `zh_cn()`/`en_us()`；外部 JSON(C) 包从 `languages_dir` 加载并 fallback 补全。
- `I18nManager` Global：`strings()`/`strings_arc()`（热路径零拷贝）。
- **新增字符串的登记点**：struct 字段、zh_cn()、en_us()、（如需）catalog——漏一处会解析错误；菜单文案走 `CommandSpec.label` 闭包。

## 8. 侧栏三面板的防重算设计

- **状态栏**（status_bar.rs）：字数/阅读时长读缓冲区（= 文件内容）；长块提示按 `(document_revision, bool)` 缓存；行列号仅 Source 模式算。
- **大纲**：`sync_workspace_outline` 仅当缓冲区内容与 `outline_source` 不同才重建（比较零拷贝，`build_outline_tree` 用 pulldown-cmark，围栏代码安全）；滚动跟随高亮按字节偏移分区 + `outline_follow_cache`（按 revision 键）。
- **搜索**：`schedule_workspace_search` 代数计数 + 120ms 去抖；工作区域走缓存的树、文档域走源码，均在 background executor + catch_unwind；结果上限 200；**重搜期间保留旧结果**（防闪空白）；文档内命中经 `sync_document_search_highlights` 画进块。
- ⌘P 快速切换（quick_open.rs）：过滤 `workspace_text_files()`，上限 12，IME 输入路由经 Editor 的 input handler。
- ⇧⌘P 命令面板（command_palette.rs）：条目来自 `commands::commands()`（`Edit`/`Format` 两档只进面板、不进系统菜单栏）；按回车与点一行同一条收尾 `run_palette_command`（:100）→ `window.dispatch_action`。面板输入框持有窗口焦点，块那一层不在派发路径上（派发读的是上一帧的派发树），所以块级命令在编辑器层收口，见 editor-core.md §8。

## 9. 窗口 chrome 与覆盖层

- window_chrome.rs：macOS 原生红绿灯；Windows/Linux `WindowDecorations::Client` + 自绘 AppControls；拖拽区 `WindowControlArea::Drag`；Linux GNOME 读 gsettings button-layout；标签条并入标题栏。
- modal.rs：所有提示走应用内模态（`ModalSpec`/`show_modal`，源码审计测试禁原生弹窗）；context_menu.rs 右键菜单；quick_open/command_palette 浮层。正文右键菜单与选中工具栏两套菜单的入口、状态、置灰判定与落点见 editor-core.md §8；它们与 ⌘P/⇧⌘P 都挂在窗口根而不是滚动区里（src/editor/render/paint.rs:1062、:1067），层级上晚进树者画在上——`selection_toolbar_anchor`（src/editor/selection_toolbar.rs:111）那串「没有别的浮层」的判断因此要把每一个浮层都列进去。
- 事件路由：`on_editor_key_down_capture`（src/editor/events.rs 捕获阶段）→ 焦点块 → 覆盖层（`OverlayInputKind`）。
