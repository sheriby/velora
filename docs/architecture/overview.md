# Velora Architecture Overview（总览）

> 面向后续 agent 的代码导览。写作于 2026-09-28，基于 `perf` 分支；2026-09-30 随「巨型文件拆分」重构更新模块地图，2026-10-12 复核模块清单与计数。**与代码冲突时以代码为准。**
> 行号会漂移，函数名不会——引用以 `文件:函数名` 为主。
> 分册：[editor-core.md](./editor-core.md)（文档模型/编辑/undo/持久化） · [render-pipeline.md](./render-pipeline.md)（渲染/虚拟化/缓存） · [workspace-ui.md](./workspace-ui.md)（工作区/配置/主题/命令） · [testing-and-build.md](./testing-and-build.md)（测试/基准/构建/vendored 补丁） · [performance.md](./performance.md)（性能基线与优化台账）

## Velora 是什么

原生 Markdown 编辑器（对标 Typora/Obsidian），Rust + **vendored GPUI 0.2.2**（`[patch.crates-io]` 指向 `vendor/gpui`，带 7 组本地补丁，见 testing-and-build.md §5）。单 bin crate（~157k 行含测试，2026-10-12 实测，`src/main.rs`），无 workspace。发版 macOS + Windows（各平台原生构建 `release`）。

### 文件组织约定（2026-09-30 重构后）

除两个已声明的例外（`editor/render/paint.rs` 的单函数 `Render::render`、`block/render/paint_parts.rs` 的单函数 `Element::paint`），**所有源文件 ≤1000 行**。大模块一律按「`foo.rs` 根 + `foo/` 子目录」拆分：根放类型定义与模块声明，子文件按职责承载 `impl` 块；测试统一放 `<name>/tests.rs` 或 `<name>/tests/` 子目录（`mod tests;` 挂载），不再内联在源文件尾部。跨子模块共享的自由函数在根以 `pub(super) use child::*` 聚合导出。

## 顶层模块地图

```
src/
├── main.rs                 启动、资产内嵌、startup timing
├── app_identity.rs         应用标识（reverse-DNS app id，窗口与打包共用）
├── file_url.rs             平台打开事件的文件 URL 解析；全仓唯一的 percent-decoding 判据
├── app_menu.rs             菜单/动作分发根（app_menu/build_menus.rs 构建、command_menus.rs 命令条目、dispatch.rs 动作路由、tests.rs）
├── commands.rs             命令注册表（菜单+命令面板唯一事实源）
├── window_chrome.rs        自绘标题栏/红绿灯/客户端装饰
├── components/
│   ├── actions.rs          gpui actions + 快捷键定义（tests 在 actions/tests.rs）
│   ├── menu.rs / switch.rs 菜单共用渲染（行/分隔线/面板）、拨动开关组件
│   ├── block/              ★ 块实体
│   │   ├── state.rs            块记录（tests 在 state/tests.rs）
│   │   ├── runtime/            缓存与运行时：mod.rs(编辑模式/缓存结构) normalize.rs(快捷前缀) text_ops.rs(类型转换/undo) cursor.rs(光标/选区投影) display_cache.rs(渲染缓存/链接脚注注册) projection.rs+projection/build.rs(行内投影) code.rs image.rs table.rs tests/（9 个主题文件）
│   │   ├── render.rs+render/   外观：render.rs(常量/tooltip) parts.rs html 样式 inline_visuals.rs content.rs shell.rs paint_parts.rs(Render/Element 主体★单函数例外) link_cursor.rs tests.rs
│   │   └── element.rs+element/ 文本元素：element.rs(布局函数) code_input.rs text_element.rs tests.rs
│   ├── markdown/           行内解析 inline.rs+inline/{tree,parse,links,delimiters,stacks}.rs；HTML html.rs+html/parse.rs；表格 table.rs+table/tests.rs；图片 image.rs+image/tests.rs；代码高亮/链接/脚注/粘贴
│   └── latex/ mermaid/     公式与图表渲染（SVG 路径）
├── editor/
│   ├── mod.rs              ★ Editor 实体与常量（分块预算等）
│   ├── buffer.rs+buffer/   ★ 文档唯一事实源：分块文本 + FileShape（file_shape.rs 记编码/行尾）
│   ├── encoding.rs         读盘漏斗：原始字节/规范文本/文件形状的唯一判据（BOM/UTF-16/GB18030）
│   ├── document.rs+document/   ★ markdown→块 导入：document.rs(类型) parse.rs(逐行扫描) import.rs blocks.rs(分块构建) document/tests/（6 个主题文件）
│   ├── tree.rs             ★ DocumentTree + 可见快照 + PendingTail（tests 在 tree/tests.rs）
│   ├── render.rs+render/   ★ 每帧渲染：render.rs(行计划类型/菜单几何) sync.rs(标题栏/焦点/滚动同步) menu_render.rs(窗口内菜单) overlays.rs(欢迎页/浮层/对话框) paint.rs(Render 主体★单函数例外) tests.rs
│   ├── events.rs+events/   块事件总线根：events.rs(按键捕获/焦点) paste.rs(粘贴/图片) quotes.rs structural.rs(结构性换行) scroll_mouse.rs table_nav.rs block_event.rs(事件分发)；events/tests/（6 个主题文件）
│   ├── selection.rs+selection/  跨块选区（tests、pointer.rs 命中换算、table.rs 表格跨格）/ source_mapping.rs 源码↔块偏移映射 / selection_toolbar.rs 选中工具栏
│   ├── history.rs          增量组 undo/redo（不是全文快照）
│   ├── persistence.rs      原子写/autosave/外部修改检测
│   ├── workspace.rs+workspace/ ★ 侧栏根：tabs session_watcher prompts context_menus sidebar documents document_matches tree_sync tree_ops tree_edit find_replace search_backend search_engine search_bench render_{tabs,panel,tree,search} file_tree overlay_input input_handler；workspace/tests/（15 个主题文件）
│   ├── tests.rs+tests/     编辑器测试入口：common.rs 共享脚手架 + 42 个主题文件（加载/滚动/选区/保存/菜单/导出/表格/图片/脚注/undo/键盘/性能/长行/知识面板等）
│   └── 其余单文件：status_bar quick_open command_palette file_history image_preview formula_editor latex_completion modal context_menu.rs+context_menu/ watcher file_drop close window_state table_edit runtime_context workspace_index format_ops paragraph_ops insert_ops clipboard_ops export update
├── config/                 config.toml / session.json / recovery；preferences.rs+preferences/{persistence,window,widgets,pages_general,pages_shortcuts_window,render,tests}.rs
├── theme/                  theme.rs+theme/{de,presets,manager_parts,tests}.rs：token 结构 / JSON 兼容反序列化 / 六套内置主题 / 全局管理；token 全表见 [theme-tokens.md](../theme-tokens.md)
├── i18n/                   mod.rs + strings.rs de.rs de_impl.rs strings_api.rs keys.rs pack.rs manager.rs tests.rs：双语+外置语言包
├── net/                    HTTP 客户端与更新检查
└── export/                 html.rs+html/{css,tests}.rs / pdf / png / 打印（Chromium 无头）
vendor/gpui/                vendored 框架 + 本地补丁
scripts/ fixtures/
```

