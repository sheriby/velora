# Apple 文本后端的 Velora 补丁

本目录基于 crates.io `gpui-pre-apple 0.3.9`。许可证保留于包内；本地覆盖由仓库根目录 `Cargo.toml` 的 `[patch.crates-io]` 启用。

`src/text_system.rs` 移植旧 `vendor/gpui/src/platform/mac/text_system.rs` 的逐段字号能力：向 CoreText 写入覆盖字号，按 UTF-16 区间回查 shaping 结果，不合并字号不同的 run，并保留新版的连字处理。原有文本构造器补齐默认字段。

旧核心的真实 shaping 测试入口没有复制到这里，应用测试直接调用公开 `AppleTextSystem`。完整迁移状态、源码去向及验证边界见 [Velora 补丁说明](../README.md)。
