# Velora Architecture Overview（总览）

> 面向后续 agent 的代码导览。写作于 2026-09-28，基于 `perf` 分支；2026-09-30 随「巨型文件拆分」重构更新模块地图。**docs/ 下的历史文档可能过期，以本目录 + 代码为准。**
> 行号会漂移，函数名不会——引用以 `文件:函数名` 为主。
> 分册：[editor-core.md](./editor-core.md)（文档模型/编辑/undo/持久化） · [render-pipeline.md](./render-pipeline.md)（渲染/虚拟化/缓存） · [workspace-ui.md](./workspace-ui.md)（工作区/配置/主题/命令） · [testing-and-build.md](./testing-and-build.md)（测试/基准/构建/vendored 补丁） · [performance.md](./performance.md)（性能基线与优化台账）

## Velora 是什么

原生 Markdown 编辑器（对标 Typora/Obsidian），Rust + **vendored GPUI 0.2.2**（`[patch.crates-io]` 指向 `vendor/gpui`，带 5 组本地补丁，见 testing-and-build.md §6）。单 bin crate（~95k 行（含测试），`src/main.rs`），无 workspace。发版 macOS + Windows（交叉编译 `releasewin`）。

### 文件组织约定（2026-09-30 重构后）

除两个已声明的例外（`editor/render/paint.rs` 的单函数 `Render::render`、`block/render/paint_parts.rs` 的单函数 `Element::paint`），**所有源文件 ≤1000 行**。大模块一律按「`foo.rs` 根 + `foo/` 子目录」拆分：根放类型定义与模块声明，子文件按职责承载 `impl` 块；测试统一放 `<name>/tests.rs` 或 `<name>/tests/` 子目录（`mod tests;` 挂载），不再内联在源文件尾部。跨子模块共享的自由函数在根以 `pub(super) use child::*` 聚合导出。

## 顶层模块地图

```
src/
├── main.rs                 启动、资产内嵌、startup timing
├── app_menu.rs             菜单/动作分发根（app_menu/build_menus.rs 构建、command_menus.rs 命令条目、dispatch.rs 动作路由、tests.rs）
├── commands.rs             命令注册表（菜单+命令面板唯一事实源）
├── window_chrome.rs        自绘标题栏/红绿灯/客户端装饰
├── components/
│   ├── actions.rs          gpui actions + 快捷键定义（tests 在 actions/tests.rs）
│   ├── block/              ★ 块实体
│   │   ├── state.rs            块记录（tests 在 state/tests.rs）
│   │   ├── runtime/            缓存与运行时：mod.rs(编辑模式/缓存结构) normalize.rs(快捷前缀) text_ops.rs(类型转换/undo) cursor.rs(光标/选区投影) display_cache.rs(渲染缓存/链接脚注注册) projection.rs+projection/build.rs(行内投影) code.rs image.rs table.rs tests/（9 个主题文件）
│   │   ├── render.rs+render/   外观：render.rs(常量/tooltip) parts.rs html 样式 inline_visuals.rs content.rs shell.rs paint_parts.rs(Render/Element 主体★单函数例外) link_cursor.rs tests.rs
│   │   └── element.rs+element/ 文本元素：element.rs(布局函数) code_input.rs text_element.rs tests.rs
│   ├── markdown/           行内解析 inline.rs+inline/{tree,parse,links,delimiters,stacks}.rs；HTML html.rs+html/parse.rs；表格 table.rs+table/tests.rs；图片 image.rs+image/tests.rs；代码高亮/链接/脚注/粘贴
│   └── latex/ mermaid/     公式与图表渲染（SVG 路径）
├── editor/
│   ├── mod.rs              ★ Editor 实体与常量（分块预算等）
│   ├── document.rs+document/   ★ markdown→块 导入：document.rs(类型) parse.rs(逐行扫描) import.rs blocks.rs(分块构建) document/tests/（6 个主题文件）
│   ├── tree.rs             ★ DocumentTree + 可见快照 + PendingTail（tests 在 tree/tests.rs）
│   ├── render.rs+render/   ★ 每帧渲染：render.rs(行计划类型/菜单几何) sync.rs(标题栏/焦点/滚动同步) menu_render.rs(窗口内菜单) overlays.rs(欢迎页/浮层/对话框) paint.rs(Render 主体★单函数例外) tests.rs
│   ├── events.rs+events/   块事件总线根：events.rs(按键捕获/焦点) paste.rs(粘贴/图片) quotes.rs structural.rs(结构性换行) scroll_mouse.rs table_nav.rs block_event.rs(事件分发)；events/tests/（6 个主题文件）
│   ├── selection.rs / source_mapping.rs  跨块选区(tests 在 selection/tests.rs) / 源码↔块偏移映射
│   ├── history.rs          全文快照式 undo/redo
│   ├── persistence.rs      原子写/autosave/外部修改检测
│   ├── workspace.rs+workspace/ ★ 侧栏根(460 行)：tabs session_watcher prompts context_menus sidebar documents tree_sync find_replace tree_ops render_{tabs,panel,tree,search} search_backend file_tree overlay_input input_handler；workspace/tests/（7 个主题文件）
│   ├── tests.rs+tests/     编辑器测试入口：common.rs 共享脚手架 + 20 个主题文件（加载/滚动/选区/保存/菜单/导出/表格/图片/脚注/undo/键盘/性能/长行/知识面板等）
│   └── status_bar.rs / quick_open.rs / command_palette.rs / modal.rs / context_menu.rs+context_menu/ / watcher.rs / file_drop.rs / close.rs / window_state.rs(tests 同级) / table_edit.rs / runtime_context.rs / workspace_index.rs+workspace_index/ / file_history.rs
├── config/                 config.toml / session.json / recovery；preferences.rs+preferences/{persistence,window,widgets,pages_general,pages_shortcuts_window,render,tests}.rs
├── theme/                  theme.rs+theme/{de,presets,manager_parts,tests}.rs：token 结构 / JSON 兼容反序列化 / 六套内置主题 / 全局管理
├── i18n/                   mod.rs + strings.rs de.rs de_impl.rs strings_api.rs keys.rs pack.rs manager.rs tests.rs：双语+外置语言包
├── net/                    HTTP 客户端与更新检查
└── export/                 html.rs+html/{css,tests}.rs / pdf / png / 打印（Chromium 无头）
vendor/gpui/                vendored 框架 + 本地补丁
benches/ scripts/ tests/fixtures/
```