## 五条主干流

1. **启动**：main → 全局初始化（配置/i18n/主题/HTTP/keybindings）→ 恢复会话或打开参数目标 → 菜单延迟安装。见 workspace-ui.md §1。
2. **打开文件**：树点击/⌘P/最近列表 → `open_workspace_file` → 按扩展名分流：markdown 走 `build_root_blocks_from_markdown`（分块渐进，`FIRST_CHUNK_ROOTS=2000`/`STEADY_CHUNK_ROOTS=250`）；**代码/纯文本整文件变单 CodeBlock（Source 模式）**；图片走只读预览（`image_preview`，二进制不进缓冲区）。见 editor-core.md §2。
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
9. **一个动作只有一处实现**：快捷键、正文右键菜单、选中工具栏、命令面板四条入口汇到编辑器层同一组函数上，可用判定也只有一份（菜单灰掉的命令键位按下去同样不写）；新增一条命令要四条一起有，见 editor-core.md §8。
10. **导出块结构只算一份，改写只在事件流上**：markdown → HTML 的块结构（围栏、引用层级、代码段、Setext 下划线、链接地址）一律由 pulldown-cmark 认定，公式、提示框、图片内联、标题 id 与目录都在它的**事件流**上替换；不允许行扫描重猜块结构——曾经那一整族「界面正常、HTML 不对」报修就出自那里。唯一例外：**字节等价的词法归一**——手写解析器比严格 CommonMark 多认的写法（如标题分隔符的全角空格 U+3000）在进 pulldown 前原位换成等价 ASCII，要求同字节数（偏移一个不动）、判据只借 `BlockKind` 解析器一处、字面区（代码块/HTML 块）取自 pulldown 自己的事件流；块结构仍然是 pulldown 算的那一份（`heading_separators_for_parse`，src/export/html.rs）。
11. **读写两头对称**：一种编码要能解码就必须能按同一种编码写回（UTF-16LE/BE 自己拼 code unit，`encoding_rs` 的 UTF-16 `encode` 不是解码的逆操作），能不能无损读写只在 `encoding::load_document` 这一个漏斗上判一次，四个打开入口都经过它；判不出来的字节拒绝打开，不以乱码打开再写回。字节表（BOM 之类）只准存在一处。
12. **快捷键的触发范围**：要让某个键「窗口有焦点就能按」，键位要不设 `context`，且动作要有 App 级全局监听（`cx.on_action`）——两条缺一，焦点不在正文块上（侧栏树、面板收起后焦点悬空）时按键会静默丢失；焦点句柄不在渲染帧内时派发回退到根节点，挂在编辑器根部元素上的监听全收不到，所以面板收起要把焦点交还正文（`Editor::refocus_document_after_sidebar_collapse`）。判据仍只有处理函数一处（如 ⌘/Ctrl-B 的双身份全在 `Editor::on_bold_capture`）。机制细节见 workspace-ui.md §4。

## 当前性能要点

- 大文档（markdown）已具备：渐进导入、可见快照、视口窗口化（±800px）、stride 学习、块级渲染缓存、shape 布局键 memo、增量大纲。
- 已解决的旧瓶颈（2026-09-28 基线）：纯文本整文件单块（10MiB log 稳态 8.7s/帧 → 512 行分块 + 窗口化）、`shape_text` 每帧重跑所有挂载块（→ 布局键 memo）、大纲每帧整篇重扫（→ 按根块增量，见 editor-core.md §3 第 22 条）。数据与优化台账见 performance.md。
- 尚未解决：大文档每键成本仍随文档线性（可见列表重排、行计划重建的剩余部分）；表格列宽每帧重测所有单元格；每帧主题 clone 与全字号缩放。
