# Velora 架构总览

> 面向后续 agent 的代码导览。基于 `dev`，2026-10-14 重写。**与代码冲突时以代码为准。**
> 引用位置一律用 `文件:函数名` 或 `文件:类型名`——行号会漂，名字不会。
> 本册只讲骨架与判据；细节在分册，别在这里堆字段清单。

## 分册地图

| 分册 | 覆盖 | 什么问题查它 |
|---|---|---|
| [editor-core.md](./editor-core.md) | 缓冲区、块树、导入解析、写回、位置换算、undo | 内容不对、字节被洗、光标落点错、撤销错、大文档卡 |
| [markdown-model.md](./markdown-model.md) | 行内模型与解析/序列化、表格/HTML/图片/公式/mermaid、代码高亮、导出 | 往返保真、写法被改、样式丢、富媒体渲染、导出与界面不一致 |
| [render-pipeline.md](./render-pipeline.md) | 帧入口、行计划、虚拟化、块渲染分派、文本元素、缓存 | 画面不对、滚动跳动、掉帧、缓存不失效 |
| [workspace-ui.md](./workspace-ui.md) | 启动、标签、文件树、侧栏、搜索、浮层、事件路由、命令与键位、配置/主题/i18n | 面板/菜单/快捷键/搜索/标签/窗口行为 |
| [testing-and-build.md](./testing-and-build.md) | 测试组织、门禁、夹具、CI、打包、vendored gpui 补丁 | 怎么跑测试、怎么出包、升级 gpui 要重放什么 |
| [performance.md](./performance.md) | 计数器、闸门、成本结构、优化套路 | 性能改动前后怎么量、哪些路径禁止全文操作 |
| [../theme-tokens.md](../theme-tokens.md) | 主题 token 全表 | 加/改颜色与尺寸 |

## Velora 是什么

原生所见即所得的 Markdown 编辑器。Rust + **vendored GPUI 0.2.2**（`Cargo.toml` 的 `[patch.crates-io]` 指向 `vendor/gpui`，带本地补丁，清单见 testing-and-build.md §7）。**单 bin crate、无 workspace**：全部代码在 `src/`，入口 `src/main.rs`，测试全是 bin 内的 `#[cfg(test)]` 模块。发布平台 macOS（ARM64 + Intel）与 Windows x64，不支持 Linux。

## 核心分层与所有权

理解这个仓库只需要抓住一条主线：**内容只有一个事实源，其余全是投影。**

```
磁盘字节
  └─ encoding::load_document        唯一读盘漏斗（解码 + 形状嗅探，判不出就拒绝打开）
       └─ TextBuffer                ★ 唯一内容事实源（LF 规范文本 + FileShape 记原形状）
            └─ DocumentTree         投影：块树（roots: Vec<Entity<Block>>）
                 └─ Block/BlockRecord  每块缓存自己那段文本的解析结果 + 写法账
                      └─ InlineTextTree 块内行内模型（定界符不落盘）
```

- **写只有一条路**：`TextBuffer::edit`。任何"改文档"最终都要变成对缓冲区某个字节区间的编辑。
- **块树可以从缓冲区重建**，反过来不行——所以**禁止**"从块树序列化出全文当文档内容"。保存、版本号、搜索、大纲、导出一律读缓冲区。
- **块与文档的唯一对应关系是字节区间**：`DocumentTree::source_spans`（`EntityId → Range<usize>`，表在树侧不在块上）。编辑落笔、位置换算、增量重算全靠它。
- **块的文本是缓存不是内容**：`record.title` 的语义是 `parse(buffer[span])` 的结果。

派生视图（大纲、搜索命中、状态栏统计、行计划）的通用规矩：**只重算被改过的字节所在的那部分**，成本必须接进计数器（见 performance.md）。

## 顶层模块地图

