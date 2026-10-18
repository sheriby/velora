# Linux 后端的 Velora 兼容补丁

本目录基于 crates.io `gpui-pre-linux 0.3.9`；保留发布包的许可证和资源。根目录 `Cargo.toml` 的 `[patch.crates-io]` 使用此副本，保留旧 GPUI 已有的接口兼容修改，不新增 Linux 产品支持。

| 源文件 | 本地修改 |
| --- | --- |
| `src/linux/platform.rs` | 打开选择器的 portal 请求接入 `PathPromptOptions::directory`；非法路径参数向调用方返回错误，替代旧版 `expect` |
| `src/linux/wayland/clipboard.rs` | 图片剪贴板构造补齐核心新增的 `html: None` 字段 |

新版上游已将文本实现拆到 WGPU 包，字号字段兼容位于 `gpui-pre-wgpu/src/cosmic_text_system.rs`。完整迁移状态与验证边界见 [Velora 补丁说明](../README.md)。
