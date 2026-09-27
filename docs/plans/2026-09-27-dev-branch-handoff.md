# dev 分支交接文档（Handoff）

日期：2026-09-27（深夜会话结束，交由下一个 agent 接手；同日白天已续接）
分支：`dev`（领先 `main` 69 提交，全部待用户 review 后合入 main；会话期共 165 次提交，含 main 上已合入的批次）
会话目标来源：用户要求以 Typora/Obsidian 为对标，实现 50+ 项 roadmap 功能点，P0/P1 全覆盖，代码高性能、可维护、可扩展，按功能点分段提交。

---

## 一、当前状态速览

| 项 | 状态 |
|----|------|
| Roadmap 完成 | **48 / 62 项**（其中 B9/B10/D6 为 ✅(部分)）；执行发现项 4 项中 **D9 已完成**（E9/E10/A7 待办） |
| 测试 | 838 通过 0 失败（基线 831 + C7 新增 3 + D9 新增 3 + 重开用例；基线偶发项本轮通过） |
| 编译 | `cargo build` 零错误零警告（dev profile）；test 目标警告已清零（0379ae0） |
| 远程 | `origin/dev` 已推送（fb11800）；main 未动 |
| 工作树 | 干净（`?? .zcodeignore` 按用户要求**不要提交**） |
| 运行 | `cargo run`（dev profile）；debug 构建位于 `target/debug/velora` |

## 二、本会话已完成项（会话期 165 次提交，均附测试/断言）

### 主线批次（合入自 main 前的 7 个用户报修修复）
1. 标题栏 Fluent 化（40→36px，标签并入标题栏）+ Tab 关闭按钮/右键批量关闭 — d91ff73
2. inline code 85% 等宽字号 — f4d4c9c
3. 图片渲染修复（480px 限宽、紧凑失败占位、data URI、段落内联图片）— c0a1722
4. VS Code 风格搜索面板（替换/大小写/全词/正则/模糊/双范围）— fecf307
5. 大纲点击跳转 + 默认展开 H3 — 1516267
6. 工作区菜单移除、打开文件夹即工作区 — d32cff8
7. 崩溃修复（重入 update）、X hover — f217ecf/7dac94e/ab52f83
8. 搜索聚焦隐藏占位符 — 4882e0c
9. 清零编译警告 — 0d3915e

### dev 迭代会话（本 handoff 主体，12 个功能批次）
- **A1** 欢迎页（空工作区/空启动显示品牌+最近文件+操作入口）— 7e795da
- **A2** 窗口大小/位置记忆（关闭写 config.toml [window]，启动恢复+屏幕钳制）— 4e9ab84
- **A3(配置)** 默认窗口宽高（[window] default_window_width/height）— 7a6a08e
- **A4** 会话恢复（session.json：工作区根+标签集+活动标签；启动无参数时优先恢复）— acdaa04
- **A5** 全局缩放 ⌘+/−/0（60-200%，持久化，整套字号层级缩放）— eb77c59
- **B2** 搜索时文档内高亮全部匹配（主题色 search_highlight_bg，层级：代码背景<高亮<选区）— f196e99
- **B3** 粘贴 HTML→Markdown（macOS 粘贴板 public.html；标题/粗斜体/链接/列表/引用/代码块/表格）— a487b00
- **B4** 选中输入配对符号自动环绕（* _ ` " ' ( [ { ）— 9890156
- **B5** 选中文本粘贴 URL → [文本](url) — 5141b06
- **C1** YAML frontmatter 逐字保留（opaque RawMarkdown 块）— 7771645
- **C3** `[[wikilink]]` 点击打开/创建工作区文件 — 955e9f4
- **C4** `#tag` 高亮点击→工作区搜索 — de0cf96
- **C5** 大纲跟随滚动高亮当前章节 — 4bc8a5e
- **C6** 大纲双击标题→选中正文标题进入重命名 — d6eed49
- **C8/C9** 链接/脚注悬停预览（HoverPreviewTooltip；链接存在性 ✓/✗）— 07f615d
- **D1** 打开文件时树展开祖先定位 — ba1c315
- **D2** 文件树排序（名称/时间/类型，树头按钮循环，持久化）— 722a38d
- **D3** notify 文件监听，干净标签自动重载外部修改 — 0152399
- **D4** 删除移入 ~/.Trash — 92e32db
- **D5** 新建 Markdown 模板（[editor] new_file_template，{date} 展开）— cb88db7
- **D6** 树右键「创建副本」（name copy.ext）— 486e073
- **D7** 树 tooltip（名称·大小·修改时间，内建历法算法）— 编译+用例
- **D8** 树过滤框（扁平匹配列表 ≤50 条）— 90b3960
- **E6** 光标位置历史 ⌥⌘←/→（跨文件，上限 100）— 43cf0ed
- **E7** 按工作区记忆侧栏宽度（session.json sidebar_width）— 6d7f066
- **E8** 状态栏面包屑（工作区›相对路径，点击树中定位）— e1754a7
- **F1(验证)** 单文件 HTML 图片内嵌回归测试 — e9fa294
- **F2** 复制为 HTML（⇧⌘C，选区/全文渲染进剪贴板）— 3f58a65
- **G1** 原子写入（临时文件+fsync+rename）— cf82df0
- **G2** UTF-16 BOM 探测→特定占位提示 — 984e0bd
- **G3** 自动保存防抖可配置（[editor] autosave_debounce_ms）— 073597a
- **G4(基线)** 10 MiB 打开基准（实测 17.5s/159,683 块 ≈109µs/块；断言 ≤400µs/块 防回归）— 64652b7
- **G6** 崩溃恢复演练（写快照→崩溃→恢复→保存 四阶段）— 2945dec
- **G7** 渲染结构黄金快照（8 类关键块）— 8f2f77a
- **H1(批次一)** 偏好设置文件页：树排序/防抖间隔/记住窗口 三控件 — 2a87968
- **C4/C7(v1)/E3/D2/E5** 标签高亮、标题折叠渲染过滤、标签拖拽排序、树排序、⌘1-9 — 同期提交
- **品牌** velora.png 图标（icns/ico/Dock）— f9e2aa2 等

