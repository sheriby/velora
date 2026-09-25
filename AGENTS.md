# Velora 项目约定

- 应用显示名称是 `Velora`，可执行文件、Cargo 包和配置目录使用 `velora`。不要把其他人名当作应用名称或包标识。
- 所有需要用户审核的文档必须使用中文，包括 `README.md`、`docs/plans/` 中的设计与实施计划、测试报告、评审报告和交接文档。代码标识符、命令、依赖名及外部资料标题可以保留原文，但说明文字与验收结论必须是中文。
- 第三方许可证和用于回归测试的原始夹具须保持原文；另写中文说明供用户审核，不把这些原文文件当成项目说明文档。
- 在当前 `velora` 工作区直接开发和提交应用代码。未经用户明确要求，不创建 Git worktree，也不把 velora 的实现放在临时目录中。可在临时目录构建独立的第三方基线副本，但不得修改原始 `../velotype` 仓库。
- 项目使用 Superpowers 的设计、计划、实施与评审流程，不维护 `.devflow` 文档。
- 用户要求自主完成决策，不要在开发过程中向用户追加决策问题。
- 当前开发阶段优先实现可用功能。使用 debug 派生的 `fastdev` profile 编译和运行，不要使用 `--release`；核心功能可用后再做性能与打包验收。macOS 先发布，Windows 正式发布安排在下一版。
- `fastdev` profile 开启轻度优化、增量编译和调试信息。GPUI、taffy、rustybuzz、ttf-parser 等重型依赖按 `Cargo.toml` 中实际存在的包级规则优化；不要为未加入依赖树的包添加无效覆盖项。
- Cargo 使用项目 `.cargo/config.toml` 中的 `sccache` wrapper。尚未安装时，用 `cargo install --locked sccache --version 0.17.0` 安装与当前 Rust 1.88 兼容的版本。
