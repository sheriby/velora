# GPUI 0.2.2 本地修补说明

本目录来自 crates.io 发布的 `gpui` 0.2.2，原项目为 [Zed GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui)，使用 Apache-2.0 许可证，原许可文本见 `LICENSE-APACHE`。为减少仓库体积，这里不复制上游示例、文档及独立测试目标；应用所需源码、构建脚本与资源保持原样。

Velora 仅修改两处源码：

- `src/window.rs`：在最后一个焦点句柄引用释放时标记需要清理。
- `src/app.rs`：只有存在待清理句柄时才遍历焦点句柄表。

原版每处理一个 GPUI 更新事件都会扫描全部焦点句柄。长篇 Markdown 会创建大量编辑块，这使首次窗口更新的开销随块数急剧增加。本修补保持原有的释放和失焦处理，只跳过没有句柄释放时的空扫描。

项目根目录通过 Cargo 的 `[patch.crates-io]` 固定使用本目录。升级 GPUI 前应重新核对该修补是否仍必要，并运行 Velora 全量测试及 1 MiB、10 MiB 大文件诊断。