### 续接会话（2026-09-27 白天）
- **C7 chevron** 标题行内折叠按钮（左侧留白绝对定位，点击折叠/展开；折叠时光标被隐藏则回退标题）— 2f957fc + 4e245ac（端到端点击用例）
- **D9 文件树扫描异步化**（background executor + 代数校验 + 扫描中占位；3 个 gpui 用例）— 4d84521
- test 目标 unused import 警告清零 — 0379ae0
- roadmap 标记同步（C6/E8/B1/B7/B11 补 ✅；B9/B10/D6 标 ✅(部分)；剩余清单按代码证据重写）+ 验收记录第十三/十四批 — 52964c4 / fb11800
- `origin/dev` 建分支并推送 — fb11800

## 三、架构要点（接手必读）

### 关键文件地图
| 文件 | 内容 |
|------|------|
| `src/editor/workspace.rs`（~5400 行） | 侧栏全部逻辑：文件树/搜索面板/大纲/标签条/过滤/排序/会话持久化/光标历史 |
| `src/editor/render.rs`（~2400 行） | 编辑器主渲染：标题栏+标签、折叠过滤（apply_heading_fold_filter）、欢迎页、缩放、覆盖层注册 |
| `src/config/preferences.rs`（~3300 行） | 偏好 TOML 读写 + PreferencesWindow 设置界面（File/Theme/Image/Shortcuts/StatusBar 五页） |
| `src/config/session.rs` | session.json（工作区根+标签集+活动标签+侧栏宽度） |
| `src/editor/quick_open.rs` / `command_palette.rs` | ⌘P / ⇧⌘P 覆盖层 |
| `src/editor/watcher.rs` | notify 文件监听 |
| `src/components/markdown/html_paste.rs` | HTML→Markdown 粘贴转换 |
| `src/components/markdown/inline.rs` | 行内解析（InlineFragment/InlineSpan；C3/C4 深度扩展在此） |
| `src/components/block/element.rs` | BlockTextElement 绘制（搜索高亮 quads、代码背景） |
| `src/export/html.rs` | HTML 导出（图片 data URI 内嵌已具备） |

### 设置项全景（config.toml）
- `[window]`：remember_bounds / frame / zoom_percent / default_window_width / default_window_height
- `[editor]`：workspace_sidebar_width / tree_sort / autosave_debounce_ms / new_file_template（`{date}` 占位）
- 会话：`session.json`（root/tabs/active/sidebar_width）
- 偏好 UI 已暴露：启动行为、主题、字体、树排序、防抖间隔、记住窗口（File 页）；**尚未暴露**：zoom_percent、default_window_size

### 重要约定/陷阱
1. **GPUI 限制**：TextRun 无逐 run 字号（inline code 缩放走渲染路径）；svg 不继承父 div 的 text_color（必须直接设在 svg 上）；Div 链式 `.id()` 后类型变 Stateful，`if/else` 分支类型需一致（用 into_any_element）。
2. **禁止重入**：Editor 实体 update 内不得再 `editor.update()`（曾致 coredump，已修，见 ab52f83）。
3. **菜单 action 时序**：原生菜单派发栈内 window handle 失效（"window not found"），必须 `cx.spawn` + `cx.update` 异步重入（见 open_recent_file）。
4. **canonicalize**：macOS /var ↔ /private/var；测试断言路径需 canonicalize（踩过 3 次）。
5. **osascript 截屏**：机器锁屏/台前调度会让截图与点击失效；验证优先用 gpui 测试。
6. **i18n**：I18nStrings 六处注册点（struct/Option/registry/resolve/zh/en）；曾两次漏掉导致解析错误。建议用行插入脚本时逐处核对。
7. **基线偶发失败**：`autosave_does_not_overwrite_external_file_changes` 在 main 即失败，勿误判为新回归。
8. **`Context::emit` 是延迟效果**：事件先入 `pending_effects`，订阅者在该实体 lease 释放后才被调用，所以事件处理器里可以安全 `read`/`update` 发出事件的那个块（C7 chevron 依赖这一点）。
9. **`Entity::update` 不会自动 notify**：渲染期跨实体写字段是安全的（不 notify ⇒ 无自触发重渲染循环），需要重绘必须显式 `cx.notify()`。
10. **元素级 UI 验证手法**：元素加 test-only `debug_selector` + `VisualTestContext::debug_bounds("名字")` 取真实布局，再用 `simulate_click(bounds.center(), Modifiers::none())` 端到端点击；锁屏环境下这比截图可靠（C7 用例见 tests.rs）。

