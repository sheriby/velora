# GPUI 子模块

GPUI 的必要补丁集中在一个独立仓库，通过 `vendor/gpui` 子模块记录确切提交：

- 仓库：`https://github.com/sheriby/velora-gpui`
- 官方基线：Zed `93d745668805029476e6d7fbb19652116fed51de`
- 快照版本：`gpui-pre 0.3.9`
- 官方原始代码、快照打包适配及产品补丁分别提交；来源与校验和见子仓 `upstream.json`。

GPUI Kit、Base、Component 和图标资源仍使用官方注册表发布包，没有本地源码覆盖。

## 检出与构建

```sh
git submodule update --init --recursive
cargo build --locked
cargo test --locked --bin velora
```

子模块记录公开 HTTPS 地址，CI 和贡献者检出不需要 SSH 密钥；维护时可配置 SSH push URL。CI 的全部 checkout 步骤启用递归子模块。普通 update 使用主仓 gitlink 的固定提交，不自动跟随 main。

## 六个依赖的统一覆盖

| 包名 | 子模块内路径 |
| --- | --- |
| `gpui-pre` | `crates/gpui` |
| `gpui-pre-apple` | `crates/gpui_apple` |
| `gpui-pre-macos` | `crates/gpui_macos` |
| `gpui-pre-windows` | `crates/gpui_windows` |
| `gpui-pre-linux` | `crates/gpui_linux` |
| `gpui-pre-wgpu` | `crates/gpui_wgpu` |

根 Cargo.toml 的六个 patch 全部指向同一个子模块，保证底层类型一致。子仓包含独立工作区，但 Velora 本身仍为单 bin crate。

详细的 21 个源码补丁、旧 15 类能力的去向、同步方法和平台边界见 [子仓 README](gpui/README.md) 与 [补丁说明](gpui/docs/patches.md)。HTML 剪贴板、图片缩放已移至应用层；原生平台修复与排版扩展保留在子仓。

## 升级

先在子仓选择与 Kit 匹配的快照，运行同步脚本生成原始基线和干净快照；在独立分支记录来源与打包适配，再合并、核对产品补丁并执行应用门禁。验证后才更新主仓 gitlink。不要直接切换到 Zed main 或自动追踪子模块远端分支。
