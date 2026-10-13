# Markdown 模型：行内解析、序列化与富媒体

> 基于 `dev`，2026-10-14 新增。这一册讲"一种 markdown 语法在 Velora 里由谁认定、由谁写回"。
> 相关：[editor-core.md](./editor-core.md)（块级解析与写回） · [render-pipeline.md](./render-pipeline.md)（怎么画出来）
> **往返保真是本仓库的头号破坏性缺陷类**：改这一册覆盖的任何代码，先想"内存模型 / 界面渲染 / 导出"三个 surface 是否还一致。

## 1. 两套解析器，别搞混

| | 用途 | 位置 |
|---|---|---|
| 手写逐行扫描器 | 导入成块树、写回、源码高亮 | `src/editor/document/parse.rs` + `import.rs`（块级）、`src/components/markdown/inline/parse.rs`（行内） |
| pulldown-cmark | **只**用于 HTML 导出与 ⌘-点击锚点解析 | `src/export/html.rs` |

两者的块结构判定必须一致，否则出现"界面正常、HTML 不对"。一致的手段不是抄一份，而是：导出侧的块结构一律信 pulldown，应用侧的特殊写法（如标题分隔符里的全角空格）在进 pulldown 前做**字节等价归一**（`heading_separators_for_parse`）。

## 2. 行内模型 InlineTextTree

`InlineTextTree = { fragments: Vec<InlineFragment>, escaped_offsets }`（类型层在 `markdown/inline.rs`，本体在 `inline/tree.rs`）。

- **定界符字符不进树**：树里只有可见文本 + 属性（粗/斜/下划线/删除线/高亮/行内代码/上下标、链接、脚注、行内公式、HTML 白名单样式、`<br>` 换行标记）。定界符在序列化时重建。
- **但写法落盘**，靠三件套：
  1. `InlineStyle::emphasis_marker` — 强调用 `*` 还是 `_`（`__粗__` 不会被写成 `**粗**`）。
  2. `escaped_offsets` — 可见文本里"这个字符是反斜杠换来的"那些区间。少了它，一次打字就会把写法读成语法：`\*不强调\*` 与 `2 * 3` 在可见文本里长得一样。**这是语义不是写法，任何路径都不清**（格式化命令也不清）。
  3. 原文字段 — 数学存完整源码、脚注存 `[^id]`、Reference/Autolink 链接存原形，序列化原样写出。
- 块级写法数据在块那一侧：`BlockRecord::list_marker`（子弹字符、序号分隔符、任务框写法）、`separator_marker`、`callout_marker`。
- 树操作：`split_at` / `append_tree` / `replace_visible_range`（编辑主入口）/ `toggle_*` / `clear_styles_in_range` / `attributes_for_insertion_at`（光标处继承属性，块首尾不外溢 code/删除线/高亮）。切片与偏移运算一律先收敛到字符边界。
- 图片语法 `![alt](src)` **不进树**，留作字面文本，由渲染层按需抽出（`image.rs:parse_inline_image_at`）。

## 3. 解析

`InlineTextTree::from_markdown` → 递归下降 `inline/parse.rs:parse_until`，逐 token 的判定顺序即优先级：

> 行内数学 → 反斜杠转义 → 脚注引用 → 链接 → `<br>` → 行内 HTML 容器 → autolink → 强调/代码定界符 → 字面

