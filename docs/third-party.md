# 第三方代码来源

当前编辑器实现以 Velotype 的提交 `ed65977be94f2f2703037fcb8b6cbab2e7579571` 为基础，原项目地址为 <https://github.com/manyougz/velotype>，许可证为 Apache-2.0。许可证原文保存在仓库根目录的 `LICENSE-APACHE`。

为先交付可运行的 Markdown 富文本编辑器，当前暂时引入了编辑器代码及其编译所依赖的应用模块、资源与基准代码。这是过渡状态；后续将把编辑部件与 maksher 的窗口、工作区和文件服务分开，并移除不需要的原应用功能。已修改的地方包括应用名称、包标识、配置目录、安装命令和帮助菜单。

分发前需要核对保留的第三方版权声明、修改标记、依赖许可和图标归属。当前沿用的 Velotype 图标仅用于本地开发，不作为 maksher 的最终品牌素材。

`tests/fixtures/velotype-original.md` 是从该提交原样保留的回归测试输入，不作为 maksher 的说明文档。