```
src/
├── main.rs              启动、资产内嵌、startup timing
├── app_identity.rs      reverse-DNS 应用标识（窗口与打包共用）
├── file_url.rs          平台打开事件的文件 URL 解析；全仓唯一 percent-decoding 判据
├── commands.rs          命令注册表 COMMANDS（系统菜单 + 命令面板的唯一事实源）
├── app_menu.rs + app_menu/   菜单构建、命令条目、动作路由
├── window_chrome.rs     自绘标题栏 / 窗口按钮 / 拖拽区 / 标签条并入
├── components/
│   ├── actions.rs       gpui 动作声明 + 快捷键表 + 键位安装
│   ├── block/           ★ 块实体：state(记录) runtime(缓存/编辑) render(外观) element(文本元素) interactions(输入)
│   ├── markdown/        ★ 行内模型与解析/序列化、表格、HTML、图片、代码高亮、脚注、链接、粘贴
│   ├── latex/ mermaid/  公式与图表 → SVG（进程 + 磁盘双层缓存）
│   └── menu.rs switch.rs 共用 UI 组件
├── editor/
│   ├── mod.rs           ★ Editor 实体（窗口级控制器）与写回分档、区间维护
│   ├── buffer.rs + buffer/  ★ TextBuffer 与 FileShape（编码/行尾/BOM）
│   ├── encoding.rs      读盘漏斗
│   ├── tree.rs          ★ DocumentTree、可见快照、区间表、序列化
│   ├── document.rs + document/  ★ markdown → 块的手写逐行解析器
│   ├── source_mapping.rs ★ 块内偏移 ↔ 缓冲区字节的全部换算
│   ├── history.rs       增量组 undo/redo
│   ├── persistence.rs   原子写、autosave、外部修改检测
│   ├── render.rs + render/  ★ 每帧渲染、行计划、滚动同步、浮层绘制
│   ├── events.rs + events/  按键捕获、块事件总线、粘贴、滚动与鼠标、表格导航
│   ├── selection.rs + selection/  跨块选区、指针命中、表格跨格选区
│   ├── workspace.rs + workspace/ ★ 侧栏：标签、文件树、大纲、搜索、会话与监听
│   └── 单文件：status_bar quick_open command_palette modal context_menu/ selection_toolbar
│              table_edit format_ops paragraph_ops insert_ops clipboard_ops window_state
│              runtime_context workspace_index file_history image_preview formula_editor
│              latex_completion watcher file_drop close update
├── config/              config.toml / session.json / recovery 快照 / 偏好设置窗口
├── theme/               token 结构、JSON 反序列化、内置主题、全局管理
├── i18n/                双语内置表 + 外置语言包（i18n 是代码不是数据文件）
├── net/                 HTTP 客户端与应用更新检查
└── export/              HTML / PDF / PNG / 打印（PDF、PNG、打印走无头 Chromium）
vendor/gpui/             vendored 框架 + 本地补丁
scripts/ fixtures/       夹具生成、打包脚本；fixtures/perf 不进版本库
```

文件组织约定：大模块一律「同名 `foo.rs` 根 + `foo/` 子目录」，根放类型定义与模块声明，子文件承载 `impl`；测试统一放 `<name>/tests.rs` 或 `<name>/tests/`，不内联在源文件尾部。新目录不要再新增 `mod.rs`。

## 主干流

**打开**：`encoding::load_document` → `Editor::from_loaded_document` → LF 归一进 `TextBuffer` → 切行 + 前瞻索引 → `build_root_block_chunk` 建首屏根块（预算见 editor-core.md §4）→ 换算并写入区间表 → 剩余行挂 `PendingTail`，前台任务分批续建。代码/纯文本文件不分块解析，按固定行数切片成 source-raw 块并进 Source 视图；图片走只读预览，二进制不进缓冲区。

**编辑**：按键 → 焦点块的 input handler 改 `record.title` → `BlockEvent::Changed` → `Editor::on_block_event` → 开撤销组 → **按精度分档写回缓冲区**（细→粗，逐档失败才降级，兜底是整篇重投影）→ 平移区间表 → 必要时区域重投影 → 引用敏感块在写回**之后**刷新运行时 → 批处理末尾结算撤销组。

**渲染**：`Editor::render` → 可见快照 + 折叠过滤 → 行计划（按结构版本缓存）→ 视口开窗（行高前缀索引 + overdraw）→ 窗口内行建真实元素、屏外行 spacer → `BlockTextElement` 的 request_layout/prepaint/paint（shape 结果跨帧备忘，paint 末尾把布局写回块供命中测试用）。