## 五条主干流

1. **启动**：main → 全局初始化（配置/i18n/主题/HTTP/keybindings）→ 恢复会话或打开参数目标 → 菜单延迟安装。见 workspace-ui.md §1。
2. **打开文件**：树点击/⌘P/最近列表 → `open_workspace_file` → 按扩展名分流：markdown 走 `build_root_blocks_from_markdown`（分块渐进，`FIRST_CHUNK_ROOTS=2000`/`STEADY_CHUNK_ROOTS=250`）；**代码/纯文本整文件变单 CodeBlock（Source 模式）**。见 editor-core.md §2。
3. **编辑**：按键 → 焦点 Block input handler → `replace_text_in_visible_range` 改 `record.title` → `BlockEvent::Changed` → Editor `mark_dirty`（revision+autosave）+ undo 落栈 + 引用敏感块刷新运行时。序列化惰性。见 editor-core.md §3-4。
4. **渲染**：`Editor::render` → 可见快照 + 折叠过滤 → `rendered_window`（±800px overdraw）→ 窗口内行建真实元素、屏外行 spacer → `BlockTextElement`（build_text_runs → shape_text → paint，写回 last_layout）。见 render-pipeline.md。
5. **持久化**：autosave 防抖后台（恢复快照+临时文件）→ 原子 rename；外部修改经内容哈希检测 + notify watcher。见 editor-core.md §6。

## 关键不变量（改代码前必读）

1. **禁止 Editor update 重入**：实体 update 内不得再 `editor.update()`（历史 coredump）。
2. **块文本只存 `InlineTextTree`**：定界符不落盘，序列化时按确定性规则重建；Raw 类块（frontmatter/HTML/数学/mermaid/不支持的构造）用 `raw_fallback` 逐字保留。
3. **往返无损**：markdown 导入→序列化必须逐字节还原（有守卫测试；分块导入与单遍导入等价也有测试）。
4. **行尾与编码形状记在缓冲区**：`TextBuffer` 的 `FileShape` 存原文件的行尾/编码，落盘按它重编码；没编辑过的文档 `file_bytes()` 就是打开时那份原始字节。
5. **`document_revision` 是缓存键**：大纲跟随、长块提示等按它失效；渲染期跨实体写字段安全（不自动 notify），需要重绘必须显式 `cx.notify()`。
6. **GPUI 限制**：svg 不继承父 div text_color；`.id()` 后变 Stateful 类型（if/else 分支需 into_any_element 统一）；TextRun 逐段字号是本地补丁（Linux 未接）。
7. **所有提示用应用内模态**，禁系统原生弹窗（源码审计测试守卫）。
8. **cargo build 与 cargo test 都要过**（test-only 代码只在测试构建存在）。

## 当前性能要点

- 大文档（markdown）已具备：渐进导入、可见快照、视口窗口化（±800px）、stride 学习、块级渲染缓存。
- 已测瓶颈（dev 基线，2026-09-28）：**纯文本整文件单块**（10MiB log 稳态 8.7s/帧）、**shape_text 每帧重跑所有挂载块**、**每帧全量 `visible_blocks().to_vec()` + 折叠过滤**。数据与优化台账见 performance.md。