## 四、剩余项（roadmap 未完成 13 项 + 部分完成 6 项 + 执行中发现 3 项）

### P0/P1 尾巴（roadmap 内）
- **A6** 全屏切换 F11 / ⌘⌃F（菜单项 + 快捷键 + 标题栏态；当前仅有窗口最大化图标）— ABSENT
- **B6** 智能标点（中英文引号/破折号智能替换，偏好开关；默认关闭）— ABSENT
- **B8** 阅读时间估算（状态栏按 300 字/分）— ABSENT
- **B12** 长段落性能护栏（单块超 2 万字降级源码渲染 + 状态栏提示）— ABSENT
- **C2** TOC 块（`[TOC]` 渲染为可点击目录）— ABSENT
- **G5** 启动时间优化（冷启动 < 1s 计时记录）— ABSENT
- **G8** 惰性建块（G4 3s 预算的关键：当前一次性建 16 万块实体；文件树侧已由 D9 异步化）

### P2（roadmap 内）
- **F3** 导出主题选择、**F4** 打印、**F5** PNG 长图（需离屏渲染，工程量最大）
- **H1 批次二**：zoom_percent / default_window_size 未暴露到偏好 UI，且缺「窗口」分组页（偏好窗口当前五页：File/Theme/Image/Shortcuts/StatusBar）
- **H2** 设置文件分组完善、**H3** 主题变量文档、**H4** 语言包外置 JSON、**H5** 命令注册表统一

### 部分完成（roadmap 标 ✅(部分) 或已有 v1，补完即可收口）
- **B9** 代码块：现为可编辑语言输入框 + 复制语言名；缺只读徽标与「复制代码块内容」
- **B10** 图片粘贴：目标目录可配置；命名仍为 `pasted-image.<ext>`+序号，缺 `YYYY-MM-DD-hash` 模板
- **D6** 文件树：已有右键「创建副本」；缺剪贴板 复制/粘贴 到树（含图片）
- **C10 v2** 源码宽度语法写回（`![alt](src){width=60%}` 或 HTML width 属性）
- **E3** 拖拽已可重排；可补拖拽中目标高亮（drag_over 已有 API）
- **F2 增强**：剪贴板 HTML flavor 写入（当前仅纯文本 HTML 源码）
- **B11** 已实现但缺专项用例（模式切换 + 撤销不丢字），补一个回归测试即可

### 执行中发现
- **E9** 快速切换器 IME（当前仅 ASCII 键入）、**E10** 恢复快照与会话合并、**A7** 锁屏窗口同步复核

## 五、验证方式

```bash
cargo build                      # 零警告零错误
cargo test                       # 838 通过 0 失败（基线偶发项偶有失败，见陷阱 7）
cargo test large_document        # G4 基准（需 node scripts/generate-fixtures.mjs tests/fixtures/perf 生成 10MiB fixture，gitignored）
cargo run .                      # 以仓库为工作区打开（可验证欢迎页/树过滤/排序/拖拽/搜索/大纲）
```

UI 视觉验证注意：机器锁屏（约 1 小时无操作）后截图全黑、System Events 报"无效索引"；台前调度开启时乱点会最小化窗口。**建议解锁后补拍**：B2 搜索高亮、C5 大纲联动、E1/E2 覆盖层布局、C10 拖拽手柄、C7 折叠 chevron、欢迎页整体布局。

## 六、工作树遗留

- `?? .zcodeignore`：用户明确要求**不提交**，保持 untracked。
- main 基线遗留的 `M src/editor/tests.rs` 性能探针已在 dev 会话中随批次提交。

## 七、建议接手顺序

1. 读 `docs/plans/2026-09-27-dev-iteration-roadmap.md`（62 项全景+状态标记；已按代码证据校对）
2. 读本文件第三节「架构要点」避免重复踩坑
3. 下一批建议：**G8 惰性建块**（大文档 3s 预算的关键）→ **C10 v2** → P2（F3/F4 → H1 批次二 → H2-H5 → F5）→ 部分完成收口（B9/B10/D6/E3/F2 增强/B11 用例）→ 未实现项 A6/B6/B8/B12/C2/G5
4. 每项：实现 → 测试 → `cargo build` 零警告 → 独立提交（feat/fix(scope): 中文描述）→ 更新 roadmap 状态标记
