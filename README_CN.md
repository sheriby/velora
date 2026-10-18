# Velora

[English](README.md) · **简体中文**

---

> **项目状态**：本项目目前处于早期阶段，功能与实现仍在快速迭代。代码编写、测试与版本发布几乎全部由 AI Agent 完成，人类开发者负责梳理需求、讨论设计与把握项目方向。稳定性与兼容性尚需持续验证。如在使用过程中遇到问题，欢迎通过 [Issue](https://github.com/sheriby/velora/issues) 提交反馈，我们将尽快排查并修复。

Velora 是基于 Rust 与 GPUI 构建的原生 Markdown 编辑器：无 Electron、无 WebView，界面由 GPU 直接渲染，启动快、开销低、响应稳。WYSIWYG所见即所得，可随时切换 Markdown 源码。在性能之外，界面同样保持现代、简洁与可读，轻量而流畅。

## 🖼️ 界面预览

![实时所见即所得编辑：行内样式、表格、代码高亮与 LaTeX 公式](assets/screenshots/editor.png)

更多截图：[工作区与多标签页](assets/screenshots/workspace.png) · [分屏阅读与编辑](assets/screenshots/workspace-split.png) · [公式编辑器](assets/screenshots/formula-editor.png) · [六套内置主题](assets/screenshots/themes.png)

## ✨ 功能

### 写作与编辑

- **所见即所得 + 源码双模式**——Markdown 随打随渲染，随时切到源码；切换后撤销历史不丢内容。
- **阅读模式**——窗口级只读开关，切换标签和分屏后保持；支持浏览、选中、复制与查找，阻止文档编辑。通过状态栏图标、视图菜单或 `⌘⇧R` / `Ctrl+Shift+R` 切换。
- **完整的块级集**——标题、有序/无序/任务列表、表格、代码块、引用、脚注、callout、分隔线与图片；行内粗斜体、行内代码、链接与删除线。
- **数学公式与图表**——LaTeX 公式与 Mermaid 图表直接渲染在正文中。
- **图片**——粘贴或拖入即自动复制到文档资源目录并插入相对路径；默认 100% 对应正文可视容器宽度的 95%，图片始终限制在容器内；右键选择 25%、50%、75%、100% 的缩放比例，保持宽高比；比例以 `{width=NN%}` 写回源码，保存后保留。
- **编辑顺手事**——列表回车自动续写；选中输入配对符号自动环绕；选中文本粘贴 URL 生成链接；从浏览器或 Word 复制的内容粘贴时自动转 Markdown。
- **智能标点**（默认关闭，设置中一键开启）——按上下文输出弯引号，`--` 转破折号。
- **VS Code 风格查找替换**——匹配交给 ripgrep 的引擎（进程内调用）：大小写、全词、正则与模糊，正则可跨行；面板打开时正文实时高亮全部匹配，结果列表、高亮、跳转与全部替换读同一张命中表。
- **语法高亮**——Rust、JavaScript/TypeScript、C/C++、C#、Go、Java、PHP、Python、Ruby、HTML/CSS、JSON、YAML、TOML 与 Bash；其他文本文件按纯文本打开编辑。

### AI 文档助手

- **按需唤起**——选区工具栏、正文右键或 `⌘J` / `Ctrl+J` 打开浮动对话，可拖动，也可将同一段对话移至侧栏继续。
- **围绕 Markdown 工作**——摘要、解释、翻译、润色和扩写五个快捷指令，其余需求通过对话完成；按任务搜索工作区，使用 `@文件` / `@目录` 指定资料，查看来源并跳转。
- **先审阅后修改**——阅读结果留在对话，写作修改在正文原位标出增删，跨文档修改从文件列表切换，可逐处／单篇／全部接受或拒绝；接受后可以正常撤销。阅读模式下仍可问答与生成提议。
- **自备模型与可复用任务**——多连接、配置文件中的 API Key、自定义指令和按需加载的 Skills；会话与未接受修改保存在本地。在「偏好设置 → AI」中配置模型，助手的 `⋯` 菜单管理历史、指令和 Skills。详见 [AI 使用指南](docs/ai.md)。

### 工作区与知识管理

- **文件夹即工作区**——文件树支持新建文件与文件夹、重命名（名称在树内直接输入）、创建副本、复制粘贴与删除（移入系统废纸篓）；排序方式（按名称、修改时间或类型）在偏好设置中选择；右键可在文件管理器中打开或复制绝对/相对路径；悬停文件显示大小与修改时间。
- **分屏与编辑器组**——左右、上下分屏可嵌套组合；拖动标签或文件树中的文件创建分屏，各组独立滚动，同一文件的多处视图保持内容同步。
- **全局搜索**同时匹配文件名*与*文件内容，支持工作区范围替换；正文解码与打开文档同一口径（UTF-8 优先、GB18030 回退）。
- **图片文件预览**——从文件树、打开文件、快速打开、最近文件或启动参数查看 JPG、PNG、WebP、ICO 等图片，按窗口大小适配显示，状态栏显示尺寸；图片只读，不进入文本编辑与保存流程。
- **大纲面板**——点击跳转，跟随滚动高亮当前章节，双击标题在正文中重命名。
- **知识链接**——`[[wikilink]]` 点击打开或创建同名笔记；`[TOC]` 渲染为可点击目录。
- **frontmatter 与结构**——YAML frontmatter 逐字保留；标题可折叠；链接与脚注引用支持悬停预览。
- **链接行为正确**——`Cmd/Ctrl+点击` 直接跳转：外部地址交默认浏览器，本地文档在应用内打开，`#锚点` 在本文内跳转。
- **外部变更**——干净标签在文件被外部修改时自动重载；已编辑标签给出冲突提示，而不是被静默覆盖。

### 导航与窗口

- **`⌘P` 快速切换器**，输入框支持中文输入法；**`⇧⌘P` 命令面板**覆盖全部命令。
- **新建文档**——`⌘N` / `Ctrl+N` 在当前工作区创建独立的 `Untitled` 标签；未命名草稿自动写入恢复快照，保存时再选择路径。
- **标签页并入标题栏**——拖拽排序，`⌘1–9` 跳转，中键关闭，右键批量关闭。
- **光标位置历史**——`⌥⌘←` / `⌥⌘→` 在文件之间后退与前进。
- **会话与窗口**——空工作区显示欢迎页；启动自动恢复上次会话的标签；记住窗口大小与位置；侧栏开关与上次使用的面板全局记忆（启动行为可在设置中配置）；`⌘+` / `⌘−` / `⌘0` 界面缩放；全屏切换。
- **状态栏**——字数与选中统计、预计阅读时间、可点击的面包屑路径。

### 可靠性与性能

- **文字不丢**——自动保存（防抖间隔可配置）、原子写入（临时文件 + rename）、崩溃后自动恢复未保存内容。
- **文件历史**——自动保存也记录版本，默认最少间隔 5 分钟、每文件最多 30 条、保留 7 天，三项均可在偏好设置中调整；手动保存标记检查点。先预览历史原文、对比当前内容，再确认恢复；恢复前备份当前内容，恢复操作可撤销。
- **长文依然快**——超大手稿首屏同步建块、立即呈现，其余内容经后台分批流入，打字与滚动全程不卡。
- **启动快**——初始化分散到后续帧，窗口即刻呈现。
- **应用内更新**——启动时检查 GitHub 正式版（可在设置中关闭或包含预发布版本；菜单与命令面板可手动检查）；更新通知确认后下载并校验 SHA-256，随后安装并重启。
- **测试守护**——1900 余项自动化测试、渲染结构黄金快照、逐块导入预算断言，回归进不来。

### 导出

- **格式**——单文件 HTML（图片内嵌为 data URI）、PDF、整篇 PNG 长图、系统打印，以及「复制为 HTML」。
- **导出主题**——导出任何内容都可选择*当前*、*浅色*或*深色*主题。
- **Chromium 依赖**——PDF、打印与 PNG 渲染经由无头 Chromium：请安装 Chrome、Chromium 或 Edge，或用 `CHROME` 环境变量指定浏览器路径。

### 外观与设置

- **六套内置主题**——Velora 浅色 / 深色、Paper、Forest、Midnight 与 Ink，并可跟随系统外观自动切换；自定义主题包放入用户目录即可加载（字段清单见[主题 token 参考](docs/theme-tokens.md)）。
- **专注模式与打字机模式**；界面、正文与代码的字体与字号分别可选，写作列宽可配置。
- **中英双语界面**——内置简体中文与英文；外置语言包放入用户目录即用。
- **应用内偏好设置**——一个窗口集中管理文件、主题、图片、快捷键、窗口与状态栏选项。

## ⌨️ 键盘快捷键

表中为 macOS 按键，其他平台使用 Ctrl 系等效键；快捷键可在「设置 → 快捷键」中自定义。

| 按键 | 动作 |
| --- | --- |
| `⌘P` | 快速切换文件 |
| `⌘J` | AI 文档助手 |
| `⇧⌘P` | 命令面板 |
| `⌘F` | 查找与替换（`⌘G` 下一个匹配，`⇧⌘G` 上一个） |
| `⌃Tab` | 切换所见即所得 / 源码模式 |
| `⌘B` / `⌘I` / `⌘U` / `` ⌘` `` | 粗体 / 斜体 / 下划线 / 行内代码 |
| `⇧⌘A` | 一步选择全文 |
| `⌘S` / `⇧⌘S` | 保存 / 另存为 |
| `⇧⌘C` | 把选区或全文复制为 HTML |
| `⌥⌘P` | 打印 |
| `⌘1`–`⌘9` | 切到第 N 个标签 |
| `⌥⌘←` / `⌥⌘→` | 光标历史：后退 / 前进 |
| `⌘+` / `⌘−` / `⌘0` | 放大 / 缩小 / 重置缩放 |
| `⌃⌘F` / `F11` | 切换全屏 |
| `⌃W` | 收起 / 展开侧栏 |
| `⌘N` / `⌘O` | 新建文档 / 打开文件 |

## 🚀 从源码构建

需要 **Rust stable 工具链**。

仓库的 `.cargo/config.toml` 已把 `sccache` 配置为编译缓存 wrapper，请先确保已安装：

```bash
cargo install --locked sccache --version 0.17.0
```

然后：

```bash
cargo build
cargo run                       # 空窗口启动
cargo run /路径/到/工作区        # 以指定文件夹为工作区打开
cargo test
```

## 📦 发布包

- **macOS**——执行 `scripts/package-macos.sh`，生成 `dist/velora.app` 与 `dist/velora-<版本>.pkg`。安装包未签名、未公证，仅用于本机与小范围内部试用。
- **Windows**——在 Windows x64 上用 PowerShell 7 执行 `./scripts/package-windows.ps1`，通过 MSVC 原生构建 release 安装包，生成 `dist/velora-<版本>-windows-x64-setup.exe`。需要 Rust MSVC 工具链、Visual Studio C++ 构建工具、Windows SDK 与 NSIS；CI 在原生 Windows 上运行构建与测试。

## ⚙️ 配置

配置位于系统应用配置目录（macOS 为 `~/Library/Application Support/velora/`）：

| 文件 / 目录 | 用途 |
| --- | --- |
| `config.toml` | 偏好设置——大部分条目可在偏好设置窗口中修改 |
| `session.json` | 各工作区打开的标签、活动标签与侧栏宽度 |
| `languages/` | 外置语言包 |
| `themes/` | 外置主题包 |

`config.toml` 主要分节：

| 分节 | 键 | 控制 |
| --- | --- | --- |
| `[window]` | `default_window_width`、`default_window_height`、`open_position`、`remember_bounds`、`zoom_percent` | 默认尺寸、居中还是记住位置打开、窗口记忆、界面缩放 |
| `[editor]` | `tree_sort`、`autosave`、`autosave_debounce_ms`、`new_file_template`、`smart_punctuation`、`external_change_policy`、`delete_policy`、`workspace_sidebar_width` | 文件树排序、自动保存开关（关掉只停写文件，恢复快照与外部改动检测照跑）、自动保存间隔、新建文件模板（`{date}` 会展开）、智能标点、外部变更处理、移入废纸篓还是永久删除、侧栏宽度 |
| `[file_history]` | `interval_minutes`、`max_entries`、`retention_days` | 自动快照最少间隔（默认 5 分钟）、每文件数量上限（默认 30 条）、保留天数（默认 7 天）；手动保存和恢复前检查点不受间隔限制，所有版本共同受数量与天数上限约束 |
| `[export]` | `theme` | 导出 HTML、PDF 与 PNG 时使用 `current` / `light` / `dark` |
| `[updates]` | `check_on_startup`、`include_prereleases` | 启动时检查更新；包含预发布版本 |

## 🙏 致谢

- **[Zed](https://github.com/zed-industries/zed)**: 提供 GPUI 框架，支撑原生界面与 GPU 渲染。
- **[Velotype](https://github.com/manyougz/velotype)**: 编辑器早期实现的参考项目。
- **[Merman](https://github.com/Latias94/merman)**: 原生 Mermaid 解析、布局与 SVG 渲染。
- **[RaTeX](https://github.com/erweixin/RaTeX)**: LaTeX 公式解析、布局与 SVG 渲染。
- **[Tree-sitter](https://github.com/tree-sitter/tree-sitter)**: 编程语言解析与语法高亮。
- **[ripgrep](https://github.com/BurntSushi/ripgrep)**: 通过 `grep-*` 与 `ignore` 库提供进程内搜索及文件遍历。
- **[pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark)**: HTML 导出中的 Markdown 解析。
- **[htmd](https://github.com/letmutex/htmd)**: 富文本粘贴时的 HTML 转 Markdown。
- **[resvg](https://github.com/linebender/resvg)**: GPUI 中的 SVG 光栅化。
- **[chromiumoxide](https://github.com/mattsse/chromiumoxide)**: 驱动 Chromium 完成 PDF、PNG 导出与打印。

## 📄 许可证

[Apache-2.0](LICENSE-APACHE)
