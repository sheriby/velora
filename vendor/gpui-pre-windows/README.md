# Windows 后端的 Velora 补丁

本目录基于 crates.io `gpui-pre-windows 0.3.9`。许可证保留于包内；本地覆盖由仓库根目录 `Cargo.toml` 的 `[patch.crates-io]` 启用。

旧版的完整 IME 日志、起始目录失败回退和原 client GUID 已保留。源码去向与验证边界见 [Velora 补丁说明](../README.md)；macOS 上的源码守卫和 Windows 交叉编译不能代替 Windows 原生验收。

| 源文件 | 当前本地修改 |
| --- | --- |
| `src/clipboard.rs` | 读取 CF_HTML，复制载荷前限制为 2 MiB；未增加 HTML 写入 |
| `src/direct_write.rs` | 排版和字形回调支持逐段字号 |
| `src/directx_renderer.rs` | 调试路径从内嵌 HLSL 编译，并展开 include；非调试路径继续使用预编译字节 |
| `src/events.rs` | 将组合态及 `VK_PROCESSKEY` 交回消息循环；保留按键、字符、组合串、结果串和缺失输入处理器的完整诊断 |
| `src/platform.rs` | 起始目录与失败回退；打开、保存选择器共用原 client GUID，保留壳层记忆 |

以下能力由干净的新版上游提供，未重放旧本地实现：`run_foreground_task` 的 10 ms 前台任务预算；`dialog.rs:show_dialog` 的专用 STA 线程、owner 生命周期、取消与请求串行化；剪贴板 `LockedGlobal` 的空指针保护。
