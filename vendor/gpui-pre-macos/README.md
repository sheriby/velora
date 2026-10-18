# macOS 窗口后端的 Velora 补丁

本目录基于 crates.io `gpui-pre-macos 0.3.9`。许可证保留于包内；本地覆盖由仓库根目录 `Cargo.toml` 的 `[patch.crates-io]` 启用。

| 源文件 | 本地修改 |
| --- | --- |
| `src/platform.rs` | `PathPromptOptions::directory` 设置打开对话框的起始目录 |
| `src/pasteboard.rs` | 写入纯文本时同时写入可选 HTML，剪贴板构造补齐新增字段 |
| `src/window.rs` | 内容视图禁用隐式拖窗；显式拖窗保留事件和窗口、延后执行并临时允许移动，避开 GPUI 窗口借用重入 |

HTML 读取仍由应用层负责，真实 CoreText 排版位于另一个包 `gpui-pre-apple`。完整迁移状态、源码去向及验证边界见 [Velora 补丁说明](../README.md)。
