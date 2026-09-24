# maksher

面向长篇写作的原生 Markdown 编辑器。首版以 macOS 为主要开发和运行平台，使用 Rust 与 GPUI；Markdown 默认以富文本形式编辑，也可切换到源码模式。AI 功能不在首版范围内。

## 首版功能

- 打开一个文件夹作为工作区，浏览 Markdown 与代码文件；
- 多标签编辑 Markdown，代码文件使用只读语法高亮窗口；
- 按文件名筛选，查看文档标题大纲和最近工作区；
- 在工作区中新建、重命名、移动和删除文件或文件夹；
- 粘贴或拖入图片时复制到资源目录并插入相对路径；
- 自动保存打开的脏标签；检测到外部文件修改时暂停写回并保留恢复快照；
- 意外退出后把未完成内容作为恢复副本打开；
- 浅色、深色主题可手动选择，也可跟随系统外观。

## macOS 开发

开发时使用 fastdev，不使用 release profile。命令：

    cargo run --profile fastdev -- /路径/到/工作区
    cargo check --profile fastdev
    cargo test --profile fastdev editor::workspace::tests:: -- --test-threads=1

项目的 .cargo/config.toml 使用 sccache 作为 Rust 编译缓存。首次构建仍需编译 GPUI 依赖。

## macOS 内部安装包

在 macOS 开发机执行 scripts/package-macos.sh，会用 fastdev 构建生成 dist/maksher.app 和 dist/maksher-0.1.0.pkg。安装包未签名或公证，仅用于本机和小范围内部试用。

## Windows 内部安装包

在 macOS 交叉构建 Windows x64 内部安装包时，需要 MinGW-w64、NSIS 和 x86_64-pc-windows-gnu Rust target。执行 scripts/package-windows.sh 会先编译 Windows 可执行文件，再生成 dist/maksher-0.1.0-windows-x64-setup.exe。安装器使用 per-user 安装；Windows 实机运行尚未验收。

## 当前验证范围

已在 macOS 开发环境验证 GPUI 应用启动、构建 macOS .app/.pkg，并交叉构建 Windows x64 安装包；工作区、主题、图片处理、自动保存、恢复、外部修改冲突、Markdown 文档解析、行内格式和表格的定向测试通过。中文组合输入通过 GPUI 输入处理模拟测试；macOS 拼音实机体验、Windows 实机运行和全机性能阈值仍需单独验收。本仓库不宣称这些项目已通过。

## 设计与来源

- [maksher 原生编辑器设计](docs/plans/2026-09-24-maksher-gpui-editor-design.md)
- [界面原型](docs/design/界面原型.html)
- [界面原型说明](docs/design/界面原型说明.md)
- [第三方代码来源与许可](docs/third-party.md)
- [首版开发验证记录](docs/验收记录/2026-09-25-首版开发验证.md)

编辑核心基于 Velotype 固定提交的一次性抽取，采用 Apache-2.0 许可。应用名称、配置目录与用户界面属于 maksher。