- **没有定界符栈数据结构**：嵌套靠递归 + 累积样式，推测解析用 `builder.mark()` / `rollback` 回退（漏回退会让正文翻倍）。
- 侧翼规则在 `inline/delimiters.rs`：`_` 走 CommonMark 口径（词内下划线不成强调，`topic_embedding_attention` 要保住）；`*`/`~`/`=` 走 CJK 友好口径（CJK 标点不算标点、CJK 字顶替"空白或标点"）。
- 链接三形态（`inline/links.rs:locate_inline_link`）：`[l](u "t")`、`[l][ref]`、collapsed `[l]`。空 label 的 `[]()` 不算链接（保留原文让光标能继续填）。
- 行内 HTML 只认白名单标签且无危险属性，映射成语义样式（strong→粗、del→删除线…）；样式与语义都无增益时按字面处理。
- 编辑后重解析走"可见文本模式"：此时 `\` 是字面字符，所有跳过转义的扫描都要检查 `literal_char`。

## 4. 序列化

`serialize_markdown`（快路径：单片段全默认样式且不含特殊字节 → 原样返回）→ `serialize_markdown_plain`。

核心是 `inline/stacks.rs:choose_fragment_stacks`：为每个片段选**定界符栈**（markdown 记号 vs `<strong>`/`<sup>` 一类 HTML 记号），用 DP 最小化"关闭旧栈 + 开启新栈"的字节数，并对三类情况加惩罚——连续多个同种星/下划线（`****` 歧义）、侧翼不合法、HTML 变体。上下标的 markdown 记号要求前一片段以 ASCII 字母数字结尾，否则退 HTML 记号。

**重建出的字节只保证"语义相同、写法可能不同"**。会不同的情形：DP 为避歧义换记号、手动 toggle 后强调记号统一成 `*`、行内代码两端的空格填充、`<br>` 统一写法、脚注上标还原成 `[^id]`。因此：

> **写回必须只动真正改过的那段字节**（editor-core.md §5），不能"从块树重新生成这一块"。

`serialize_markdown_plain`（不建映射）与 `markdown_offset_map`（建映射）是**两条实现**，一致性由 `inline/tests/serialize.rs:serialize_markdown_matches_offset_map` 守着；改一条必须同步另一条。

## 5. 三套偏移映射

| 映射 | 方向 | 消费者 |
|---|---|---|
| `InlineMarkdownOffsetMap`（`tree.markdown_offset_map()`，按需现算不缓存） | 可见文本 ↔ 规范序列化结果 | 源码模式光标投影、跨块选区、粘贴、搜索高亮 |
| `InlineSourceOffsetMap`（`inline/parse.rs`） | 规范序列化结果 ↔ **用户原文** | `source_mapping.rs`（源码视图块 ↔ 缓冲区） |
| `InlineEditResult.visible_to_normalized` | 编辑前可见偏移 → 归一化后偏移 | 光标/选区跟随重解析 |

`InlineSourceOffsetMap` 的对齐算法：解析后重序列化，字节相同就是恒等表；否则按"同一可见偏移的边界组"对齐两侧字节组，组内取公共前后缀再线性内插，始终收敛到字符边界。这是"HTML 样式被改写后仍能精确编辑显形的链接地址"的支撑。

## 6. 块级富媒体模型

- **表格**（`markdown/table.rs`）：`TableData { header, rows, alignments }`，单元格在模型侧是 `InlineTextTree`，运行时每格另有一个 `Block` 实体（`TableRuntime`）。所有变更操作先 `normalize_shape()` 补齐矩形；行列数有上限（每格是块实体）。列宽是水位法（`TableColumnLayout`），测量用的字号与间距必须与渲染一致。
  **竖线切分只有一处判据**：`cell_separator_positions`（等长反引号串内不切、串外奇数反斜杠前缀的 `\|` 不切），解析与序列化共用。对齐的"无标记"与"左对齐"刻意区分，无标记列不会被静默补冒号。
- **HTML 块**（`markdown/html.rs` + `html/parse.rs`）：html5ever 解析（永不失败，畸形标记按浏览器规则恢复）+ **安全白名单分类**。三档结果：语义节点（可渲染）、raw 文本块（整棵按原文显示与存储）、空（只剩游离闭标签，整块不画）。任何 `on*` 属性或 URL scheme 不在白名单 → 该节点整棵降级为 raw。style 只认 color / background-color / font-size / text-align。
  **callout 不在这里**：它是引用块头 `[!TYPE]`，模型在 `block/state.rs:CalloutVariant`，识别与建块在 `editor/document/blocks.rs`。
- **图片**（`markdown/image.rs`）：**路径解析唯一口径 `resolve_image_source(src, base_dir)`**——剥尖括号 → 远程 URL（不做 percent 解码，`%20` 属于 URL）→ `file:` 前缀 → percent 解码 → 绝对或相对基目录。阅读视图与导出同源。`{width=NN%}` 是缩放手柄写回的属性。引用定义扫描要跳过围栏内/HTML 块内/注释内，并剥容器记号前缀。
- **公式**（`components/latex/`）：`parse_display_math_segments` 交代**整个源码跨度**（支持 `$$x$$ 尾文`、同行多段）；只取"第一个公式"的那个视图给分类器与草稿用，**渲染与导出必须消费分段表**。ratex → SVG，`recolor_default_black` 换成主题色、根标签单位归一到 px。行内必须用 Text 样式排版，否则大算子被压小。
- **mermaid**（`components/mermaid/`）：先过图类型白名单再渲染；产出含错误标记就视为失败，块渲染层退回显示源码。显示缩放按复杂度与可用宽度算。
- **SVG 缓存**：公式与 mermaid 都是"进程内 LRU + 磁盘"两级，键含**版本串**（升级即整体失效）；mermaid 的显示态键把缩放按千分位分档（拖宽度不逐像素换键）；半截文件（无 `</svg>`）按未命中；写盘用同目录临时文件 + rename。

## 7. 代码高亮两条路

| | 何时用 | 位置 |
|---|---|---|
| tree-sitter | 渲染态的围栏代码块 | `markdown/code_highlight.rs` |
| 手写逐行扫描器 | **源码视图**（按行切片的 markdown 源文件）、公式编辑态的 LaTeX 着色 | `markdown/source_highlight.rs` |

源码视图不用 tree-sitter 的原因：文档按固定行数切片，围栏跨接缝，逐块解析拿不到入口状态。扫描器带 `MarkdownSourceState`（围栏 / 数学 / frontmatter / HTML 注释四态），每块记 `source_fence_entry/exit`，由编辑器在整树重建时全量串联、打字后增量级联（render-pipeline.md §6）。

高亮产物统一是 `CodeHighlightSpan { range, class }`；class → 主题色的映射在 `code_highlight_color`（通用类读 `code_syntax_*`，markdown 源码类读 `md_syntax_*`）。注意"认识这个语言但没有 grammar"时返回**空 spans 而不是 None**，调用方别把 None 当"无色"。

## 8. 导出：三个 surface 的第三个

内存模型、界面渲染、导出必须对同一段 markdown 给出同一个解释。导出侧的口径（`src/export/`）：

- **输入与保存同源**：导出取的是缓冲区文本（`document_text_for_save`），不是块树序列化的结果。
- **块结构只算一份**：`html.rs` 只保留一层文本预处理（front matter 剥离），其余全部在 **pulldown-cmark 的事件流**上做后处理——公式、提示框、图片内联、标题 id 与目录都在事件流上替换。按行改写原始 markdown 再交给 pulldown 的做法等于用手工启发式重推块结构，会与阅读视图的解析器分叉，是"应用里正常、导出就错"整类缺陷的根因。
- 允许的例外只有**字节等价归一**（`heading_separators_for_parse`）：同字节数、偏移不动、判据只借 `BlockKind` 解析器一处、字面区（代码块/HTML 块）取自 pulldown 自己的事件流。
- 与应用侧共用判据而不是各写一份：图片路径走 `resolve_image_source`、货币启发走 `looks_like_currency_between`、HTML 消毒走 `sanitize_html_for_export`、percent 解码走 `file_url.rs`。
- **PDF / PNG / 打印都以导出 HTML 为视觉基准**，经无头 Chromium（`chromiumoxide`）渲染：PDF 打印到临时文件、PNG 量全文尺寸后整页截长图、打印复用 PDF 再交系统命令。主题 CSS 内嵌在 `<head>`（`html/css.rs`，另有 Chromium 打印专用的一套）。
- 断言导出结果**只看 `<body>` 之后**：内嵌样式表里就有 `.vlt-inline-math`、`[TOC]` 这类字面量，拿整份文件做"这里不该出现"的断言会把样式表读成缺陷。

## 9. 判据单点清单（改前先 grep 这些名字）

`is_commonmark_escapable`（可转义字符集）· `backslash_run_escapes`（反斜杠奇偶）· `looks_like_currency_between`（`$…$` 是公式还是货币）· `void_inline_html_tag`（void 标签表）· `angle_bracket_destination`（尖括号目标）· `resolve_image_source`（图片路径）· `cell_separator_positions`（表格竖线）· `can_use_markdown_script_delimiters`（上下标记号，序列化与投影共用）· `split/compose_display_math_region`（公式读写一对）· `FileShape::detect_and_decode`（编码嗅探）· `BlockKind::parse_atx_heading_line_with_marker`（标题分隔符宽度）· `gpui::Img::extensions()`（图片扩展名，不手抄表）。

发现抄本就把判据收编到一处，而不是在抄本上再补一个特例。

## 10. 陷阱

- 按偏移切片前必须过字符边界收敛（鼠标与映射给出的偏移可能落在多字节字符内部；release 是 `panic=abort`）。
- `normalize_fragments` 合并相邻同样式片段，但**数学片段永不合并**、link/footnote 必须相等；给片段挂新属性后要重归一化，否则同链接的相邻片段会被序列化拆成两个壳。
- `split_at` 会把数学元数据置 None（公式是原子格，切开即丢）。
- 粘贴一律先 CRLF→LF（缓冲区是 LF 规范空间）；剪贴板 HTML 走 `markdown/html_paste.rs` 转 markdown，表格首行提升为表头。
- 表格单元格序列化禁止全局反斜杠加倍，换行必须折成 `<br>`（格必须单行）。
- 导出 HTML 的断言只看 `<body>` 之后：单文件导出把整张主题 CSS 内嵌在 `<head>`，样式表里就有 `.vlt-inline-math`、`[TOC]` 这类字面量。

## 11. 测试落点

`markdown/inline/tests/`（编辑操作、序列化往返与两路径一致）、`markdown/{html,table,image}/tests.rs`、`components/actions/tests.rs`；其余模块（高亮、公式、mermaid、脚注、链接、粘贴、frontmatter）的测试是文件内 `#[cfg(test)] mod tests`。消费侧回归在 `editor/tests/`（`round_trip_fidelity`、`block_source_write_back`、`source_mapping_fidelity`、`image_runtime`、`author_report`）。

> 这些模块外层常有 `use gpui::*`，子模块里的 `#[test]` 会被 gpui 同名属性宏遮蔽导致宏展开递归爆栈——测试模块要么精准导入，要么用 `#[gpui::test]`。
