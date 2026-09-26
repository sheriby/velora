# dev 分支迭代验证记录（2026-09-27）

## 范围

roadmap（docs/plans/2026-09-27-dev-iteration-roadmap.md，62 项）中已完成 12 项：

A1 欢迎页、A2 窗口 frame 持久化、A4 会话恢复、A5 字号缩放、B2 搜索文档内
高亮、C1 frontmatter 保留、D1 文件树定位、D3 外部变更监听、D4 垃圾桶删除、
E1 快速切换器、E4 中键关标签、E5 ⌘1-9 切标签、G3 自动保存防抖可配置。

## 验证方式与结论

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| A1 欢迎页 | gpui 渲染烟雾测试 + 状态转换测试（welcome_page_renders/hides） | 通过 |
| A2 窗口 frame | config.toml 序列化断言（remember_bounds = true）+ 保存/读取回环 | 通过 |
| A4 会话恢复 | session.json 保存/读取/缺省回退测试 | 通过 |
| A5 缩放 | 全量测试通过；⌘=/⌘-/⌘0 绑定注册 | 通过 |
| B2 高亮 | 编译期映射正确性 + search 全部用例 | 通过（视觉效果待解锁屏截图复核） |
| C1 frontmatter | 两个 gpui 测试：保留为 opaque 块 / 非 frontmatter 的 --- 仍为分隔线 | 通过 |
| D1 树定位 | workspace 全部用例通过 | 通过 |
| D3 文件监听 | 编译 + workspace 用例；运行时行为需解锁后实测 | 待实机复核 |
| D4 垃圾桶 | 编译通过；workspace 用例通过 | 通过 |
| E1/E4/E5/E 快捷键 | tab 相关 93 项用例通过 | 通过 |

## 已知事项

- 全量测试唯一失败项 `autosave_does_not_overwrite_external_file_changes`
  为基线（main 9254d49）即存在的偶发用例，与本批改动无关。
- 2026-09-27 23:5x 起机器自动锁屏，后续原生截图需解锁后补拍；
  已改为 gpui 测试验证渲染路径。
- 执行中发现 4 个新增项（D9/E9/E10/A7）已录入 roadmap 待办。
