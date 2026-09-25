# Velora

面向长篇写作的原生 Markdown 编辑器。首版以 macOS 为主要开发和运行平台，使用 Rust 与 GPUI；Markdown 默认以富文本形式编辑，也可切换到源码模式。AI 功能不在首版范围内。

## 首版功能

- 打开一个文件夹作为工作区，浏览 Markdown 与代码文件；
- Markdown 与代码文件都在同一个窗口以标签页编辑；
- Rust、JavaScript/TypeScript、C/C++、C#、Go、Java、PHP、Python、Ruby、HTML/CSS、JSON、YAML、TOML 和 Bash 提供语法高亮；SQL、Swift、Kotlin、XML 等也可打开并按纯文本编辑；
- 已识别且可界定的 `:::` 扩展块按原文显示和编辑；未闭合 `:::`、`!!!`/`???` 提示语法会切到源码模式并说明原因；其他第三方插件语法不保证自动识别；
- 在左侧切换文件、搜索与大纲；搜索同时匹配文件名和文件内容并显示命中行；文件树可拖动调整宽度，再点文件图标可收起；从 macOS 菜单栏打开或切换最近工作区；
- 通过文件树右键菜单新建、重命名、移动和删除文件或文件夹；
- 粘贴或拖入图片时复制到资源目录并插入相对路径；
- 自动保存打开的脏标签；检测到外部文件修改时暂停写回并保留恢复快照；
- 意外退出后把未完成内容作为恢复副本打开；
- 内置 Velora 浅色/深色、Paper、Forest、Midnight、Ink 多套主题，也可跟随系统外观；正文与代码字体、字号可在设置中调整。

## macOS 开发

开发时使用 fastdev，不使用 release profile。命令：

    cargo run --profile fastdev -- /路径/到/工作区
    cargo check --profile fastdev
    cargo test --profile fastdev editor::workspace::tests:: -- --test-threads=1

项目的 .cargo/config.toml 使用 sccache 作为 Rust 编译缓存。首次构建仍需编译 GPUI 依赖。

## macOS 内部安装包

在 macOS 开发机执行 scripts/package-macos.sh，会用 fastdev 构建生成 dist/velora.app 和 dist/velora-0.1.0.pkg。安装包未签名或公证，仅用于本机和小范围内部试用。

## Windows 内部安装包

在 macOS 交叉构建 Windows x64 内部安装包时，需要 MinGW-w64、NSIS 和 x86_64-pc-windows-gnu Rust target。执行 scripts/package-windows.sh 会先编译 Windows 可执行文件，再生成 dist/velora-0.1.0-windows-x64-setup.exe。安装器使用 per-user 安装；Windows 实机运行尚未验收。

## 当前验证范围

macOS 已使用 `fastdev` 构建并生成可启动的 `.app` 与 `.pkg`。上一轮全量测试 782 项中 781 项通过；主题默认色失败项修复后已单独复测通过。本轮工作区搜索与界面改动另有定向测试。最新布局可看[界面原型图](docs/design/界面原型-最新.png)；Windows 实机运行和全机性能阈值仍待验收。

## 设计与来源

- [Velora 原生编辑器设计](docs/plans/2026-09-24-velora-gpui-editor-design.md)
- [界面原型](docs/design/界面原型.html)
- [界面原型说明](docs/design/界面原型说明.md)
- [主题与长文排版](docs/design/主题与长文排版.md)
- [Velora 更名与界面调整验收](docs/验收记录/2026-09-25-Velora-更名与界面调整.md)
- [第三方代码来源与许可](docs/third-party.md)
- [首版开发验证记录](docs/验收记录/2026-09-25-首版开发验证.md)

编辑核心基于 Velotype 固定提交的一次性抽取，采用 Apache-2.0 许可。应用名称、配置目录与用户界面属于 Velora。
