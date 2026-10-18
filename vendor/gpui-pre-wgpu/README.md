# WGPU 文本后端的 Velora 兼容补丁

本目录基于 crates.io `gpui-pre-wgpu 0.3.9`；保留发布包的许可证和资源。根目录 `Cargo.toml` 的 `[patch.crates-io]` 使用此副本。

`src/cosmic_text_system.rs` 接续旧 Linux 文本系统的字段兼容：`ShapedRun` 增加 `font_size: None`，裁切 `FontRun` 时继续传递原字号字段，并补齐相关测试构造器。CosmicText 仍按整行字号排版，没有增加逐段字号覆盖能力。

这些改动用于保持公开结构体兼容，不代表 Velora 新增 Linux 或其他 WGPU 平台支持。完整迁移状态与验证边界见 [Velora 补丁说明](../README.md)。

发布包的完整测试目标缺少 `naga` 开发依赖及 `../../../assets/fonts/` 中的上游字体，不能直接当作本仓门禁。独立验证时需启用 `test-support`，在临时测试工作区补齐开发依赖与同一 Zed 快照的字体；不要往应用依赖或资源中加入这些测试专用文件。