**保存**：`document_bytes_for_save` = 缓冲区字节（未编辑过就是打开时那份原字节）→ 临时文件 + fsync + rename 原子写 → 校验磁盘版本 → 更新版本号、清恢复快照。autosave 是防抖后台任务，写恢复快照 + 临时文件，回主线程校对修订号后提交。

**导出**：markdown → HTML 的块结构一律由 pulldown-cmark 认定，公式/提示框/图片内联/标题 id/目录都在它的**事件流**上替换（见不变量 10）。PDF/PNG/打印在 HTML 之上走无头 Chromium。

## 关键不变量（改代码前必读）

1. **禁止 Editor 重入更新**：实体 `update` 闭包内不得再 `editor.update()`（历史上因此 coredump）。同一实体也不能边 `read` 边 `update`。
2. **缓冲区是唯一事实源**，块树是投影；`TextBuffer::edit` 是唯一写入口，且每次用户可见的 edit 必须落在一个开着的撤销组里。
3. **往返无损**：markdown 导入 → 序列化必须逐字节还原未编辑部分。定界符不落盘，但**写法落盘**（强调记号、列表记号、转义、原始源码）。
4. **规范化只住一条命令**：Setext→ATX、`__`→`**`、`1)`→`1.`、表格列宽重排只许 `format_document` 做；打开、打字、保存、撤销、重投影一律保原写法。
5. **行尾与编码形状记在缓冲区**（`FileShape`），落盘按它还原。能解码就必须能按同一种编码写回；判不出来的字节拒绝打开，不以乱码打开再写回。字节表（BOM 等）只准存在一处。
6. **文档级坐标一律用缓冲区字节偏移**，不要再引入"序列化文本里的偏移"。来自旧内容的偏移进新内容前必须过 `floor_char_boundary`。
7. **`document_revision` 是缓存键**；跨文档缓存还要带 `TextBuffer::identity`（两份未编辑文档 revision 同为 0）。渲染期跨实体写字段安全（不自动通知），要重绘必须显式 `cx.notify()`。
8. **按键路径禁止全文操作**：整篇序列化、整篇映射重建、整棵快照重排、大纲整篇重扫、逐块读实体，全部有计数器与闸门测试盯着（performance.md）。
9. **一条语法只准有一处判据**：编码/BOM 字节表、可转义字符集、图片路径解析、表格竖线切分、标题分隔符这类"口径"被抄出第二份就必然漂移，而报修会以**下游消费者**的面貌出现。动手前先 grep 现成的那一处（单点清单见 markdown-model.md §9）。
10. **导出块结构只算一份**：markdown → HTML 的块结构由 pulldown-cmark 认定，改写只在它的事件流上；不允许行扫描重猜块结构。唯一例外是**字节等价的词法归一**（同字节数、偏移不动、判据只借 `BlockKind` 解析器一处、字面区取自 pulldown 自己的事件流），见 `heading_separators_for_parse`（src/export/html.rs）。
11. **一个动作只有一处实现**：快捷键、正文右键菜单、选中工具栏、命令面板四条入口汇到编辑器层同一组函数，可用判定也只有一份。新增命令要四条一起有，键位仪式八处见 workspace-ui.md §9。
12. **所有提示用应用内模态**，禁系统原生弹窗（源码审计测试守卫）。
13. **GPUI 限制**：svg 不继承父 div 的 text_color；`.id()` 后元素变 Stateful（if/else 分支需 `into_any_element` 统一）；`TextRun` 逐段字号是本地补丁；焦点句柄不在当前渲染帧内时按键派发回退根节点（收起面板要把焦点交还正文）；`is_focused` 判不出窗口切走，画光标要加 `is_window_active`。
14. **`cargo build` 与 `cargo test --bin velora` 都要过**：vendored gpui 里有只在测试构建存在的代码，只跑测试会漏掉生产构建错误。

## 定位索引：要改 X 去哪

