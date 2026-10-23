# Velora 主题编辑器

一个独立的网页小工具：在浏览器里实时调整 Velora 的全部主题 token，预览文档与界面外观，然后导出一份完整主题 JSON，再交给 Velora 现有的「主题 → 添加主题配置…」导入。

它不打包进 Velora，也不启动本地服务——直接双击 `index.html`（`file://`）即可使用。

## 打开

```sh
open tools/theme-editor/index.html
```

或者把 `tools/theme-editor/index.html` 拖进浏览器。首次打开默认跟随浏览器语言（中文 / English），右上角按钮可随时切换。

## 用法

1. 在顶部选一个基底主题：Velora Dark、Velora Light、Paper、Forest、Midnight、Ink。
2. 左侧按分组调整 token：
   - 颜色用取色器、`#RRGGBBAA` 输入框与透明度滑杆；
   - 尺寸、字号、行高、字距用数字输入；
   - 字重从 `thin`…`black` 里选；
   - 字体族支持自由输入（带常用字体提示）；
   - 占位文案是文本框。
3. 右侧「文档」「界面」两个页签会实时反映改动。
4. 填好「主题名称」和「作者」，点「导出主题」（或 `⌘S` / `Ctrl+S`）下载 `<名称>_<作者>.json`。
5. 在 Velora 里打开「主题 → 添加主题配置…」，选择刚下载的文件。

单项、分组、全部重置都会回到当前基底的值；切换基底或导入前若有未保存改动，会先确认。

## 导出格式

导出的是**完整快照**，不是相对基底的 diff：

```json
{
  "name": "My Theme",
  "creator": "Anonymous",
  "base_theme_id": "velora-dark",
  "theme": {
    "name": "My Theme",
    "colors": { "editor_background": "#1e1e1eff", "...": "..." },
    "dimensions": { "writing_max_width": 720.0, "...": "..." },
    "typography": { "...": "..." },
    "placeholders": { "empty_editing": "" }
  }
}
```

`theme` 四个 section 齐全，字段名与 `docs/theme-tokens.md` 一致，颜色保持 `#RRGGBBAA`。导入时也接受只覆盖部分字段的裸 Theme JSON，缺的字段会从当前基底补齐。

## token 清单从哪来

网页不自带第二份 token 表。`tokens.js` 由 Rust 测试模块
`src/theme/theme/theme_editor_tests.rs` 从 `Theme` 结构体和 `docs/theme-tokens.md`
生成，里面有 token 元数据、六个内置主题快照与分组信息。

主题结构体新增字段后，重新生成：

```sh
VELORA_UPDATE_THEME_EDITOR_TOKENS=1 cargo test --bin velora theme_editor_tokens -- --nocapture
```

`cargo test --bin velora theme_editor` 会检查生成物是否过期、token 是否覆盖每个
`Theme` 字段、分组与说明是否齐全，以及预览 CSS 是否引用了不存在的 token。
**不要手工编辑 `tokens.js`。**

## 测试

```sh
node --test tools/theme-editor/theme-editor.test.cjs
```

覆盖颜色归一化、单位换算、导入包/裸主题的归一化、完整快照导出与文件名清洗。
CI 在每个平台跑这一条；`tokens.js` 的一致性由 `cargo test` 守。

## 限制

- 预览是模拟渲染：用 CSS 近似编辑区、代码块、表格、菜单、对话框与状态栏，少数与真实视口相关的尺寸只能看数值，不能完全复刻应用里的最终排版。
- 工具只改主题 token，不校验配色对比度，也不替你决定导出后的主题是否好看。
- 页面状态（语言、基底、名称、作者）存在浏览器 `localStorage`；`file://` 下若浏览器禁用存储，工具仍可正常使用，只是不记忆。