| 要动的东西 | 入口 |
|---|---|
| 某种 markdown 语法认不出/认错 | `src/editor/document/parse.rs` 的识别函数族 + `document/import.rs:build_blocks_from_lines_with_lookahead` 的分发顺序 |
| 块种类与它的判据 | `src/components/block/state.rs:BlockKind`（解析判据与 `markdown_line` 序列化都在这） |
| 行内样式/强调/链接/脚注/行内公式 | `src/components/markdown/inline/`（解析 parse.rs、定界符规则 delimiters.rs、序列化选栈 stacks.rs） |
| 打字后字节写错地方 | `src/editor/mod.rs` 的 `write_back_*` 分档 + `src/editor/source_mapping.rs` 的换算族 |
| 光标/选区落点 | `selection.rs:mapping_source_offset`（插入点口径）与 `mapping_source_range_endpoint`（区间端点口径）——两套口径不能混 |
| 撤销行为 | `src/editor/history.rs`（开组/记账/结算/重放） |
| 表格编辑与格子定位 | `src/editor/table_edit.rs`（写侧）+ `source_mapping.rs:push_table_*`（读侧），两侧共用同一把尺 |
| 编码/行尾/BOM | `src/editor/buffer/file_shape.rs`（唯一字节表）+ `src/editor/encoding.rs`（唯一漏斗） |
| 保存/自动保存/外部修改冲突 | `src/editor/persistence.rs`；冲突模态在 `workspace/documents.rs` |
| 打开文件/标签切换/阅读现场 | `workspace/tabs.rs`、`workspace/documents.rs`、`file_drop.rs:replace_document_content` |
| 文件树/扫描/重命名 | `workspace/file_tree.rs`、`workspace/tree_sync.rs`、`workspace/tree_edit.rs` |
| 搜索（工作区 / 文档内） | `workspace/find_replace.rs`（调度）、`search_backend.rs` + `search_engine.rs`（工作区）、`document_matches.rs`（文档内命中表）、`tree_sync.rs:sync_document_search_highlights`（画进块） |
| 大纲 / `[TOC]` | `workspace/tree_sync.rs:sync_workspace_outline`（增量缓存 `OutlineSegment`） |
| 每帧渲染 / 滚动 / 视口开窗 | `src/editor/render/paint.rs`（`impl Render for Editor`）、`render.rs`（行计划类型）、`window_state.rs:rendered_window_*` |
| 单块外观 | `src/components/block/render/paint_parts.rs`（kind 分派）+ `render/shell.rs`（公共壳） |
| 文本绘制 / 命中测试几何 | `src/components/block/element/text_element.rs` + `element.rs` |
| 块内缓存不失效 | `src/components/block/runtime/display_cache.rs:sync_render_cache`（失效枢纽） |
| 代码高亮（围栏块 / 源码视图） | `markdown/code_highlight.rs`（tree-sitter）/ `markdown/source_highlight.rs`（手写扫描器） |
| 公式、mermaid | `src/components/latex/`、`src/components/mermaid/` |
| 图片路径/尺寸/加载 | `markdown/image.rs:resolve_image_source`（唯一路径口径）+ `block/runtime/image.rs` |
| HTML 块渲染与安全 | `markdown/html/parse.rs`（白名单与分类）+ `block/render/content.rs:render_html_node` |
| 导出 HTML/CSS/PDF/PNG/打印 | `src/export/`（口径见 markdown-model.md §8） |
| 新命令/快捷键/菜单条目 | `components/actions.rs` → `commands.rs` → `app_menu/` → 编辑器层处理者 → `context_menu/document_menu.rs` → `selection_toolbar.rs`（八处，见 workspace-ui.md §9） |
| 右键菜单 / 选中工具栏 / 命令面板 / ⌘P | `editor/context_menu/`、`selection_toolbar.rs`、`command_palette.rs`、`quick_open.rs`；层级与互斥见 workspace-ui.md §7 |
| 偏好设置窗口 | `src/config/preferences/` |
| 主题 token | `src/theme/theme/`（结构在 theme.rs，反序列化在 de.rs，内置主题在 presets.rs）+ `docs/theme-tokens.md` |
| 文案 / 双语 | `src/i18n/strings.rs`（结构 + zh_cn + en_us，三处都要登记） |
| 应用更新 | `src/editor/update.rs` + `src/net/` |
| 启动顺序 / 窗口 frame 记忆 | `src/main.rs`、`src/editor/close.rs` |
| 拖放文件 | `src/editor/file_drop.rs` |
| gpui 行为异常（焦点、派发、shaping、Windows 对话框） | 先查 testing-and-build.md §7 的本地补丁清单，再读 `vendor/gpui` |
