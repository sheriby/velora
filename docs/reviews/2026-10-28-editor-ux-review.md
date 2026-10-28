# Velora Markdown 核心编辑体验深度评审报告

- 日期：2026-10-28　基线：`dev` @ 6be8a6f
- 范围：`src/editor/`、`src/components/block/`、`src/components/markdown/` 全部与「编辑、选择、跳转、复制、粘贴、删除、渲染、撤销、保存、搜索、表格、vim、公式」体验相关的代码（约 9.6 万行，不含测试文件）
- 方法：12 个并行只读评审子代理按模块逐行通读（缓冲区/持久化、写回换算、块树解析、选择命中、粘贴链、事件引擎、渲染管线、块元素输入、块运行时、行内模型、富媒体、表格 UI、vim、搜索大纲），主评审另逐行精读 selection/paste/block_event/mod/history/buffer/encoding 等核心文件；全部发现去重合并（同一根因只记一条，多处现场合并举证），已验证为「非问题」的疑点（如表格格子映射含管道符的怀疑）不列入。
- 阅读说明：每条含【现象/原因/改进】；标注「待验证」的条目表示证据链完整但触发条件需要动态验证，修复时须先独立复核真伪。

## 总览

| 章节 | 主题 | 条目 |
|---|---|---|
| 一 | 块树与 markdown 解析 | P1–P16 |
| 二 | 写回分档与坐标换算 | P17–P40 |
| 三 | 选择、光标与指针命中 | P41–P55 |
| 四 | 复制与粘贴 | P56–P62 |
| 五 | 事件总线与结构编辑 | P63–P70 |
| 六 | 渲染管线与滚动窗口 | P71–P85 |
| 七 | 块文本元素与输入交互 | P86–P100 |
| 八 | 块运行时缓存与外观 | P101–P118 |
| 九 | 行内模型：解析与序列化 | P119–P135 |
| 十 | 表格/链接/图片/HTML/高亮组件 | P136–P150 |
| 十一 | 表格编辑与编辑入口 UI | P151–P165 |
| 十二 | 撤销、保存与缓冲区 | P166–P186 |
| 十三 | 搜索、大纲、快速打开与命令面板 | P187–P202 |
| 十四 | 段落/格式/插入操作族 | P203–P213 |
| 十五 | 公式编辑器、补全、预览与拖放 | P214–P230 |
| 十六 | vim 模式 | P231–P246 |

---

## 一、块树与 markdown 解析

**P1｜引用块的懒续行被拆成独立段落，重投影还会多插空行**（document/parse.rs:690 `collect_quote_raw_region`；类别：缺陷｜高）
现象：输入 `> a` 紧跟一行无 `>` 的 `b`（CommonMark 懒续行，b 属于引用内段落）时，引用区域在 b 处断开，b 被解析成独立段落；渲染上引用只剩「a」，b 掉到正文，与 CommonMark 及导出侧 pulldown 语义直接冲突。更糟的是序列化端对「引用→段落」接缝固定补一个空行，未编辑的 `> a\nb` 在整篇重投影后变成 `> a\n\nb`。
原因：区域收集只认 `is_quote_start`，而懒续行按定义没有 `>` 记号；`build_native_quote_block` 的续行循环只在 dequoted 行数组内工作，永远见不到区域外的懒行。tree.rs:1373 的注释还写着「懒续行干脆没有记号」，说明序列化端预期存在懒行，解析端却收不进来。
改进：`collect_quote_raw_region` 遇非 `>` 行先判断是否前一段落的懒续行（非空、不是任何根块起点、引用处于未闭合段落），是则纳入区域并让 strip 账记宽度 0；`build_native_quote_block` 把该行按继承前缀 0 记入 `dequoted_origins`，保证位置换算不漂。

**P2｜带子块的引用一律按规范 `> ` 重拼洗写法；引用首部空行整行丢失**（tree.rs:1352 `Quote` 序列化分支；blocks.rs:237；类别：缺陷｜高）
现象：逐行宽度账只在 `widths.len() == body.len()` 时生效，而引用一旦有子块必然走 `format!("{indentation}> {line}")` 规范分支：文件里的 `>- item` 被写成 `> - item`、`>>` 写成 `> > `、无空格空行 `>` 写成带尾随空格的 `> `。更严重的是 `>\n> - item`（引用以空行开头）时 pending_blank_lines 被直接清零，那一行从模型里消失，保存后文件行数变少——真实数据丢失。
原因：宽度账的口径是「标题行数」，子块行的 `>` 记号记在子块自己的账里，父块无法按行落笔，只能整体退回规范形状；前导空行在标题与子块都为空的判断分支里被静默丢弃，且该分支没有任何测试覆盖。
改进：给 Quote 增加覆盖全行（含子块行）的宽度账，子块序列化后按行向父块回填「本行让开几字节」，序列化用每行宽度落笔；前导空行在标题为空时折叠进标题或落一个空段落子块，保证行数 1:1。

**P3｜列表项标记后直接跟公式（`- $$…$$`）往返变形，多出一个空标题行**（document/blocks.rs:574 `item_math` 分支；类别：缺陷｜中）
现象：`- $$\begin{aligned}…` 导入后项文本被清空、公式成为子块；序列化时项先落一行空标题（`- `），子块公式再缩进落笔，输出变成 `- \n  $$…`——3 行变 4 行、项标记与公式分离，与原文件形状不同。现有测试只断言 `\begin{aligned}` 出现一次，抓不到变形。
原因：该分支只考虑了「渲染端不能把 `$$` 显示成源码」，把公式整体搬进子块却没给序列化端任何「公式原本长在标记行上」的记账；`marker_is_math` 置 `prefixes_desynced` 后位置账也一并放弃，往返只能变形。
改进：为这一族记录原始形状（如 `math_on_marker_line: bool`），序列化时把子块公式首行并回项标记行、续行用子块缩进；或最小改动——项标题为空且有 math 子块时不落 `- ` 单独行，首子块首行前拼 `{marker} `。

**P4｜未闭合公式区域在空行处收尾时吞掉整段空行，序列化接缝再补一行，往返凭空多空行**（parse.rs:593 `collect_display_math_region`；类别：缺陷｜中）
现象：`$$\nx\n\n- item`（公式未闭合、空行后跟列表）解析时 `return lookahead` 把空行段整体划进块区域；序列化时该块落 3 行，接缝又补 1 个空行，全文变成 `$$\nx\n\n\n- item`——比原文多一个空行，且每多编辑一次就再涨一行，逐字节往返不变量被破坏。
原因：`return lookahead` 与 `return index` 之差正是「空行归块还是归接缝」的口径：归块后根级空行段逻辑看不到它，而序列化端 `root_separator_count` 无条件给非列表接缝补一行，两边各记一次。
改进：`region_ends` 时 `return index` 把空行段还给接缝，由根级空行逻辑按 run-1 保留 + 1 分隔恰好逐字节还原；若有意让块吞空行，则序列化端需要为「raw 块以空行结尾」的接缝免补那一行。

**P5｜`markdown_region_for_roots` 无条件弹掉末尾空行，可能把块自己的尾空行当接缝删掉**（tree.rs:702 `lines.pop()`；类别：缺陷｜中，待验证）
现象：区间写回前把区域末尾空行无条件 pop，注释假定它是「替下一块补的分隔」；但当区域里最后一根本身以空行结尾（典型：P4 造出的未闭合公式 raw 块 `$$\nx\n`）时，弹掉的是块自己的内容行——区域文本比模型少一行，按区间写回后文件丢一个换行，下一次整篇重投影又把它加回来，往返抖动。
原因：pop 的依据是「行内容」而非「这行归谁」：块区间的 `line_span.end` 在弹行前已记录，弹行后端点被钳到新文本末尾，账面看似自洽，实际把模型字节写短了。
改进：pop 前判断末空行是否属于最后一根块的 span（该块序列化以空行收尾时不弹），改由接缝侧补分隔；或让 `collect_root_markdown_lines` 把「接缝空行」与「块自身空行」用不同标记区分。

**P6｜多行 Setext 不识别；`===` 被当分隔线落笔成 `---`；单字符下划线两头不认**（import.rs:372；state.rs:516/221；类别：缺陷｜中）
现象：CommonMark 允许 setext 标题内容多行（`a\nb\n===` → h1）。本实现只在「新根块候选行」处查一行 setext，段落收集到 `===` 时因命中根块起点判据断开、走 Separator 分支，而 `SeparatorMarker::detect("===") 只看含不含 `*`/`_`，落笔成 `---`：标题降级成「段落+分隔线」，未编辑字节被改写。另外 `Title\n=`、`Title\n--` 因 `rest.len() < 3` 既不是标题也不是分隔线，退化成段落文本。
原因：setext 判据只有一个位置、只看一行；分隔线与 setext 在「连续 -/=」上语义重叠但没有「上一行是未收段落内容」时的优先级让渡；detect 只记录记号字符族不记录原文字节。
改进：段落收集器遇 `-`/`=` 游程行先回溯判定 setext 下划线（内容判据放宽为可连续多行）；至少把 `parse_setext_underline` 下限改为 1 字符并把原文字节记进账，写回照抄。

**P7｜行内反引号前瞻过粗：后文任意位置有同长反引号游程时，列表/标题/围栏行被吞进段落**（blocks.rs:29；parse.rs:97 `MarkdownLookahead`；类别：缺陷｜中）
现象：段落断行条件依赖 `has_backtick_after`，而它查的是「该游程长度在全文档最后出现的行号」（覆盖写）。只要段落里有一个未闭合单反引号（如笔误），且文档后文任意行再出现一个反引号，那么 `- item`、`# 标题`、甚至围栏行都会被判定「还在代码 span 里」吞进段落，整段结构塌掉；导出侧把同样文本解析成段落+列表，应用内外口径分裂。空行也会被吞（代码 span 按规范不能跨空行）。
原因：前瞻索引只按字面游程长度记账，不考虑闭合配对与距离，一个几千行之后的反引号就能让当前行不被判为根块起点。
改进：前瞻加两个约束——列表标记/ATX 标题/围栏行即使在代码 span 候选里也断段（与 CommonMark 块结构优先一致）；空行一律断段。更准可在行内层做配对扫描替代字面游程。

**P8｜段落后的 4 空格行被拆成缩进代码块，重投影再凭空补一个空行**（parse.rs:802；import.rs:437；类别：缺陷｜中）
现象：`some text` 紧跟无空行的 `    code-ish line`：CommonMark 规定缩进代码块不能打断段落，本实现段落收集器在 4 空格处断开，根循环收成缩进代码块；序列化时段落→代码接缝固定补 1 空行，原文被加行改写。纯文本诗行、ASCII 图、无空行粘贴的代码都会踩中。
原因：根块起点判据不区分「上一行是段落内容且本行是紧邻续行」；缩进代码分支没有像 setext 那样的「不可打断段落」检查。
改进：`collect_paragraph_block` 收缩进 ≥4 的行时，若紧邻（`pending_blank_lines == 0`）且非更强根块则按段落续行吞入（前导空格保留在模型文本里即可往返）；根循环的缩进代码分支只允许在空行之后或文档起点启动。

**P9｜列表缩进/续行/记号后写法按「每级两格+空格」规范化，未编辑行被改写**（blocks.rs:871；state.rs:901 `markdown_line`；类别：缺陷｜中）
现象：解析端逐行记了 `source_line_prefixes`，但列表项序列化只信 `markdown_line(list_depth)`：4 格嵌套的 `    - b` 落成 `  - b`；项内 3 格续行落成 2 格；`-\tfoo` 的 tab 整个消失落成 `- foo`（`ListMarkerStyle` 没有记号后空白/制表符字段）；`-   x` 靠「多出的空格留在 title 里」侥幸往返，续行与嵌套缩进无此侥幸。部分形状被测试烘焙成断言，但 `-\tfoo`、4 格嵌套等常见形状无测试，属静默改写用户未碰的字节。
原因：列表族的逐行宽度账只服务位置换算，写侧 `markdown_line` 是独立一套「按模型拼」的实现——同一语法两处判据，正是「一条语法只准一处判据」禁止的抄本关系。
改进：列表项分支像 Quote/CodeBlock 一样优先消费 `source_line_prefixes`（首行=记号宽度、续行=缩进宽度），对不上才退规范形状；`ListMarkerStyle` 增加 `marker_gap` 字段写回照原文。

**P10｜裸列表标记（`-`、`1.` 行尾无空格）认不出，退化成段落文本**（parse.rs:326/383；类别：缺陷｜低）
现象：CommonMark 允许空列表项：单独一行 `-`（或 `1.`）是合法空项，常用于快速起列表。本实现里 `-` 行既不是列表标记（记号后必须有一个空白）也不是分隔线，最终落成段落文本；`-\n-\n-` 三个空项变成三行段落。渲染上用户看到孤零零一个减号而不是空列表项。字节往返不丢，纯属解析认错。
原因：`parse_list_marker` 把「记号后必须有一个空白」当硬前提，而规范只要求「记号后跟行尾」即成空项；`parse_ordered_list_marker` 的 `separator` 检查同理。
改进：记号后无空白且已到行尾时返回空文本的列表项；`content_indent_columns` 取记号宽度+1 的规范值，序列化端对空标题按 `- ` 落笔即逐字节还原。

**P11｜引用定义 URL 换行写法不识别：`[foo]:` 成 raw 块、`/url` 成可见段落**（parse.rs:427/455/537；类别：缺陷｜低）
现象：CommonMark 允许链接定义的 URL 写在下一行（`[foo]:` 换行 `/url`）。本实现的定义区域收集只把「引号/括号标题续行」并入，裸 URL 续行被留给根循环解析成普通段落——用户看到一个突兀的 `/url` 段落，而导出侧把它解析为定义不显示。整篇重投影时 raw 块与段落接缝还补一个空行，字节被改写。多行 URL 更是完全不认。
原因：续行判据只覆盖 title 一族；URL 续行需要「上一行是未完成的定义（`]:` 后为空或无 title 收尾）」的前瞻判断，当前实现没有这条。
改进：定义区域收集在 `]:` 后为空（或上一续行仍缺 title）时，把下一个非空行并入区域，最多延伸两行并要求出现合法 title 或止于空行；认不出时保持现状回退 raw。

**P12｜列表项内每个子块都对「本项剩余区间」做完整 dedent 克隆，O(子块数×剩余行数)**（blocks.rs:634/727/847；parse.rs:276；类别：性能｜中）
现象：项体内每遇到一行更深缩进且命中子块分支，就把剩余全部行克隆成新 String。一个含 K 个子段的大列表项要克隆约 K×剩余行数次字符串。import_round_trip 的闸门测试宣称修掉了 O(子块数×剩余行数)，但它数的是 scan_probe（只挂在 Lookahead/InlineCodeState/matching_fence 上），dedent 的克隆完全不在计量内——闸门测不出来。
原因：子块消费是增量推进的，但 dedent 窗口总取到 `item_end`；多数情况下消费 1-2 行后剩余切片立即作废，属于典型「窗口取满、用量极少」。
改进：dedent 窗口截到本子块可能的跨度——先廉价探几行（子块起点判据只依赖首行+setext 前瞻 1 行），命中子块类型后再按消费量切片；或复用根块路径的「共享切片+line_base」方案。

**P13｜每次结构变更整棵 normalize+sync，每块两次实体写+children/content 克隆**（tree.rs:863/952/974；类别：性能｜中）
现象：任何结构事件（回车、删块、换 kind）都走 `rebuild_metadata_and_snapshot`：先对每一根块做 take_children + supports_children 判断 + 回写 children（两次实体写），再全树 DFS 重建快照（逐块克隆 children、收集 content UUID、写 11 个字段）。10 万根块的文档一次结构编辑就是 20 万次实体写——代码注释自己承认「10 MiB 一次按键那 175 毫秒的第一嫌疑就是这里」。
原因：normalize 对没有 children 的叶块也做 take+put（拿空 Vec 原样放回）；sync 对未变化子树的锚点/序号全部重算，没有「变更只落在局部」的短路。
改进：normalize 只在 `!children.is_empty() && !supports_children()` 时才动实体（其余路径零写）；sync 传 dirty region（结构变更本就知道锚点 location），只重算受影响子树并局部更新快照对应段。

**P14｜结构命令同步 flush 尾部用 usize::MAX 预算一次建完，大文档首次回车卡顿**（tree.rs:521/844；类别：性能｜低）
现象：巨大文档分批导入的好处是「先看到前几百根块」，但用户只要敲一次回车，`flush_pending_tail` 就以 `root_budget: usize::MAX` 把剩余全部行同步建完（对照异步路径用 STEADY_CHUNK_ROOTS 分步）。10 MiB 文档若用户在物化完成前开始编辑，第一次回车会同步构建剩余几万根块，主线程明显卡顿；区域重投影也因 pending 存在直接退回整篇重投影，放大这次成本。
原因：结构写回需要整棵根布局做 before/after 对比，于是选择「一次建完最简单」，没有利用 chunk 预算循环。
改进：flush 内部按 STEADY_CHUNK_ROOTS 循环同步跑完；或把「结构命令前必须全量物化」改为「只物化到锚点后 N 根，尾部 pending 保留为 raw 尾巴参与写回」。

**P15｜FrontMatter 探测扫到全文任意下一个 `---`，分隔线开头的普通文档被整篇吞成 YAML 块**（import.rs:255；类别：体验｜低，待验证）
现象：以 `---`（水平分隔线）开头的文档，只要后文再出现一个 `---`，从首行到那个 `---` 的全部内容——标题、正文、列表——都被并进一个 FrontMatter 块，渲染成 YAML 源码控件，用户打开文件只见一片「元数据」。现有测试只覆盖「没有闭合 `---`」的情形，挡不住这个形状。
原因：闭合探测没有内容约束：不要求闭合行紧随、不限中间行数、不看中间是否已出现其它根块结构，只要全文还有第二个 `---` 就吞。
改进：闭合搜索加常识约束（遇到首个空行后仍未闭合即放弃；或中间行出现 ATX 标题/列表标记等「绝不可能出现在 YAML」的形状即放弃），放弃后按 Separator 处理——根循环里本来就有该分支。

**P16｜引用相关块每次按键触发整篇元数据与可见快照重建**（events/quotes.rs:15 `refresh_rendered_quote_metadata_if_needed`；类别：性能｜中，待验证）
现象：`on_block_changed` 每次按键都调用该函数；只要编辑目标与引用沾边（quote_depth>0 或引用容器），就执行 `document.rebuild_metadata_and_snapshot(cx)`——名字与「整棵快照重排」同族，而不变量明令按键路径禁止整篇快照重排。在长引用里打字时每次按键都付 O(文档) 重建，与 P13 同一函数但触发器不同（前者是结构事件，这里是每键）。
原因：引用结构相关的判定过宽（quote_depth>0 即触发），而「是否真的需要重建」没有按键级别的短路；rebuild 本身也没有局部化。
改进：把触发收窄到「引用结构真的可能变」的编辑（行首列编辑、换行、跨行改动），或给 rebuild 传入受影响区域做局部更新；至少给按键路径加代际短路。

## 二、写回分档与坐标换算

**P17｜四个建表器对「续行前缀字节」的反表口径分裂，同一字节在两条路径解析出相差一行的内容位**（source_mapping.rs:263 对比 120/201/708；类别：缺陷｜中）
现象：量表路径（引用/列表里多行段落最常走）里，续行行首的容器记号字节（`> `、列表缩进）经 `source_to_content` 反查落到上一行内容的换行偏移；兜底拼前缀路径里同样的字节落到下一行第 0 内容位。消费方 `endpoint_for_source_offset` 与撤销选区恢复都用反表换回内容偏移，同一个落点在两条路径下恢复出的光标差一整行。
原因：`build_line_prefixed_content_mapping` 的反表靠零填充（`if *slot == 0 { *slot = previous; }`），续行前缀间隙继承的是上一行末尾；而 `build_prefixed_content_mapping` 在换行分支先推进 content_offset 再把前缀字节记为下一行首。两份代码对「前缀归谁」给出相反答案，注释声称「归最近的内容起点」与实现不符。
改进：把行间隙显式记为下一行首内容偏移，或在零填充后对每行 `[line_end+1, content_start)` 统一改写；补一条对拍测试：同一份「引用中的两行段落」分别走两条路径断言反表逐位一致。

**P18｜映射查询的提前 break 只认「target 自身有文本映射」，表格/分隔线/空段落作目标时白扫剩余整篇**（source_mapping.rs:2060；类别：性能｜中）
现象：`source_mapping_for_entity(表块/分隔线/空根段落)` 不仅返回 None，还从该块所在根块起把后面所有根块全部重建一遍映射。调用点全在热路径：滚动锚（每帧可调）、撤销选区捕获（每次撤销）、快速写回。文档后半段大块多时一次查询就是 O(剩余文档)，且结果注定 None。
原因：break 条件是 mappings 里出现 target id；但表格块只推格子映射、Separator 不推、空根段落只写 block_ranges，target 的映射永远不出现，break 永不触发。
改进：break 判据改为「本轮推过的根块的根祖先 == target 的根祖先」，或同时查 `block_ranges.get(&id)`，让表格/空段落目标也能在所属根块推完后立刻停住。

**P19｜脚注定义子块之间的接缝字节数硬编码 1/2，无视解析期记下的 source_separator_bytes**（source_mapping.rs:1746；类别：缺陷｜中）
现象：脚注定义体内两个子块隔着两个及以上空行时（文件里 3+ 个换行字节），走查只加 2 字节，后续子块的 absolute_start 整体前漂，块内偏移错位——打字落进上一块、选区恢复差几字节。主循环对普通子块用的是 `1 + source_separator_bytes`，同为「子块接缝」的两处口径不一致。
原因：硬编码分支按 previous_kind/is_list_item 给 1/2，两个段落子块一律按一个空行算；`source_separator_bytes` 是解析器看见空行段时当场记下的事实，这里没有消费。
改进：与主循环同一把尺：首块与头相接保持 1，其余用 `1 + source_separator_bytes`；补「脚注内两段隔两空行后打字」的落点测试。

**P20｜表格映射入口直接 slice 区间，没过 span_is_trustworthy 闸，坏区间当场 panic**（source_mapping.rs:894；类别：缺陷｜低）
现象：表格映射入口拿 `source_span_of` 的区间直接切片，没有可信性检查；`TextBuffer::slice` 内部是裸字符串切片，端点落在多字节字符中间会直接 panic——整个映射重建乃至一次按键的写回链路当场崩。runtime_context 处理同一来源区间时专门写了 `span_is_trustworthy` 过滤并注释「那会让整篇注册表重建当场 panic」，说明作者已认定区间可能不可信，但表格这条路没装同一道闸。
原因：slice 之前只有 None 检查；「倒置、越界、非字符边界」三件事只有 slice 自带的防倒置。
改进：与 runtime_context 同判据 `.filter(|span| self.buffer.span_is_trustworthy(span))`，不可信时退回按根扫描的路径或不推格子映射。

**P21｜无区间块（Raw/HTML/FrontMatter/脚注子块）每键触发整篇注册表重建**（runtime_context.rs:48-72/316-337；类别：性能｜中）
现象：编辑挂在容器里的 RawMarkdown、HTML 块或脚注定义子块时，每次文本变更都命中「可能改引用」的宽口径，随后 `rebuild_image_runtimes`：整篇 String 拷贝 + 两遍全文定义解析 + 把所有可见块与全部表格格子 clone 进 Vec + 逐块实体 update+notify。10 MiB 文档就是注释里自引用的 766ms 档，且每键。
原因：表格格子被收窄成窄口径，但「其余无区间块退回宽口径」这一档把 FootnoteDefinition、RawMarkdown、HtmlBlock、FrontMatter 无条件算「可能改」，而这几类块恰恰总是容器子块（无自己的 span），窄口径优化对它们完全不生效。
改进：把「编辑前后源码都含 `]:`/`[^` 才算可能改」的判据同样用于这几类（脚注 id 有结构化表示 `footnote_definition_id()`，Raw 块可对本块文本做记号扫描）；或让这几类块在解析期也记账获得 span。

**P22｜rebuild_image_runtimes 的另外两个无条件触发口：引用结构 normalize 与「插入块」**（history.rs:875-876；insert_ops.rs:121；类别：性能｜中）
现象：`normalize_rendered_quote_structure` 每次调用无条件 `rebuild_table_runtimes + rebuild_image_runtimes`（调用点是引用文本需要重parse 的每次按键路径）；「插入块」五类（分割线/目录/代码块/公式/FrontMatter）任何一类也整篇重建。三处（含 P21 的宽口径）都在没有「定义行真的变了吗」判据的情况下把最贵的 O(文档) 路径走满，10 MiB 档单次数百毫秒。
原因：`rebuild_image_runtimes` 的调用方各自维护「要不要重建」的判断，判据互不一致：normalize/insert 完全不判，runtime_context 只对表格格子判窄。
改进：把「contains_reference_definition_marker 命中才重建，未命中只 sync 本块」的窄判据收敛成一个公共入口，三处调用方统一走它；插入分割线/目录这类结构上不可能含定义的块直接跳过整篇重建。

**P23｜跨块删改、表格选区替换、多行粘贴三条高频路径每次都整篇重建投影**（selection.rs:741；selection/table.rs:557；events/paste.rs:59；类别：性能｜中高）
现象：`write_back_cross_block_source_edit`、`replace_table_text_selection_with_text`、`paste_multiline_through_buffer` 三条路径在缓冲区落笔后都调用 `rebuild_document_from_buffer`——整篇重解析建根块 + 重建表格运行时。哪怕只跨两段删几个字、表格里删一格文字、粘贴两行，大文档（几千块）上都付整篇重解析的钱；撤销条目虽是增量，但重建期间主线程无响应。
原因：注释自证「只有重建，根块才挂得上区间」——但这只在被编辑区域跨根块时才必须；受影响区间两侧的根块实体完全可以原样保留，只重解析区间内的根块。
改进：实现「区域重投影」：利用 edits 的区间定位受影响根块，区间外的块实体与区间原样保留，只对区间内重建并重挂区间；`reproject_root_region` 已有雏形，把它接进这三条路径。

**P24｜源码视图下每个按键整篇重序列化（O(文档)/键）**（mod.rs:2079/2110；类别：性能｜高，架构已知）
现象：`writes_through_the_buffer` 要求 view_mode == Rendered，源码模式下写回永远走不到按区间落笔的档位（`write_back_root_region` 对 Source 直接 false），每键 mark_dirty → `raw_source_text()` 整篇序列化 → 整篇写回 → 区间全重挂。作者注释自证历史实测：1 MiB 源码文档一次按键 2.28 秒。用户切到源码视图编辑大文档时打字延迟随文档线性劣化。
原因：源码/代码文档的块树是「整篇源码的一份投影」，按区间写回的账本还没为这一档建立（roadmap「阶段 2」）。
改进：源码模式的单块切片区间本来就存在（`source_slice_span_ranges`），打字完全可以只写自己那一片——把「可见插入」档位适配到源码分块（块内偏移=源码偏移，映射恒等），结构变更仍走整篇兜底。

**P25｜collect_single_block_source_mappings 对 Table/CodeBlock/Raw 等也先算 measured_first_line，白付行切片**（source_mapping.rs:1500；类别：性能｜低）
现象：`measured_block_prefix` 对每个进入走查的块先算一遍——包括 Table、CodeBlock、RawMarkdown、Separator 这些最终消费不到的 kind。它开头就要 line_of + line_range + slice（表格首行/长段落行可能上千字节）外加 quote 层层剥离，全部白付；`line_prefixes` 闭包还要为每个 inline kind 克隆一份 Vec<u32>。按键路径每次重建映射对沿途每个块都付一次。
原因：计算提前到 match 外，没按 kind 排除；消费点只在「有子块要继续走」的分支。
改进：把调用挪进 is_list_item/quote_container/FootnoteDefinition 的条件下，或让 `measured_block_prefix` 对 atomic kind 先走快路径（先 match kind 再碰缓冲区）。

**P26｜hit_overlaps 对零宽命中在 span.end 上用闭区间，交界空命中同时归前后两块**（source_mapping.rs:13-34；类别：缺陷｜低）
现象：零宽命中（正则空匹配、长度 0 的搜索结果）落在两块交界处时对前后两块都返回 true，与「零宽区间按起点归块」的注释矛盾；空匹配会在前一块末尾多渲染一个零宽高亮/多算一次命中归属。
原因：`hit.start <= span.end` 里的 `<=`——按「起点归块」语义，前一块的判据应是 `< span.end`。
改进：若注释是意图，把两处 `<= span.end` 收紧为 `< span.end`；若闭区间是刻意的（让文末光标归属最后一块），改注释并确认渲染侧对零宽区间不会画出重复高亮。

**P27｜产品路径 8 处 title.expect()，排除清单与分支集靠人工对齐**（source_mapping.rs:1553 等 8 处；类别：缺陷（规约）｜低）
现象：Heading/Paragraph/三类列表项/Quote/Callout/FootnoteDefinition 八个分支都以 `title.expect(...)` 取标题映射。今天不会 panic 依赖「1480 行排除清单」与「match kind 分支集」两处代码的人工对齐——将来有人在排除清单里加一种块而忘了同步八个分支就会生产 panic，编译期毫无提示。
原因：排除清单与 match 分支是同一枚举的两种手写投影，expect 把结构化事实降级成运行时断言，与「产品路径禁 unwrap/expect」的成文不变量冲突。
改进：收敛成 `has_title_mapping(kind)` 单一判据，title 的构造与八个分支都用它；或让 title 用 match 解包、其余 kind 由类型表达，消除 expect。

**P28｜Separator 与空标题 Quote 按模型序列化长度计长，文件写法非规范时同根后续块整体漂移**（source_mapping.rs:1534/1641；类别：缺陷｜低）
现象：根内分隔线的走查长度取 `markdown_line(...).len()`——模型写死 `---` 一类；文件里若是 `***`、`______` 或更长横线，长度差直接加到后续兄弟块的 absolute_start 上。空标题且无子块的 Quote 兜底路径拼出 `> `（2 字节），文件里空引用行是 `>`（1 字节），同根后续子块前漂 1 字节。根内最后一个映射被错误截断、中间映射整体错位。
原因：Separator 分支只算长度不量文件；空 Quote 落进兜底支，`build_prefixed_content_mapping("", "> ", "> ")` 拼出带尾空格的两字节。
改进：Separator 分支用 `buffer.line_range(line_of(absolute_start)).len()` 量文件实际行长（分隔线独占一行）；空 Quote 行同理。两者不必推文本映射，但长度必须按文件。

**P29｜measured_block_prefix 的 Heading 分支不处理缩进列，≥4 空格缩进的 ATX 标题量出 prefix=0**（source_mapping.rs:1096；类别：缺陷｜低，待验证）
现象：Paragraph 分支有 `strip_leading_columns(&rest, list_dedent)` 兜底，Heading 分支只有 `parse_atx_heading_line_with_marker`，后者最多容忍 3 个前导空格。列表项里缩进 ≥4 格的标题行解析失败 → prefix=0 → 该标题块的内容第 0 位对到行首缩进上，落笔/映射整体左漂缩进宽度。
原因：Heading 分支复用了「marker_len 自带 ≤3 空格」的巧合，没把 list_dedent 当回事；失败时返回未量准的 consumed 而不是 None，调用方无从知道没量准。
改进：Heading 分支先 strip_leading_columns 再解析，或解析失败时 return None 让调用方退兜底；补「列表项内四格缩进 ATX 标题」落点测试。

**P30｜recorded_block_line_prefixes 对越界行无守卫，prefix==0 的行按 0 字节记账静默漂移**（source_mapping.rs:1140；类别：缺陷｜低）
现象：循环里 `line_range(first_line + offset)` 没有 `range.start >= byte_len` 检查（同文件另外三个量取函数都有）。缓冲区行越界时 line_range 钳到文末返回空区间，prefix==0 的行得到 file_len=0，被当成「这一行占 0 字节」继续构建映射——full_source_range 与 file_lens 全部少算，块内后续行落点漂移且无信号。
原因：`if prefix > range.len() { return None; }` 的检查被空区间+零前缀绕过；账本行数核对核的是模型行数，核不了缓冲区行数。
改进：循环开头加 `if range.start >= self.buffer.byte_len() { return None; }`，与同文件「量不出就整块放弃」的既有约定对齐。

**P31｜install_pending_update 先 take 掉安装计划再启动，启动失败后计划丢失无法重试**（update.rs:183-199；类别：体验｜低）
现象：退出流程调 `install_pending_update` 时先把 `pending_install.take()` 取走；随后 `launch_install` 若失败（安装包被杀毒挪走、磁盘满、权限问题），只报错并把全局态清空——计划已经没了。用户处理完问题再点退出，plan 为 None 直接正常退出，更新静默丢失，必须重新检查+下载整个安装包。
原因：take → launch → 失败即弃的顺序；下载阶段的取消有 cancelled 原子位可恢复，安装阶段没有「失败保留现场」。
改进：失败分支把计划放回 pending_install（或 clone+成功后清除），让下一次退出重试；错误提示注明「已保留更新，修复问题后再次退出即可安装」。

**P32｜下载完成回调把全局互斥状态清理放在 weak_editor.update 闭包内，实体消失时 download_active 永久卡死**（update.rs:490-525；类别：缺陷｜低，待验证）
现象：下载结束后，`download_active = false` 等清理写在 `weak_editor.update(cx, ...)` 里面。窗口正常关闭时有兜底清理，但编辑器实体在窗口未关闭情况下被丢弃（分屏替换、实体迁移）时 update 返回 Err，闭包不执行，全局 download_active 永远为 true——此后所有窗口点「安装更新」都被拦下永远弹「正在准备」，只能重启应用。
原因：全局状态的生命周期绑在「某一个窗口实体还活着」上，而它本应是应用级的。
改进：把 update_global 的清理移到实体 update 之前独立执行；Err 分支补日志说明全局态未清。

**P33｜下载期间每 100ms 无条件 notify 全窗重绘，进度无变化也刷**（update.rs:480-489；类别：性能｜低）
现象：`futures::select!` 每 100ms 触发一次 `weak_editor.update(cx, |_, cx| cx.notify())`，即下载全程以 10Hz 强制重绘整个编辑器窗口。进度条只在 progress 原子值变化时才需要重绘，但 tick 分支不做差分直接 notify——网络慢、进度长时间停在 37% 时也一样每秒 10 次全窗重绘；通知面板未打开时同样照刷，低端机/大文档上是纯粹的后台功耗。
原因：tick 分支不读 progress.load() 与上次值比较。
改进：tick 分支差分比较，值不变跳过 notify；或轮询周期放宽到 250ms。

**P34｜导出文件名只在完全没有扩展名时补全，notes.txt 会存进 HTML/PDF 字节**（export.rs:133-135；类别：体验｜低）
现象：保存对话框返回的路径只有在完全没有扩展名时才补 .html/.pdf/.png。用户手输 `notes.txt`（或 `report.v2`、`2026.10`）会原样落盘——文件内容是 HTML/PDF 字节，扩展名却是 txt/v2/10，双击打开是乱码或用错程序；macOS 上保存对话框允许自由输入，这条路径很容易踩。
原因：只判断「有没有点」，不判断「点后面是不是目标格式」。
改进：判据改为「扩展名 ≠ 目标格式扩展名时追加」：`if path.extension().and_then(|e| e.to_str()) != Some(format.extension()) { 追加 ".ext" }`（追加而非 set_extension，避免把用户故意写的 .v2 覆盖掉）。

**P35｜show_export_error 用 let _ = 静默吞掉「无法显示错误」，连日志都没有**（export.rs:171-177；类别：缺陷（吞错）｜低）
现象：导出失败后向用户报错走 `editor.update` 弹应用内模态；返回值被 `let _ =` 丢弃。窗口此时若已关闭（导出是后台线程+oneshot，用户完全可能中途关窗），错误信息无声无息地消失——文件没写成，用户也看不到任何提示。同文件 spawn 失败、channel 断开都认真报了错，唯独「报错本身失败」被吞。
原因：`let _ = editor.update(...)` 违反「不许静默吞错」。
改进：改为 `if let Err(error) = editor.update(...) { eprintln!(...); }`，与仓库其余错误路径的日志口径一致。

**P36｜脚注注册表的定义键用 visible_text()、引用键用 fragment.footnote.id，两把尺无一致性保证**（runtime_context.rs:252/275；类别：缺陷｜低，待验证）
现象：定义注册键是 `title.visible_text()`，引用匹配键是行内解析器给出的 `fragment.footnote.id`。两把尺没有代码保证相等：id 含转义序列、全角/零宽字符，或定义标题经行内树规范化后与解析器截取的原始 id 有出入时，`bindings.get_mut(&footnote.id)` 查不到 → 该引用静默降级为「未解析脚注」，无任何诊断。
原因：两处各自取值，中间没有 debug_assert 或对账逻辑；脚注 id 在模型里有结构化事实（`footnote_definition_id()`），注册表却绕开它用了第二把尺。
改进：定义侧改用与映射层同源的 `footnote_definition_id()`；对「有 fragment.footnote 但无 binding」的引用计数进现有探针体系，让失配可被测试闸发现。

**P37｜end_block_pointer_selection_sessions 每次指针收尾全量克隆可见块表并遍历所有表格格子**（runtime_context.rs:111-151；类别：性能｜低）
现象：每次鼠标选择会话收尾（拖选结束/抬针即调）都执行 `visible_blocks().to_vec()` 把全部可见块的 Entity 句柄克隆进新 Vec 再逐个 update；接着把所有表格格子也 clone 进第二个 Vec 逐个 update。大文档一次抬针就是几百次 Arc 增减与实体写；多数会话里对绝大多数块都是 no-op，但「查一遍」的成本是全量的。
原因：句柄收集阶段没有「该块是否处于指针选择会话」的预筛，to_vec 只是为了绕借用。
改进：Editor 维护「处于指针选择会话的块 id 集合」（会话开始登记），结束时只遍历集合；或先收集 changed 的 id 再统一 update，避免整表克隆。

**P38｜物化流与用户编辑的交错依赖「打字不改行数」这一未成文的隐含约定**（mod.rs:1121-1180 + attach_root_spans；类别：隐患｜中）
现象：后台续建块用的行基址是打开时的行号；期间用户打字不改行数所以行号仍对，行数变化必然是结构事件而结构事件先 flush 尾巴——正确性押在这个不变式上，代码里没有断言或注释把「打字不改行数」钉住。若未来某个非结构事件改了行数（比如某类 IME 提交、自动列表续号），续建区间整体错位，字节写进别的块。
原因：设计依赖事件分类的完备性，但「哪些事件改行数」没有单一判据，靠开发者记忆维持。
改进：materialize 前校验 tail.next_line 对应行的内容签名（同撤销行签名机制），不匹配即先 flush 再续建；至少补注释+debug 断言钉住约定。

**P39｜reattach_root_spans 每次整篇重投影都重建 HashMap 并逐块 update 写 source_kind**（mod.rs:2164-2207；类别：性能｜低）
现象：整篇重投影后为 10 万根块建 HashMap + 逐块 update 同步 source_kind——每根块一次实体 update（10 万次实体更新）只为把 `record.source_kind` 对齐。当年 O(N²) 的线性 find 已收掉，剩余成本仍在撤销/切视图这类高频路径上。
原因：逐块 update 是为了绕借用与逐块计算；source_kind 的同步本质是「全部根块的同一字段批量赋值」。
改进：DocumentTree 增加批量接口一次落（树侧持 span 表与根块列表，循环内直接 block.update 也比 HashMap+zip 更省），或把 source_kind 同步合并进 replace_roots 的构建循环。

**P40｜恢复出来的 markdown 文档不带原始字节形状，保存时静默重编码**（mod.rs:2221-2256；类别：体验｜低）
现象：恢复快照只带文本，不 attach_file_origin——恢复出来的文档保存时按当前编码重新编码；若原文件是 CRLF/GBK/带 BOM，恢复后保存就换了形状（用户未被告知）。恢复快照设计如此（存的是规范化文本），但保存提示与 UI 上没有区分「这份文档已不是原文件的字节形状」。
原因：RecoverySnapshot 结构只有 markdown 文本，没有携带 FileShape 或原始字节。
改进：恢复快照增加 FileShape 字段（形状是几个枚举值，体积可忽略），恢复时 restore_file_shape；至少在标题栏/保存对话框标注「（恢复）保存将按 UTF-8/LF 重写」。

## 三、选择、光标与指针命中

**P41｜复制到剪贴板只有纯文本一个 flavor，粘贴到外部应用得到 Markdown 源码**（selection.rs:90-125 `on_copy_capture`；类别：体验｜中）
现象：⌘C 把 markdown 源码以 `ClipboardItem::new_string` 写入剪贴板，没有富文本 flavor。粘贴到邮件、聊天、Office 等外部应用时得到的是带 `**`、`#`、表格竖线的源码而非排版文本；用户必须在目标应用里手动清理记号，「复制」这个动作对「粘贴出去」的场景不友好。
原因：复制路径只产出单一字符串 flavor；导出侧明明有现成的 markdown→HTML 管线但没有接到剪贴板上。
改进：复制时同时写入 HTML flavor（复用 export/html 的块级渲染管线，只取选区范围），宿主剪贴板 API 支持多 flavor 时并存；纯文本复制命令保留现行为。

**P42｜纯文本跨块复制丢段落间空行语义**（selection.rs:878-911/955-998；类别：体验｜低）
现象：「复制为纯文本」把各块的可见文本用单个换行连接；两段之间原本是空行分隔（段落边界），粘贴到纯文本环境后段落结构信息丢失，所有行粘成一串紧排行。跨块选中「标题+两段正文」复制出来的纯文本无法凭空看出哪些行原本属于同一段。
原因：可见文本拼接只按块边界加 `\n`，没有查源码里块与块之间是否隔着空行。
改进：拼接时按块间源码是否有空行决定 `\n` 与 `\n\n`（块区间表现成，查一次即可）；或在纯文本复制里也保留空行。

**P43｜selected_markdown_text 无选区时 O(可见块数) 线性扫描**（selection.rs:1002-1052；类别：性能｜低）
现象：无跨块选区时从头线性扫所有可见块找非空 selected_range；`single_block_selection_candidates` 先链上表格格再链上全部可见块。复制/剪切是低频操作可接受，但同一条链路上 `selected_visible_text` 已经按「活动块优先」优化过（注释自证旧实现每帧读 16 万块实体的教训），另一函数却仍是 O(N) 全扫，若未来被挂到工具栏/状态栏的帧路径就重演旧账。
原因：两个函数的「找当前选区」逻辑各写一份，优化只落在其中一份上。
改进：把「活动块优先、退回可见序」的选区定位收敛成一个公共函数，两个复制口径共用；扫描路径保留但只在活动块拿不到时才走。

**P44｜endpoint_for_source_offset 距离平局取先遇到的块，块间空行归属不确定**（selection.rs:623-651；类别：缺陷｜低，待验证）
现象：offset 恰在两块之间（块间空行）时 `min_by_key` 距离相同，取迭代序在前的块；如果空行按语义归属下游块，则端点会落错块（光标恢复/选区重建落在上一块尾）。offset 在文档最前（如 frontmatter 之后）时 clamp 到第一块 0。
原因：块区间表不覆盖块间空行，min_by_key 的平局由迭代序决定，语义上没有「空行归谁」的判据。
改进：给距离函数加次级判据（平局时取区间起点更靠后的块，即空行倾向归下一块——与「块尾停在内容末尾」的口径一致），并补一条块间空行落点的测试钉住行为。

**P45｜mapping_source_offset 与 range_endpoint 版本在「盖住块尾」时行为不一致**（selection.rs:553-621；类别：缺陷｜低，待验证）
现象：插入点口径在 offset ≥ visible_len 时取内容末尾映射；区间端点口径在 clean_offset ≥ clean_visible_len 时直接取 full_source_range.end（含隐藏围栏），但中间值经 markdown_offset_map 换算后 `.min(max_content)` 截断——markdown_offset 超过 content_to_source 末项时端点落在内容末尾而非块尾，与 at_end 行为不一致。可能造成「选区盖住块尾最后 1 字符」时复制漏掉块尾隐藏记号（如闭合 `#`、尾随空格）。
原因：两条口径的截断策略不同：端点版本对「超末尾」直接给块尾，对「映射不到」却给内容末尾。
改进：端点版本对 markdown_offset ≥ 末项的情形同样返回 full_source_range.end（既然「 ≥ clean_visible_len」都给块尾，「映射算不到」更应保守取块尾）；补选区盖尾的复制/删除用例。

**P46｜replace_cross_block_selection_with_text 的相对选区未夹字符边界**（selection.rs:794-803；类别：缺陷｜低）
现象：selected_range_relative 以 `new_text.len()` 截断但没过字符边界——若调用方（IME 组合、模板插入）给出的相对区间落在多字节字符内部，映射出的源码区间端点落在字符中间，下游 apply_selection_snapshot 虽有 floor_char_boundary 兜底，但中间层先做了 `inserted_start + relative` 的算术，溢出/错位的中间值会流入标记区间（apply_marked_source_range）。
原因：相对区间的两个端点只做了 `.min(new_text.len())` 的长度钳制，没有做边界收敛。
改进：对 relative.start/end 先过 `clamp_to_char_boundary(new_text, …)`（表格路径的 source_offset 闭包已有同款做法），再算源码区间。

**P47｜apply_marked_source_range 跨块时静默放弃 IME 标记区间**（selection.rs:744-765；类别：体验｜低）
现象：跨块替换后 mark_inserted_text 的标记区间 start/end 实体不同直接 return——跨块粘贴/替换场景下 IME 组合标记丢失，组合输入的第二段文字不再与第一段关联，输入法行为退化为逐字独立提交。
原因：标记机制建立在单块 selected_range/marked_range 上，跨块插入没有对应的标记载体。
改进：跨块时把标记落到焦点所在块（新文本末尾那块）的块内区间，或显式放弃前给块发一次 CompositionChanged 让输入法收尾，避免组合状态悬挂。

**P48｜rendered_document_is_fully_selected 不识别反向全选**（selection.rs:213-235；类别：体验｜低）
现象：只识别 anchor={first,0}、focus={last,len} 的正向全选；用户反向拖拽（从文末拖到文首）出全选后第二次 ⌘A 不会短路，会重新执行全选——结果幂等但每次都对全部可见块做一轮 read+update，大文档上有可感知顿挫。
原因：全等比较没有先归一化方向。
改进：比较前把 anchor/focus 按可见序归一（或同时接受反向形状），第二次 ⌘A 直接短路。

**P49｜跨块拖选每次 mouse move 对挂载区每行分配 Vec 并逐块读实体**（selection/pointer.rs:332-409；类别：性能｜中）
现象：每次拖动 move 事件都对挂载区每一行执行 `plan_row.entities().filter_map(...).collect::<Vec<_>>()`（每行一次堆分配），并对每个成员块 `entity.read(cx)` 读 last_bounds。长文档挂载窗口几十行×每行数块，一次 move 几百次 tracked read+几十次分配；拖动是高频事件，几何在行计划稳定期间并不变。
原因：行成员几何没有按「本帧行计划版本」缓存；命中测试逐行线性扫。
改进：行计划构建时缓存每行成员的 (entity, bounds 快照)；move 事件先按 y 二分定位行，再在该行内挑块，消除逐行 Vec 分配与全量实体读。

**P50｜拖选过程中每次 move 全量重铺选区高亮**（pointer.rs:247-304 → selection.rs:471-529；类别：性能｜中）
现象：跨大段落拖动时每次 move 对选区内全部块做 read+update（比较 editor_selection_clean_range），选区跨几千块时一次 move 触发几千次实体读写——即使中间块的新旧区间相同（比较后有短路，但 read/update 本身已是 O(选区)）。
原因：visuals 同步以「整个选区范围」为单位，没有利用「本次 move 通常只改变头尾两端点」这一事实。
改进：记录上一次铺过的端点块，本次只更新端点变化的头尾块（中间块区间不变时跳过 update）；全量重铺只在选区首次建立/清除时执行。

**P51｜点击在滚动区外不清搜索导航选区**（pointer.rs:231-233；类别：体验｜低）
现象：只有点击落在 scroll_handle.bounds() 内才清 workspace.search_navigation_selection；点到滚动区外的编辑器空白（正文底部以下的大片区域）时搜索高亮残留，用户以为搜索状态还挂在某个命中上。
原因：清理条件用「点击在滚动区内」而不是「这次按下进入了编辑器」。
改进：把清理挪到「编辑器捕获到任何左键按下」的入口（滚动区内外的分界只在要不要落光标），保证一次按下必然收掉搜索导航选区。

**P52｜拖动期间行计划过期则静默不更新选区**（pointer.rs:259-263；类别：体验｜低）
现象：cross_block_endpoint_for_point 返回 None（visible_len 与 plan 不一致，如拖动中文档被后台重建）时 move 直接 return，用户继续拖但选区停在旧值，无任何反馈；松手后 drag 被清。极端时序下用户「拖了个寂寞」。
原因：过期时没有降级路径，也没有触发行计划重建。
改进：过期时请求一次行计划重建并跳过本帧（下一帧几何就绪后继续跟手）；或至少在拖动会话期间缓存上一个有效端点，用行号增量估算延续。

**P53｜跨格拖选每次 move 对整张表 O(行×列) 扫描并克隆 runtime**（selection/table.rs:242-274/279-325/328-382；类别：性能｜中）
现象：跨格拖动时每次 move：`table.read(cx).table_runtime.clone()`（克隆 Vec<Vec<Entity>>），随后全表双重循环，每格 cell.read(cx) 读 clean_visible_len 并 update 比较高亮区间；跨块选区盖住整表时同样路径。百行百列表 = 每次 move 万级 tracked read，拖动手感明显劣化。
原因：选区 sweep 没有利用 start..end 的行列范围（全表扫描后靠 key 过滤）；runtime 用 clone 而不是借用。
改进：只 sweep start..end 的行列（行优先线性序本来就有序）；runtime 改为借用（在同一个 read 生命周期内完成比较与收集），或缓存上次 sweep 的行列范围做增量。

**P54｜table_text_position_at_point 线性扫 table_cells 绑定表**（selection/table.rs:123-146；类别：性能｜低）
现象：`self.table_cells.values().find(...)` 每次按下/move 都线性扫全部格子绑定找 (table, position)，万格表上每次 mouse move 万次比较。绑定表是 HashMap<EntityId, Binding>，按内容查找只能线性。
原因：缺少以 (table_block_id, position) 为键的反向索引。
改进：维护一张 HashMap<(EntityId, TableCellPosition), EntityId> 的反向索引（绑定建立/拆除时同步），命中测试 O(1)。

**P55｜撤销无法恢复单格内的表格文本选区**（selection/table.rs:105-107；类别：体验｜低）
现象：`start.cell == end.cell` 时直接返回 false，撤销后格子内选区不恢复（只恢复光标）；跨格选区才恢复。表格里最常见的选择恰恰是单格内选几个字，撤销/重做后选区丢光。
原因：快照应用函数只处理跨格情形，单格选区的恢复依赖块级 selected_range 快照，而表格格子走了独立的 TableTextSelection 状态，两者没有接续。
改进：单格情形把快照换算成该格的块内区间写入格子的 selected_range（或转成 TableTextPosition 单格形状），与跨格路径共用 endpoint 换算。

## 四、复制与粘贴

**P56｜图片粘贴目标路径不转义空格，产出非法 Markdown 链接**（events/paste.rs:244-251 `markdown_image_target`；类别：缺陷｜高，待验证）
现象：生成的 `![alt](./screen shot 1.png)` 目标里的空格未转义也未包 `<>`，CommonMark 目的地含空格非法，多数解析器断在空格处——粘贴后图片不渲染、往返破坏。文件名带空格（截图默认名、Windows 复制路径）是高频场景。当前仅转义 `\ ( "` 四个字符。
原因：markdown_image_target 的转义表漏了空格（以及 `%`、换行）而目的地的解析端不会给含空格的目标兜 `<>`。
改进：目标含空格/非法字符时整体包 `<>` 或做 percent 编码（与 file_url 的 percent 判据同源，避免第二份转义表）；补「带空格文件名粘贴后可渲染」的用例。

**P57｜剪贴板 HTML 转换在主线程同步跑且 macOS 无大小上限**（html_paste.rs:46-73/158-168；类别：性能｜中）
现象：一次 HTML 粘贴最多 3 次 DOM 解析（表头提升、下划线中和、htmd 自身）+ 2 次序列化，全在主线程；macOS 分支不限制 HTML 大小，复制整个大网页（几十 MB）粘贴时 UI 冻结秒级。Windows 限 2MB 但超限直接静默放弃 HTML flavor（无提示），两个平台行为还不一致。
原因：转换管线没有大小防线与异步化；平台分支各自为政。
改进：macOS 同样设上限（与 Windows 对齐 2-4MB）；超限时提示「按纯文本粘贴」；转换挪后台线程，主线程只收结果。

**P58｜粘贴图片的磁盘写入在主线程同步执行**（events/paste.rs:189-238 `materialize_pasted_image`；类别：性能｜中）
现象：大截图（数 MB PNG）粘贴时同步 fs::create_dir_all/fs::copy/fs::write 阻塞 UI 线程，慢盘（网络盘、杀软实时扫描）上可感知卡顿——粘贴是高频动作，冻结直接打断输入节奏。
原因：物化图片是纯 IO，却在按键处理的调用链上同步完成。
改进：后台写盘 + 完成后回主线程插入 markdown；插入前先在 UI 上落一个占位（或直接等待），失败走应用内模态（现有 show_image_paste_error 已具备）。

**P59｜未命名文档粘贴图片把文件写进「当前工作目录」**（events/paste.rs:83-101 `image_paste_root_dir`；类别：缺陷｜中）
现象：文档无路径（未保存新文档）且无 workspace root 时，图片物化到进程 CWD（随启动方式漂移，可能是 / 或应用包目录），并插入 `./xxx.png` 相对路径——文档保存到别处后图片全部断链；且未经用户同意在任意目录落文件。
原因：兜底 `std::env::current_dir()` 是为了让功能「能跑」，但语义错误：CWD 与文档没有任何关系。
改进：无路径时先弹保存对话框（先有文件再有图片引用），或物化到应用数据目录并插入绝对路径，保存后提示「是否把图片移到文档旁并改相对路径」。

**P60｜产品路径里的 unreachable!() 与无上界探测循环**（events/paste.rs:127-141、162-175；类别：缺陷（约定）｜低）
现象：`image_target_dir` 与 `unique_file_path` 两处 `for index in 0..` + `unreachable!()`。LocalPath 分支在目录已存在时 continue 使循环继续探测 untitle1.assets…untitleN.assets——理论上磁盘全被占满时死循环卡 UI；`unreachable!` 违反仓库「禁 unwrap/expect，需写明为何不可能失败」的约定（这两处连注释都没有）。
原因：循环终止条件依赖「文件名空间不会被耗尽」这一物理假设，代码层无上界。
改进：设探测上限（如 1000），到顶后复用带序号名或返回错误走模态；unreachable 换成带注释的显式返回。

**P61｜孤儿图片清理静默吞错**（events/paste.rs:461-463；类别：约定｜低）
现象：删除未接收的图片文件失败（只读盘、权限、被占用）时 `let _ = std::fs::remove_file(&path)` 无任何日志，磁盘留孤儿文件用户不知情；Windows 上文件被索引器占用时这一支并不罕见。违反「不要静默吞错」。
原因：清理失败被当成无关紧要，但孤儿文件会污染工作区索引与图片预览。
改进：失败时 eprintln 带路径；连续失败可在状态栏给一次性提示「有 1 个未使用的粘贴图片未能清理」。

**P62｜多行粘贴 O(n²) 的 pipeless 表格探测**（markdown/paste.rs:14 `is_soft_wrapped_paste`；类别：性能｜中低）
现象：对每个行起点都调一次 collect_pipeless_table_region 做区域扫描，万行剪贴板粘贴时 O(n²)；粘贴虽低频，但大文本（日志、导出表格）时秒级卡顿在主线程上，粘贴前 UI 无响应。
原因：逐起点全量重扫，没有利用「表格区域一旦确定即可跳过其内部起点」的信息。
改进：外层一次线性扫描：命中表格起点后让内层消费掉整个区域，i 直接跳到区域末尾；或先廉价预筛（存在 `---` 分隔形状行）再做区域判定。

## 五、事件总线与结构编辑

**P63｜RequestFocus 对全部可见块逐个 notify，每次点击整篇重绘**（events/block_event.rs:1032-1041；类别：性能｜高）
现象：块获得焦点（点击正文任意处）时对 `flatten_visible_blocks()` 的每一块执行 update+notify，强制所有块元素重跑渲染；flatten 本身还要付每个 Entity 句柄的引用计数与 LeakDetector 登记（同文件自己的注释实测 10 MiB 档 16 万块克隆一次 17ms）。点击在大文档上是可感知卡顿，且点击是最高频的交互之一。
原因：这段代码想刷新「活动块样式」，但活动样式切换只涉及前一个活动块与新的活动块两块；逐块 notify 是全量兜底。
改进：只 notify 参与活动样式切换的前后两块；或利用 `document_selection_active` 共享 Rc 的指针相等机制（selection.rs 已有 ptr_eq 短路），让块在渲染期自行判断是否需要变化。

**P64｜结构/导航事件无差别 flatten 整篇可见块表**（events/block_event.rs:119；table_nav.rs:68；类别：性能｜中高）
现象：任何结构事件（最常见=回车、Backspace 合并）都先 `flatten_visible_blocks()` 整篇克隆再找 index，而 current_visible_index 只有 merge/delete/outdent 等分支用，RequestNewline 根本不用；表格边缘垂直导航同样为找相邻块整篇克隆。大文档每次回车付整篇克隆成本（作者自测 10 MiB=17ms），回车是最高频按键之一。
原因：可见序号查询走「克隆+线性找」，而 DocumentTree 已有 `visible_index_for_entity_id` 的增量索引。
改进：按分支惰性取（只在需要的分支里调 index 查询）；flatten 的结果在分支内复用，不在 match 外提前付钱。

**P65｜mark_dirty 兜底整篇重投影洗掉无关块写法（引用内粘贴、行尾补段落两处现场）**（events/block_event.rs:607/887；类别：缺陷｜中高）
现象：quote_related 时多行粘贴退回「块树重建+mark_dirty 整篇重投影」；行尾自动补尾段落同样 mark_dirty。mark_dirty 的 resync 从块树序列化全文写回缓冲区——按 selection.rs 注释的自证，该路径会把不相干块洗成规范化写法（表格列宽填充重算、`__`→`**`、Setext→ATX、CRLF 丢失），撤销条目也膨胀成全文。在引用里粘贴一大段、或在文档末尾按 ↓ 补段落，全文档其余部分的写法被静默改写。
原因：这两处没有像 RequestQuoteBreak 那样先按区间落笔再 skip_next_resync，直接交给了整篇兜底。
改进：引用区域按其根块区间写回（quote 容器的根块区间本来就盖住整根），行尾补段落按「锚点块+新块」区间写回；兜底范围从整篇缩到受影响根块。

**P66｜structure_unchanged 每次按键 O(可见块) 全比对**（events/block_event.rs:1104-1111；类别：性能｜中低）
现象：每个按键的 Changed 事件都把新可见序列与旧 id 序列 zip 全量比较；虽然比克隆便宜（EntityId 是 Copy），16 万块 ≈ 每键 16 万次比较。注释表明作者知道克隆的 17ms 成本并已优化掉，但 O(N) 比较仍在按键热路径上。
原因：结构是否变化用「逐位对比快照」判断，而不是「结构代际计数器」。
改进：DocumentTree 维护结构代际（结构变更 +1），按键路径 O(1) 比较代际；快照对比只在怀疑不同步时（debug/测试）开启。

**P67｜RequestOpenWikilink 的 defer 回调静默吞错**（events/block_event.rs:247-251；类别：约定｜低）
现象：`cx.defer` 回调里 `let _ = handle.update(...)`——窗口已关闭时 update 失败被静默丢弃。窗口关闭场景下失败可接受，但没有任何日志，出现异常行为时无从排查；同类场景在 update.rs 等处至少有 eprintln。
原因：defer 闭包拿不到好的日志通道，作者选择了最省事的丢弃。
改进：改为 `if let Err(error) = handle.update(...) { eprintln!("wikilink 打开被丢弃：{error}"); }`，与仓库其余错误路径口径一致。

**P68｜表格格子内每次按键克隆整张表的记录**（events/table_nav.rs:184-199 → sync_table_record_from_runtime；类别：性能｜中）
现象：格子里每敲一个字都 sync_table_record_from_runtime（把运行时格子内容同步回 record.table，含整表 TableData 克隆），再 write_back_table_cell_source 按格写回。大表（百行百列）打字成本随表大小线性涨——单格打字付整表克隆的钱。
原因：同步函数以「整表一致性」为单位，没有「只动的那一格」的增量路径。
改进：sync 增量化：只把活动格的 InlineTextTree 写回 TableData 对应单元（活动格位置在 binding 里现成），结构操作前再整表同步兜底。

**P69｜表格水平/垂直移动假设每行列数与表头一致，ragged 表静默失焦**（events/table_nav.rs:106-140/142-170；类别：缺陷｜低）
现象：`columns = runtime.header.len()`，线性索引 row*columns+column；若某行格子数少于表头（异常/历史文档/合并单元格预期），next 落点会越行取到 None 静默失败（`let _ =` 忽略返回值），焦点原地不动且无提示。垂直方向 `position.column.min(header.len()-1)` 同样按表头宽度钳制。
原因：表格运行时假定矩形，对 ragged 形状没有防御。
改进：按行取 `row.len()` 做边界；取不到目标格时把焦点交给相邻块（与「移出表格边缘」同径），而不是原地不动。

**P70｜容器内补尾段落没有缓冲区立足点，首次击键退整篇重投影**（events/structural.rs:334-357；类别：体验/性能｜中低）
现象：根级补尾段落时特意补分隔空行+挂零宽区间（注释解释了为什么）；容器内（quote/callout 里）只插块不挂区间，首次击键找不到落点退整篇重投影——别的块的字节（表格列宽、定界符写法）被这一次打字洗掉。注释自己承认了这个坑但未修。
原因：容器内没有「缓冲区末尾」概念，区间落点需要父容器的 span 收尾计算，作者选择了留坑。
改进：容器内按父容器根块区间末尾落零宽区间（根块 span.end 就是容器内容的字节末尾），与根级同构；至少先在 quote 单层验证。

## 六、渲染管线与滚动窗口

**P71｜换标签交还后的 row_floor 钳制，键盘向上滚动时留下持续性未挂载空白**（window_state.rs:259/675；类别：缺陷｜中）
现象：切回标签交还滚动现场后，用键盘/Vim 在长块内持续向上移动光标（vim_request_reveal 只置 pending_scroll_active_block_into_view），视口滚过 restored_row_start 对应内容位之后，其上方的行永远不被挂载——`run_start = run_start.max(row_floor)` 把挂载上界钉死，这些行整段渲染成 spacer，用户看到光标段落上方是大片空白，且不会自愈。
原因：restored_row_start 的清除点只有结构变化、宽度/主题变化、cancel_pending_scroll_restore 三处；键盘触发的视口移动（ensure_focused_caret_visible 直接 set_offset）不经过任何取消路径。
改进：交还现场生效若干帧（拿到真实几何）后自动清掉 restored_row_start；或 viewport_first < row_floor 时放开钳制并触发 needs_fill；至少让键盘滚动与滚轮同口径清除 row_floor。

**P72｜折叠过滤可能把 callout/脚注组从中间拆开，同一组画出两个壳**（render.rs:737-777；类别：缺陷｜中，待验证）
现象：组扫描要求 kept 数组中同 anchor 的成员连续；若折叠掉组内部的一个标题（其章节成员被滤掉），同一 callout anchor 的成员在 kept 里被切成前后两段，各自生成一个带边框/底色/内边距的完整 callout 壳——视觉上同一个引用框裂成两个、中段内容消失的位置出现双份边框。脚注组同理。
原因：行计划把「连续性」当成「同组」的判据，而折叠过滤只保证可见下标升序、不保证组内连续；组身份（anchor）与连续性在没有折叠时恰好重合。
改进：组扫描按 anchor 而非连续性归组；或对组内标题禁止折叠/折叠时整组隐藏；补「callout 内标题折叠」用例。

**P73｜源码/代码模式每个按键 O(全文) 行号重算并对全部根块无条件两趟 entity.update**（window_state.rs:969-1036；类别：性能｜中）
现象：源码视图或代码文档下每敲一个键：先 root_blocks().to_vec() 整表克隆，逐块 display_text().len() 累计（O(全文)），再调 lines_and_line_starts（遍历全部 chunks 的 O(全文) 扫描），随后两个循环对每一根根块各做一次 entity.update（写 reading_mode + set_source_line_gutter_basis），即使行号与 basis 根本没变。10 MiB 代码文件每个按键两次全文遍历加 2×块数次实体写锁，打字延迟直接受文档大小支配。
原因：行号与 gutter basis 的写入没有「值未变则跳过」闸门；重算没有利用 buffer 已知改动区间。
改进：行号与 basis 各加比较短路（值相同不 update）；basis 只在 buffer.line_count() 变化时刷新；行号批量换算只对受编辑区间影响的块做。

**P74｜跳转滚动估算用 index/total 平均比例，现成的精确估算器没被复用**（render/sync.rs:135-186 对比 window_state.rs:873-882；类别：体验｜中）
现象：⌘点击/搜索跳转/vim gg/G 时目标块未挂载，走 `content_height * (index+0.5) / total` 的均匀比例估算落点。行高不均匀的文档（首屏大表格/图片、后续几百短段）里这个估值误差可达数屏：首跳大幅偏离，随后依赖半屏爬行+90 帧预算逐帧逼近，用户看到先跳错、再缓慢爬的过程。Top/Bottom 对齐还把块中心估计当块顶用，系统性多偏半块。
原因：仓库已有按行计划前缀工作的 `estimated_scroll_y_for_block`（对已量行精确、未量行下界），估算分支没走它，两套估算器并存且精度差一个量级。
改进：估算分支优先取 estimated_scroll_y_for_block；Top/Bottom 用目标的 visible_start 对应前缀值。

**P75｜滚动条 thumb 长度按像素比、位置按行权重，两种口径混用**（window_state.rs:34-45；render/paint.rs:338-350；类别：缺陷｜低）
现象：thumb 高度按像素空间计算，thumb 位置却按「块最小行高×源码行数」的权重空间覆盖。渲染模式下源码行数与渲染高度严重不成比例（一个 3 行源码的标题可能渲染 200px），thumb 的长度不代表可视占比、中点不对应文档中点，拖动时抓点与内容出现比例错位感。scrollbar_geometry 里先算的那份 progress 实际是死代码。
原因：两半个口径各取一半；权重设计为了滚动进度稳定却牺牲了几何一致性。
改进：统一权重口径：thumb 高度改为「视口覆盖的权重跨度/总权重×track」，删掉死代码；或让权重∝实测像素（权衡滚动进度随测量漂移）。

**P76｜拖动滚动条期间 set_offset 不按 max_offset 钳制**（render/paint.rs:318-329；类别：缺陷｜低）
现象：拖动中每帧把 inverse 映射结果直接写回滚动句柄，只有下界没有对 max_offset 的上界钳制；拖动过程中文档高度收缩（渐进导入回退、外部修改重投影、行高测量使总高变矮）时，drag.max_scroll_y 还是拖动开始时冻结的旧值，inverse 结果可以超出当前可滚范围，产生一帧越界滚动/回弹。
原因：拖动会话的 max_scroll_y 在 start 时捕获，跨度期间分母变化没有重新钳制。
改进：拖动分支写回前按当前 max_offset 钳制；或每帧用当前 plan 重算 max_position 再参与映射。

**P77｜每帧深拷贝约 200 字段的 Theme，与热路径用 Arc 的设计注释直接冲突**（render/paint.rs:105-106；类别：性能｜低）
现象：每帧把整个 Theme（注释自述 ~200 fields and a String name）深拷一次，只为 apply_ui_typography 能拿 &mut 改十来个数值字段；theme manager 的注释明确写着「热渲染路径用 Arc 避免 ~200 字段拷贝」，paint.rs 恰恰在每帧热路径上把这个优化抵消了。
原因：apply_ui_typography 的签名是 &mut Theme，调用方只好先克隆；派生缓存没有覆盖「再套 UI 排版」这一步。
改进：ThemeManager 加第二个派生档（key = ui_size+zoom+theme_generation，缓存 Arc<Theme>），或把 apply_ui_typography 改成返回只含受影响字段的小结构体。

**P78｜AI 评审标记激活时每个挂载行每帧分配 Vec 并做 O(标记数) 扫描**（render/paint.rs:618；ai_assistant/experience.rs:443-473；类别：性能｜低）
现象：ai_review_marks 非空时，每个挂载行每帧都要取块 span、把本行全部块的 span 收集进新建 Vec，再对全部标记做线性扫。评审一篇大文档（标记数十上百、挂载行约 40）时每帧 40 次 Vec 分配+40×O(marks) 比较，纯为画每行左侧的评审色条。
原因：「行→命中标记」映射在标记集合与行集合都不变时被逐行逐帧重复计算。
改进：标记写入或行计划重建时一次性算好「行号→MarkKind」旁路表，渲染期 O(1) 查表。

**P79｜entity_is_mounted 线性扫挂载行，被 block_at_viewport_top 放大成 O(可见块×挂载行)**（window_state.rs:142-148/748-773；类别：性能｜低）
现象：entity_is_mounted 对目标 id 遍历挂载行（约 40 项），每项再逐成员比较；block_at_viewport_top（换模式取视口锚时调用）对全部可见块逐个调它。1 万个可见块的文档一次模式切换约 40 万次 id 比较，是可感知的同步卡顿点。
原因：挂载区是行计划上的连续区间外加一个岛行，块的 visible_index 与行号一比即知；现实现退化成逐行逐成员枚举。
改进：entity_is_mounted 改为 visible_index_for_entity_id + 所属行号判断 `run_start <= row < run_end || row == island.row`；block_at_viewport_top 复用同一判定。

**P80｜换模式视口锚点在源映射缺失时回退拼接显示偏移，落点可能错位**（window_state.rs:710-737；类别：缺陷｜低，待验证）
现象：渲染模式取视口顶锚点时，映射缺失分支直接把块内显示偏移当源码偏移拼接；渲染块的显示文本与源码不等长（`1)`→`1.`、`__粗__`→`**粗**`），只要存在「非 raw 且映射缺失」的块，换到源码模式后视口就落到错误的字节位置（可能落在标记中间）。
原因：注释假设「映射缺失时块内偏移就是同一坐标」，但代码没有任何 kind 检查兜住这个前提。
改进：回退前检查块是否 raw/代码类；非 raw 块映射缺失时退到 span.start 对齐块首，并加计数器观察回退频率。

**P81｜冷启动续挂 8 帧上限后彻底停止调度，极端行高下视口空白滞留**（render/paint.rs:34-35/390-395；类别：缺陷｜低）
现象：行高被严重低估时（一行真实 9000px、估 16px），needs_fill 连续为真 8 帧后达到上限、不再排续挂帧；此后没有降频重试也没有定时唤醒，视口里没铺满的部分保持空白，直到用户下一次输入/滚轮/光标闪烁帧顺手把渲染带起来。
原因：上限本意防每帧重排，实现却是「达到上限→无限期放弃」，放弃后没有任何状态记录与降级策略。
改进：达上限后切低频续挂（每 250ms 一帧直到铺满）；上限只应约束连续高频重排。

**P82｜视口尺寸一变就取消挂着的滚动交还并重新发起滚向活动块**（render/sync.rs:486-497 → window_state.rs:1038-1052；类别：体验｜低）
现象：窗口/分屏分隔条拖动导致的每次视口尺寸变化（超 0.5px 阈值）都会取消尚未应用的标签滚动现场（连带清 restored_row_start）并置 pending_scroll_active_block_into_view。刚点开一个标签、现场还没来得及在下一帧交还时，一次 resize 就把「回到上次阅读位置」静默丢掉；阅读模式下游览到文档中部后连续调整窗口，视口还会被反复拽向最后一次点击过的活动块。
原因：resize 处理复用了「请求滚到活动块」这条重路径，没有区分「现场交还进行中」与「常规保持活动块可见」。
改进：sync_scroll_viewport 在 pending_scroll_restore 存在时跳过取消（先交还再适配新尺寸）；或 resize 后只对已聚焦的编辑目标做 Reveal。

**P83｜菜单打开期间每帧重复分配标签数组并重算估宽**（render/paint.rs:1070-1138；render/menu_render.rs:55/188；类别：性能｜低）
现象：菜单/汉堡列表打开的每一帧：menu_labels Vec 新建、汉堡模式再走一遍、button_widths 重建并逐项跑逐字符估宽扫描、面板宽度被两处各自重算。菜单树本体已有按代数缓存，但由它派生的这些几何量每帧全部重算。
原因：这些量只依赖（菜单树代数，主题 dimensions），是天然 memo 键，却散落在四个调用点各算各的。
改进：与 window_menu_cache 同代数缓存一份「labels+button_widths+panel 宽度」派生结构（Rc），渲染期直接取。

**P84｜paint.rs 长持有 stride_prefix 的 Ref，与 set_stride 的可变访问构成借用 panic 地雷**（render/paint.rs:357；render.rs:199-218；类别：缺陷（潜在 panic）｜低）
现象：行 357 起 RefCell<StridePrefix> 被不可变借用并持有到 render 结束。当前所有 set_stride/fill_strides 调用都发生在 357 之前（逐个核对过没有 panic），但只要日后有人在 357 之后插入一次 set_stride（比如在行元素构建或 scrollbar 分支里顺手写回一个测量值），就会当场 BorrowMutError panic，且报错点离根因很远。
原因：共享访问器内部隐藏「脏则重建」的可变分支，调用方无法从签名看出「持有 Ref 期间不能写」的约束，纯靠调用顺序约定维持。
改进：在 357 处先 clone 需要的前缀片段立刻归还借用；或把 StridePrefix 刷新收敛到 set_stride 内部，使 stride_prefix() 成为纯读接口。

**P85｜焦点岛与挂载区之间未量行的估计高度变化时，岛的位置逐帧跳动**（render/window_state.rs:268-287；render/paint.rs:625-644；类别：体验｜低，待验证）
现象：光标块被钉成岛、与挂载区相隔大量未测量行时，岛的绝对位置与间隔 spacer 都用估计值；随着滚动使中间行逐步挂载、真实高度写回，前缀在岛与挂载区之间逐帧变长/变短，屏幕上表现为「远处的光标段落/活动块在视野边缘一格一格地挪」，滚动条 thumb 同步抖动。
原因：估计-测量收敛过程没有阻尼或一次性结算机制，每帧写回都直接改变可见布局。
改进：对岛 lead_h 采用冻结策略（记录创建时估计值，直到岛并回 run 才结算）；或测量写回时变化量小于阈值则推迟到下一帧批量应用。

## 七、块文本元素与输入交互

**P86｜展开超长行按容器宽换行，破坏「排版行↔源行」一一对应，行号/箭头/命中测试全部错位**（element/text_element.rs:286-358/467-470/877-909；类别：缺陷｜高）
现象：带行号槽的源码块中，点击行号箭头展开一条超长行（>1024 字符）后，该行以容器宽参与 shape，宽于容器时产出多条 WrappedLine——行号按 WrappedLine 逐条编号（把一条源行的每个折行都编号），long_line_markers 用源行下标去索引按 WrappedLine 累计的行号表，箭头指向错误行；命中测试把视觉行下标喂给按源行建的硬行表，点击落点整体错位。debug 构建下断言「每源行恰好一条 WrappedLine」直接触发。
原因：折叠行走不换行 shape，而展开的超长行与普通短行同等对待传入换行宽——与「layout 行↔源行 1:1」的前提自相矛盾。
改进：展开行语义二选一：行号与箭头按 plan.ranges 聚合多行顶部、命中测试经「WrappedLine→源行」映射表换算；或展开行也不换行交给外层裁切，并把断言升级为覆盖展开场景的回归测试。

**P87｜非源码块的块尾前向删除（⌦）是死键，不与下一块合并**（interactions.rs:465-523；类别：缺陷｜中）
现象：在普通渲染段落（含列表项、标题）的块尾按 Delete 没有任何效果——不会把下一块合并上来。与 Backspace 在块首触发合并形成不对称，渲染视图里无法用 ⌦ 跨块合并；WordDeleteForward 在块尾委托给同一函数，同样无效。
原因：on_delete 的普通块路径在块尾时 next_boundary 返回块长、select_to 无操作、删除空区间什么也不发生；只有源码 raw 分支发 RequestMergeFromNext。编辑器层 capture 只处理表格/跨块选区，动作冒泡后无人接管。
改进：普通块路径补上与 raw 分支对称的判断：选区空且光标在块尾时发 BlockEvent::RequestMergeFromNext（编辑器 400 行已有该事件处理臂），并补「渲染段落块尾按 ⌦ 合并下一块」测试。

**P88｜行内代码背景绘制链路三处 expect/unwrap，条件集合未按构造互斥**（text_element.rs:691/707-708；code_input.rs:203；类别：缺陷｜中）
现象：`wrapped_line_for_y(...).expect(...)`、`find(|run| run.font_size.is_some()).expect("inline code has a font-size run")`、`code_run.font_size.unwrap()`，语言输入框 paint 还有 `line.take().expect(...)`。release 下 panic=abort（仓库注释明确记录过这类 coredump 报修）。
原因：走 build_code_text_runs（font_size 恒 None）的条件与行内代码背景循环的条件未按构造互斥；is_source_raw_mode 是运行期状态，编辑模式切换瞬间同一帧两个读取点也可能看到不同值。
改进：find 不到 font_size run 时 continue 或退回 base font 量 ascent/descent；code_input 的 expect 改 if let；给两个条件集合补静态测试钉住互斥性。

**P89｜shape_text 失败被静默吞成零尺寸，整块文本无声消失**（text_element.rs:356/369；类别：缺陷｜低）
现象：按行 shape 与整块 shape 的 `Err(_) => return Size::default()` 把排版失败吞成零尺寸：块高度与宽度直接归零，该块从布局中消失，无日志、无事件、无降级路径，用户只会看到「内容不见了」；且 shape 备忘未写入导致每帧重试同样失败。零尺寸还会向上游容器传播错误约束。
原因：Result 的 Err 被丢弃错误值本体，违反「不许静默吞错」。
改进：至少 eprintln 记录失败与文本代数；更稳妥是失败时退化为单 run shape，再不行才返回保守尺寸（硬行数×行高）保证块可见。

**P90｜双击选词在尾随空白文本上选中空白段而非最后一个词**（interactions/keys.rs:473-497；类别：缺陷｜低）
现象：文本以空白结尾（如 `"word "`）时在词尾右侧双击，选中段是那个尾随空格而不是 word：选区高亮几乎不可见，用户感知为「双击没反应」，与主流编辑器「双击词尾选中最靠近的词」不一致。
原因：at_tail 判定命中空格段后 break 直接采用，没有向前回退到最后一个非空白段。
改进：at_tail 命中的段若纯空白则继续向前找最后一段非空白；补 `"word "` 尾部双击 → 选中 0..4 的用例。

**P91｜全编辑器缺少 Shift+Up / Shift+Down 纵向扩选**（components/actions.rs:334-344；interactions/keys.rs:8-32；类别：体验｜中）
现象：块内或跨块按 Shift+↑/Shift+Down 不产生任何扩选——没有对应 action、没有键位绑定。横向 shift 扩选（Shift+←/→/Home/End）齐全，纵向缺失；多行段落或代码块里想用键盘从行中间向下选几行只能鼠标拖选，是基础编辑能力缺口。
原因：actions 表的 up/down 只映射 FocusPrev/FocusNext，全表没有 shift-up/shift-down；keys.rs 没有 SelectUp/SelectDown action 与实现；move_cursor_vertically 只支持移动光标不支持保持锚点扩选。
改进：新增 SelectUp/SelectDown action（键位 shift-up/shift-down），handler 复用 move_cursor_vertically 的落点计算但走 select_to；跨块时发 RequestFocusPrev/Next 的扩选变体（需编辑器侧传 preferred_x）。

**P92｜代码块中 Tab 用四个空格整体替换选区（多行选区内容被毁），Shift+Tab 无反向缩进**（interactions.rs:589-638；类别：体验｜中）
现象：代码块里选中多行后按 Tab，选中的全部内容被替换成四个空格——不是给每个选中行加缩进，内容直接丢失（可撤销但破坏性直观）。反向操作 Shift+Tab 在代码块里完全无效（on_outdent_block 只处理列表嵌套与表格），而 Tab 又确实插入空格，进出不对称。
原因：on_indent_block 对代码块一律 replace_text_in_range(None, "    ")，有选区时整段替换，没有「选区含换行时逐行前缀插入」分支；outdent 无代码块实现。
改进：代码块且选区跨行时逐行插入前缀（按硬行表找选区覆盖的行，逐行插入）；on_outdent_block 对代码块删除每行行首至多 4 空格/一个 Tab。

**P93｜Home/End 作用于硬行首尾，软换行段落里跳过视觉行**（interactions.rs:739-741；keys.rs:4-6/106-122；类别：体验｜低）
现象：段落按容器宽软换行后，光标在第三视觉行按 Home 直接跳到整个段落（硬行）的 0 位而不是本视觉行行首；End 跳到硬行末尾。竖屏窄窗口下的长段落（正是所见即所得主场景）里 Home/End 与用户视觉预期不符，Shift+Home/End 扩选同样跨过若干视觉行。
原因：cursor_line_display_range 基于硬行，on_home/on_end 直接 move_to；排版层已有视觉行几何（wrapped_row_offsets）但 Home/End 链路没有消费。
改进：提供 VS Code 式「先视觉行、再硬行」的两段式 Home（或设置项）；复用 cursor_bounds_for_offset 求当前视觉行，再映射回文本偏移。

**P94｜行号槽每帧逐行重新 shape，无缓存**（text_element.rs:467-493；类别：性能｜中）
现象：带行号槽的源码/源文件块每次 prepaint（每帧）都对每个行号标签调用 shape：500 行的文件每帧 500 次文字整形。行号内容、字体、字号在滚动、选择、光标闪烁（33ms 一帧）期间完全不变，这笔开销持续支付；与正文 ShapeMemoKey 的精细备忘形成反差。多分块源码文档同屏多块时开销相乘。
原因：prepaint 里直接构建 Vec<ShapedLine>，键（行号数值、字体、字号）与 PrepaintState 一起每帧重建；块上没有类似 shape_memo 的槽位。
改进：行号整形结果缓存在 Block 上（键：source_line_start、行数、font_size、字体指纹、主题代数），文本代变化只补差量行；行号是纯数字 ASCII，命中率应接近 100%。

**P95｜词级移动/删除每次按键对全文重建词索引**（interactions/keys.rs:34-68；text_ops.rs:683-702；类别：性能｜低）
现象：Option+←/→、⌥⇧+←/→、⌥+⌫ 等词级操作每次按键都对整块文本跑 unicode_word_indices 全量扫描：53KB 的源码分块每按一次就分配并遍历数万词元，长文档按住 ⌥+→ 连续跳词时每键 O(n)，输入延迟随块大小线性增长。
原因：从头遍历的迭代器无缓存；display_text 未变时代数缓存本可承载词表。
改进：在 display_generation 维护惰性缓存的词起点表（UAX#29 边界），二分查找相邻边界；或至少用局部扫描（从 offset 向前/后到最近词边界）。

**P96｜源码分块文档里 Cmd+A 只全选当前分块**（interactions/keys.rs:93-104；类别：体验｜中）
现象：打开源码文件（按行分块）后按 Cmd+A，只有当前聚焦的那一个分块被全选，而不是整份文档；渲染文档同样按键却通过编辑器级全选。同为「文档视图」，全选口径不一致，且源码视图下用户几乎必然期望全文档。
原因：on_select_all 以 show_source_line_numbers() 分流，源码分块恰好命中块内分支；编辑器层没有别的 SelectAll 兜底。
改进：区分「块属于源码分块文档」与「独立带行号块」：前者向编辑器发源码版全选事件（编辑器把所有分块置为跨块选区——selection.rs 的源码跨块快照机制已具备）。

**P97｜未聚焦块上 shift+点击丢失旧锚点**（interactions/keys.rs:458-465；类别：体验｜低）
现象：在块 A 选中文字后，直接 shift+点击另一个未聚焦的块 B，B 会把光标落到点击点而不是从 A 的锚点扩选——已有选区被丢弃。注释自认「跨段的 shift 扩选不在这一笔」，但对用户而言是 shift 扩选随机失效：同一个手势，目标块是否已聚焦结果完全不同。
原因：块层只在 was_focused 时才走扩选，编辑器层没有捕获跨块 shift 点击。
改进：块层在 shift && !was_focused 时发 RequestShiftExtendTo { position }，由编辑器用现存跨块选区锚点扩展（extend_cross_block_selection_at_point 已具备）；无锚点再回退 move_to。

**P98｜剪贴板同时含图片与文字条目时图片静默优先**（interactions/keys.rs:26-31/251-260；类别：体验｜低，待验证）
现象：图片来源探测在读取文本之前执行 find_map——只要剪贴板条目序列里存在图片，即便同一 ClipboardItem 还带文字（部分应用复制时同时挂 image+text flavor），粘贴结果就是图片，文字被无声丢弃，用户无从知道为什么粘出来的不是预期内容。
原因：先找图片再取文本，两者之间没有「有条目文本则优先文本」的仲裁；⌘⇧V 也走同一个图片优先分支。
改进：item.text() 非空且非空白时优先按文本处理，仅当剪贴板只有图片条目时才落图；待验证宿主剪贴板是否真产出混合条目（macOS 常见）。

**P99｜代码字号设置可抬高所有正文块的行高**（text_element.rs:129-140/445-450；类别：缺陷｜低，待验证）
现象：行高保底 `window.line_height().max(code_size * 1.35)` 对每一个块生效，注释辩称「这个值恒小于本块正文行高」——只在 code_size ≤ body_size 成立。若用户把「代码块字体大小」调到大于正文号/1.35，保底值会超过正文行高，所有段落、列表、标题（无论有没有行内代码）的行距都被抬高，排版整体松散且与主题排版意图脱节。
原因：inline_code_font_size 对普通段落原样返回用户设置的 code_size，无上限约束；request_layout 与 prepaint 两处用同一公式一致放大。
改进：行高保底只应作用于实际含行内代码的行，或对保底项加 `code_size.min(font_size) * 1.35` 钳制；补设置组合的布局测试（待验证偏好 UI 是否已限制 code_size 范围）。

**P100｜双击与三击共用同一路径，三击不选行**（interactions/keys.rs:444-453；类别：体验｜低）
现象：click_count >= 2 的分支同时覆盖双击与三击（三击仍走选词），三击无法选中整行/整段——所有主流文本编辑器的标配手势缺失；连续快速点击四下五下也永远停在词选，用户没有鼠标外快速选段落的手段。
原因：分支没有 == 2 精确匹配，也没有三击分支；行几何现成未接入。
改进：拆分 click_count == 2（选词）与 >= 3（选行；软换行段落建议选硬行与 P93 的行定义一致），三击后拖动按行粒度扩展。

## 八、块运行时缓存与外观

**P101｜混合可视渲染路径丢失斜体与删除线，聚焦/失焦外观跳变**（render/inline_visuals.rs:717-900；类别：缺陷｜高）
现象：只要段落含行内公式、上下标或 `![`（中文笔记配公式极其常见），未聚焦时整块走混合可视路径逐段 div 渲染。该路径只处理了粗体、行内代码、上下标、下划线与高亮底色，全文件 grep italic|strikethrough 零命中——`*斜体*` 与 `~~删除线~~` 在失焦状态下渲染为正常直立文字、没有删除线；一点进块编辑（切回文本元素路径）斜体/删除线突然出现，聚焦瞬间外观跳变。
原因：普通路径的样式来自 build_text_runs（italic/strikethrough 两条都有），混合路径重建 div 时只搬运了 font_weight 分支，样式在两条渲染路径间不对称。
改进：混合路径补齐 italic → .italic() 与 strikethrough → .strikethrough()；抽一个「InlineStyle → div 样式」公共函数供两条路径共用，避免下次新增样式再漏；补「混合路径与 run 路径样式等价」对照测试。

**P102｜marked_range 五处直接赋值绕过 set_marked_range 宣称的唯一出口**（runtime/display_cache.rs:46/49/127-128/199-200/411；类别：缺陷｜中，待验证）
现象：text_ops 明文规定改 IME 组合跨度的唯一出口是 set_marked_range（CompositionChanged 是编辑器收撤销组的唯一信号，少发的后果是下一次编辑的字节并进这个组，一次撤销退两步）。display_cache 有 5 处直接写 marked_range 不发事件：IME 组合进行中若发生注册表同步（如别处敲出 `[^1]:` 定义触发重解析本块、可见长度变化导致映射后的跨度值改变），跨度变更不会发出事件。
原因：同步函数直接赋值，值可能改变但事件缺失；normalize 的相等比较基准还是被同步污染后的中间态，可能吞掉一次本应发出的变更。
改进：5 处直接赋值改为集中走一个静默赋值入口并在出口 diff 旧值补发；apply_title_edit 里在 sync_render_cache 之前快照旧 marked_range 作比较基准。

**P103｜[[wikilink]] 普通单击即打开文件，与行内链接的 Cmd+Click 口径相反**（render/inline_visuals.rs:848-892；类别：体验｜中）
现象：混合可视块里 `[[目标]]` 的 mouse_up 判 click_count >= 1 即打开——单击（包括想点进词中间放光标、或拖选结束在词上的那一下释放）都会直接打开/创建工作区文件。而同文件普通链接明确要求 Cmd/Ctrl+click 才打开（注释写明 plain click 落回编辑）。同一个块里两种链接交互口径相反，且 mouse_down 未 stop_propagation，单击同时改变光标位置又跳文件。
原因：wikilink 分支只做了 click_count 判据，没有检查 modifiers.secondary()，也没有区分纯点击与编辑意图。
改进：与普通链接对齐——secondary 修饰才打开，或至少双击触发；mouse_down 按需 stop_propagation；两类链接保持同一悬停手型与提示语。

**P104｜混合路径中链接/脚注永远没有下划线，underline 处出现死条件**（render/inline_visuals.rs:784-818；类别：缺陷｜低）
现象：脚注分支与链接分支都提前 return，其后的 `if underline || link.is_some() || footnote.is_some() { element.underline() }` 对 link/footnote 永远为 false（死代码）。含公式段落里的链接只有颜色加手型光标，没有普通文本路径必有的下划线；带下划线样式的链接文本也丢下划线，聚焦前后观感不一致。
原因：早退 return 的顺序把 underline 甩在两个分支之后，link/footnote 的下划线职责早退前没做、早退后够不着。
改进：underline 计算移到两个 tooltip 分支构造之前（或分支内复用加了下划线的 element），删除死条件。

**P105｜inline_word_chunks 只按空白切词，中文整段成一个 flex 项，断行协作失效**（render/link_cursor.rs:71-79；类别：体验｜中，待验证）
现象：混合可视路径用 flex_wrap 实现「文字与公式/图片同处一行并在词间换行」；但按空白切词对中文这类无空格文本返回单一 chunk，整段中文成为一个 flex 项：该项超宽后占满整行内部换行，flex 只能在项与项之间折行，于是行内公式/上下标/图片会被整体挤到下一行，即使前一段文字最后一个视觉行还有大量空间。产品以中文为主，这是混合排版的常见形态。
原因：没有字符级 fallback；英文按词切分的方式对 CJK 无效。
改进：对不含空白的 chunk 追加按字素簇限量切分（每 8-16 字素一段），或把纯文本 run 交给单个支持续排的文本元素、仅把公式/上下标/图片作为兄弟项插入；补「中文段落+行内公式」布局测试。

**P106｜entry_offset_for_vertical_focus 对空行布局做 len()-1 下溢 panic**（runtime/text_ops.rs:471；类别：缺陷（防御性）｜低，待验证）
现象：`lines.len() - 1` 与随后直接下标只防了 last_layout 为 None，没防 Arc<Vec> 为空 Vec 的情形：0-1 在 release 下回绕成 usize::MAX，随后下标直接 panic（release=abort）。同文件 move_cursor_vertically、index_for_mouse_position 等全部用 get+or_else 兜底，唯独这里裸算。
原因：目前布局对空文本也产出占位行，空表理论不可达；但该不变量只存在于另一文件的实现细节里，任何未来改动都可能产生空表。
改进：改成 checked_sub 并在 None 时回落 visible_len()，或入口 is_empty 早退，与同文件其他入口的防御口径拉齐。

**P107｜Table/CodeBlock 臂整体丢弃 render_shell 的焦点壳，容器 padding 区点击无响应**（render/paint_parts.rs:1064-1437；类别：缺陷｜低，待验证）
现象：有 table_runtime 时表格臂返回的顶层元素是手工拼的裸 div——没有 track_focus、key_context、任何 action/mouse 监听，这些全部挂在 render_shell 生成的 focused_base 上，而 focused_base 在该分支构建后被丢弃。点击表格容器自身的 padding/缩进区不会触发块级命中链（聚焦/放光标），行为与其它块不一致；且每帧白建一个带 30+ listener 的 shell 元素树。CodeBlock 臂同样丢弃。
原因：为了用 items_center 居中包裹而另起炉灶，没有把 focused_base 当宿主。
改进：把表格/代码块宿主作为 focused_base 的 child（保留 track_focus 与 mouse 事件），或显式确认容器不参与指针交互并删掉无效构建；为表格 padding 区点击补命中测试用例。

**P108｜[TOC] 块聚焦后没有文本元素，无光标、无回显**（render/paint_parts.rs:1555-1593；类别：体验｜低）
现象：目录臂对聚焦与失焦一视同仁地只渲染目录行列表，从不挂文本元素也不挂 bounds 记录器。用户点进 TOC 块后看不到光标、打字无回显（键盘 action 仍会改 record.title 但没有可见反馈），也无法直接把 `[TOC]` 改成别的文字。
原因：实现时把 TOC 当纯展示控件处理，遗漏了「块聚焦」这一态；其它只读形态（图片块、公式块失焦）都有对应的聚焦回落分支。
改进：聚焦时回落到普通段落文本分支，或给容器补占位光标与「目录由编辑器自动生成」提示；至少 focused 时保留透明文本元素承接 IME。

**P109｜renders_nothing 早退位于源码模式分派之前，孤立闭合标签块在源码视图也不可见**（render/paint_parts.rs:22-24；类别：缺陷｜低）
现象：`if self.renders_nothing() { return 空div }` 在一切分派之前执行；渲染层对源码 raw 模式的 HTML 块同样早退——内容为 `</div>` 的 HTML 块在源码文档视图里既不渲染文本元素也不挂 mouse 事件，用户无法把光标放进这行修改。同时早退不清 last_layout/last_bounds，块从可渲染变为不渲染的瞬间旧几何残留，指针命中测试按旧 bounds 返回偏移。
原因：早退初衷是渲染视图不画「空 HTML 文档」，但条件没有排除源码编辑模式。
改进：早退条件加 `&& !self.is_source_raw_mode()`，并在早退分支清 last_layout/last_bounds，与 Quote 空容器臂的做法对齐。

**P110｜空段落收窄臂不清 last_layout/last_bounds，命中测试沿用陈旧几何**（render/paint_parts.rs:259-272；类别：缺陷｜低）
现象：段落文本变空且失焦后渲染只输出一个最小高度壳，不再挂文本元素——last_layout/last_bounds 停留在块还有内容那一帧的值（可能几行高）。此状态下 index_for_mouse_position 以陈旧 bounds 参与判定：点击视觉上属于下方块的区域，只要落在旧 bounds 内就可能把光标塞进这个看不见的空块。同一文件 Quote 空容器臂与公式/mermaid 失焦臂都显式清了这两个字段，唯独此处遗漏。
原因：收窄臂假设空块不需要几何，但读侧仍会读旧值；字段清理责任散在各渲染臂里，没有统一出口。
改进：该臂返回前同样清两个字段，或把「渲染为无文本形态时清几何」收进 render_shell/render 的公共尾部，消除臂间不一致。

**P111｜insertion_attributes_for_current_offset 三处直接下标访问 fragments**（runtime/normalize.rs:299/311/323；类别：缺陷（防御性）｜低）
现象：三个分支都写 `&self.record.title.fragments[segment.fragment_index]`。当前正确性完全依赖「投影永远与当前 fragments 同代构建」这一跨文件不变量；投影的生存期横跨多次调用（缓存键命中时直接复用），任何未来在「改 fragments 之后、重建投影之前」插入读路径的改动都会越界 panic——release 即 abort。同文件另一函数对同一字段用 .get()，口径自相矛盾。
原因：直接下标把不变量的维护成本转嫁给所有未来调用点，违反「产品路径禁直接下标」。
改进：统一改 .get() 并在不一致时回落默认插入属性；或投影附带构建时的 fragments 长度，读取前 debug_assert+回落。

**P112｜代码语言输入 replace_code_language_text_in_range 切片前只夹长度不夹字符边界**（runtime/code.rs:233-238；类别：缺陷（防御性）｜低）
现象：range 只做 min(len) 钳制后直接字符串切片——偏移未做 is_char_boundary 收敛，也未防 end<start 倒置区间（拼接结果错乱）。现役调用方恰好都安全（IME 按整字符累计、鼠标 clamp、键盘 grapheme），但这是对外的 pub(crate) 编辑入口，与同类入口都自带 clamp 的风格不一致；多字节语言名一旦有新调用方传字节中点即 panic。
原因：防线建立在「所有调用方自觉传合法偏移」上，而不是入口自身收敛。
改进：入口处对 start/end 先 min(len) 再 clamp_to_char_boundary，并 start=min(start,end) 防倒置；补多字节语言名的 IME 替换测试。

**P113｜caret_affinity_for_clean_offset 用 visible_text().len()，每次调用整树拼接分配**（runtime/cursor.rs:42；类别：性能｜低）
现象：为了拿一个长度，把整棵标题树所有 fragment 拼成一个新 String 再立刻丢弃。该函数位于热路径：clean_range_to_display_range 每个端点各调一次，后者在存在跨块选区时每帧调用；一个 100KB 的长块被选中时每帧无谓分配两次 100KB 字符串。同文件已有零分配的 visible_len()（只求和）。
原因：把返回 String 的拼接版 visible_text() 与零分配的 visible_len() 混用。
改进：改用 `record.title.visible_len()`，一行改动消除热路径分配；顺手审查编辑路径上其余 visible_text() 调用，能换 visible_len/is_empty 的都换。

**P114｜HtmlBlock 每帧深克隆整棵 HtmlDocument**（render/paint_parts.rs:1440-1448；类别：性能｜低）
现象：`record.html.as_ref().cloned().unwrap_or_else(...)` 对节点树+多层 String 做整棵深拷贝，随后渲染只按引用读取。HTML 块每帧（光标闪烁 33ms 一拍、悬停、选区变化都会触发）付一次 O(节点数) 分配；一个含大表格/长列表的 HTML 块每次重绘都是完整 DOM 复制。数据本身不陈旧，克隆只为绕开一个不存在的借用冲突——渲染函数取引用即可。
原因：unwrap_or_else 需要 owned 值兜底，作者顺手把正常分支也 cloned 了。
改进：match 拆开：Some 时直接传引用渲染，None 时 parse 后渲染；同理审查表格臂每帧的 table_runtime.clone()（O(单元格) 引用计数）是否可改借用。

**P115｜公式块 $$ 前后的 Text 分段每帧重新解析**（render/inline_visuals.rs:278-287；类别：性能｜低）
现象：失焦的数学块每次重绘（含光标拍所在的窗口级重绘）都对定界符之外的普通文字做完整行内 Markdown 解析再拼接。同块内公式分段的 SVG 结果有主题/字号级缓存，Text 分段却没有任何缓存；公式块前后有长说明文字时，每次重绘多付一次 O(文字长度) 的解析+分配。
原因：解析→可见文本内联在渲染闭包里，没有像 title_markdown() 那样挂代数键；块文本不变时结果恒同。
改进：按 display_generation（或 title_revision+markdown 串）为 Text 分段可见文本做 memo，或直接用已有的渲染缓存可见文本替代二次解析。

**P116｜含 ![ 的普通文本块每次编辑做两轮独立图片解析外加一次全量序列化**（runtime/image.rs:57-105；类别：性能｜低）
现象：sync_render_cache 每次都调 sync_image_runtime。凡 fragments 里出现 `![`（包括正在讨论图片语法的普通段落），先整树拼接+parse 一轮；失败后再 serialize_markdown()（标题刚被编辑过、memo 必未命中，等于全量序列化）+ 第二轮 parse。在「讲图片的段落」里打字，每键多付两次全文扫描与一次完整序列化。探针只在测试构建生效，线上无法观测命中频率。
原因：挡板只挡住「与图片无关的块」，没有区分「独立图片块」与「正文恰好含 ![ 的块」。
改进：对非独立图片形状先做廉价预判（starts_with("![") 且 trim 后单行）再进入完整解析；序列化 fallback 加「visible 文本与上次相同则跳过」代数键；release 下保留低开销计数。

**P117｜sync_render_cache 在块未聚焦时也会重建投影，批量撤销付成倍无效功**（runtime/display_cache.rs:17-30；类别：性能｜低）
现象：keep_projection 只检查「投影存在」与「模式支持」，不检查焦点。块失焦后正常情况下下一帧就会清投影；但在失焦与下一帧之间若发生 mark_changed/注册表同步（编辑器批量操作、撤销恢复常对非焦点块调这些），sync_render_cache 会先按「保持投影」分支执行全量投影构建（走查+缓存重建+选区端点亲和换算），下一帧又被整体丢弃。批量撤销还原大量块时这是成倍的无效功。
原因：投影「是否应存在」的判据散落两处（模式维度与焦点维度），sync 只看前者。
改进：给 Block 增加缓存焦点态，keep_projection 加焦点条件；失焦路径直接走精简分支，与 clear_inline_projection 语义合并。

**P118｜表格轴激活条覆盖表头内容热区，行选择带负偏移依赖父级不裁切**（render/paint_parts.rs:1124-1265；类别：体验｜低）
现象：列轴激活条叠在表头单元格顶部并拦截滚轮/拖选——表头贴顶的文字上方一小条无法用于文本选择，只能用于列选择，用户难以察觉这条隐形热区。行选择带用负 left 伸到块外，依赖所有祖先不设 overflow_hidden——一旦未来给块壳或引用容器加裁切（图片块、HTML 块已有 overflow_hidden），行柄直接消失且无告警。
原因：激活热区与内容热区没有互斥（只在无内容处透明覆盖）；负偏移定位把稳健性寄托在外层布局约束上。
改进：激活条 hover 时才提升为热区；行带改为不越界的内嵌布局（容器内左侧留白槽），并在 quote/callout 容器下做回归截图。

## 九、行内模型：解析与序列化

**P119｜链接标签丢失强调写法标记，`__[a](u)__` 保存后变成 `[**a**](u)`**（inline/links.rs:386-415；类别：缺陷｜高）
现象：文档里写下划线强调包住链接（中文写作常见），一次加载→保存循环后写法被改写成星号形式，违反「强调记号写法落盘、未编辑部分逐字节还原」两条核心不变量。星号写法不受影响，因此现有测试语料没有暴露。
原因：链接标签不经 emit_token 输出，而是先重解析再由 apply_extra_style_to_fragments 并入外层样式；该函数只逐位设置 bold/italic 等布尔位，唯独没有 emphasis_marker 的传递——外层 `__` 解析出的 `Some('_')` 到标签片段处变成 None，序列化时 unwrap_or('*') 写出 `**`。两条样式合并路径判据不一致，正是「一条判据抄出第二份」的漂移。
改进：apply_extra_style_to_fragments 补 emphasis_marker 传播（或统一抽 merge_inline_style 供两处调用，消除第二份判据）；补 `__[a](u)__` 与 `_[a](u)_` 的逐字节往返测试。

**P120｜链接 title 转义不对称，含 `\` 或 `"` 的标题每次保存多一个反斜杠（无界增长）**（inline.rs:271-280；link.rs:332-347；类别：缺陷｜高）
现象：源码 `[a](u "ti\"tle")` 解析出的 title 原样保存 `ti\"tle`；序列化转义成 `ti\\"tle`；再解析时按原文存为 `ti\\"tle`——比上次多 1 字节，每次保存继续增长永不收敛。含转义引号或反斜杠的标题都会触发；现有测试的 title 不含这些字符，未暴露。
原因：escape_link_title 是「写时转义」，但解析的 title 分支只做字面截取不反转义——存储值与源码字节相同，写回时再加一层转义，转义就不是对合（involution）了。destination 侧有反转义做对合，title 侧漏了同一步。
改进：解析的 title 分支做 CommonMark 反斜杠反转义（复用 is_commonmark_escapable 单点判据），使「解析反转义 ∘ 序列化转义」为恒等；补含 `\` 与 `"` 的 title 逐字节往返测试，断言二次往返不再增长。

**P121｜嵌套链接外层吞掉内层，保存时内层目标被静默删除**（inline/links.rs:23-40；parse.rs:1070-1073；类别：缺陷｜中）
现象：源码 `[a[b](c) d](e)` 加载后不编辑直接保存，盘上变成 `[ab d](e)`：内层链接目标 `c` 从文件里消失。CommonMark 语义是「链接不能嵌套、最内层优先」，本实现让外层赢，两者对同一段源码读法不同；更实质的是未编辑内容的字节被改写且信息净丢失。
原因：label 扫描允许 label 内再含 `[...]`，于是 label 吞掉了内层链接；对 label 重解析得到内层链接片段后，无条件用外层 link 覆盖内层，内层目标既不在文本也不在任何元数据里，序列化自然写不出来。
改进：与 CommonMark 对齐：label 内检测到已成链的片段时放弃本次外层识别（让 `[` 按字面走）；至少在重解析产物带链接时拒绝识别，避免净丢失。

**P122｜相邻同目标链接被合并成一个，`[a](u)[b](u)` 保存后变 `[ab](u)`**（inline/parse.rs:1075-1087；tree.rs:1216/338；类别：缺陷｜中）
现象：两个紧邻、目标相同的链接解析后变成一个片段，保存时写出 `[ab](u)`：两个可独立编辑/点击的链接塌成一个，源码字节被改写（可见文本恰好不变，界面无感，重新打开才发现）。
原因：InlineLink 是纯值类型按字段比较，两个相同目标的 link 恒等；解析的合并循环没有「两个 link 来自源码中两个不同语法节点」的概念，树模型里也没有任何 span 边界信息存活到序列化。
改进：解析每次链接调用结束置「禁止与上一片段合并」标记（转义处理已有同类先例），normalize_fragments 与序列化 run 分组尊重它；或 InlineFragment 加 link_boundary 位。补往返测试。

**P123｜强调解析缺「rule of 3」与闭合优先匹配，`**a*b**`、`*a**b*` 的界面读法与导出不一致**（inline/parse.rs:581-784；delimiters.rs:56-118；类别：缺陷｜中）
现象：`**a*b**`：本实现读成字面 `*`+斜体(a)+字面 `**`；CommonMark/pulldown 读成 strong(a*b)。`*a**b*`：本实现读成两段斜体、可见文本 `ab`；CommonMark 读成 em(a**b)、可见文本 `a**b`——连可见文本都对不上，导出 HTML 与编辑器渲染不同文。保存时编辑器会把 `*a**b*` 重写为 `*ab*`，字节与文字双改写。
原因：解析是「开定界符驱动+最近候选闭合」的递归下降，没有实现 CommonMark 两个关键规则：closer 驱动就近匹配、以及「既能开又能闭的定界串，两串长度之和为 3 的倍数则禁止匹配」。全仓搜不到 sum-of-runs 检查。
改进：在闭合判定处加同一条 rule of 3 判据，单点函数供解析与序列化两侧调用；长期改 delimiter-stack 算法。至少为这两个输入建立与 pulldown 对表的测试，明确接受或修复。

**P124｜HTML 行内写法未编辑保存时被改写成 markdown 记号**（inline.rs:53-68；stacks.rs:147-177/309-315；类别：缺陷｜中，是否有意待确认）
现象：`<strong>x</strong>`、`<em>y</em>`、`<del>x</del>`、`<sup>x</sup>`、`<ins>x</ins>`、`<b>`/`<i>` 加载后不编辑直接保存，分别被改写为 `**x**`、`*y*`、`~~x~~`、`^x^`、`<u>x</u>`。同仓库对 `<span style>` 的改写与 shortcut 引用改写都被测试固化为有意，唯独本条无测试也无注释声明。
原因：写法模型只记 `*` vs `_`，不记「原文是 HTML 写法」这一维度；解析走 with_bold() 把 marker 定成 `*`，序列化 DP 的 preference_rank 让 markdown 候选必胜。
改进：若口径是「HTML 写法也是用户写法」，InlineStyle 增加来源标记并在序列化时优先写回；若口径是「统一规范化」，在文档与测试中固化该行为（仿照 span 样式的做法），避免静默改写被当成 bug 反复报修。

**P125｜脚注/行内公式切断强调 run，`**a[^1]b**` 保存后变 `**a**[^1]**b**`**（inline/tree.rs:324-346；类别：缺陷｜低）
现象：强调内部包含脚注引用或行内公式（解析完全支持）时，序列化把一个强调 span 拆成两段、脚注/公式夹在中间的裸文本。语义无损（重解析得到相同可见文本与 span），但未编辑内容的字节被改写，违反往返不变量。
原因：run 分组把 footnote/math 片段绝对排除，脚注走独立输出，前一个 run 被迫闭栈、后一个重新开栈；markdown 本身允许这种写法，序列化器没有「跨 raw 片段保持栈」的表达。
改进：run 序列化器允许 footnote/math 片段原样嵌入当前栈内（前后不做栈切换），偏移映射路径同步修改；补往返测试。

**P126｜autolink 判据缺 scheme 校验，`<foo>`、`<http://a b>` 被当成可点击链接**（inline/links.rs:129-151；类别：缺陷｜中，待验证是否有意放宽）
现象：`<foo>`（无 scheme）成为 autolink 渲染成链接，点击时被分类为本地路径去打开当前目录下名为 foo 的文件；`<http://a b>`（目标含空格）与 `<a:b>`（scheme 仅 1 字符）同样被接受。CommonMark 要求 autolink 是 `<scheme:…>`（scheme 字母开头、2–32 字符）或邮箱形且内容不含空格；pulldown 导出把这三者按字面输出，界面与导出分叉。文档注释与实现不符。
原因：locate_autolink 只找 `<`、扫到第一个 `>`、用 HTML 标签长相排除，没有任何 scheme/空格/邮箱判据（file_url::url_scheme 现成可用）。
改进：补 CommonMark 判据：合法 scheme 前缀且 target 不含空白，或邮箱形（mailto 语义）；判据单点放置；被拒的 `<...>` 按字面走，往返不受影响。

**P127｜`![^1](x)` 这类 alt 以 ^ 开头的图片被脚注分支抢食**（inline/parse.rs:947-956；类别：缺陷｜低）
现象：源码 `![^1](img.png)`（图片 alt 文本为 ^1）解析成：字面 `!` + 脚注引用 `[^1]` + 字面 `(img.png)`——图片写法被拆散，渲染成感叹号、上标 1 和括号文本，与作者意图完全不同；重新保存后结构也回不去。
原因：parse_until 对 `[` 的处理顺序先脚注后链接；脚注分支只检查当前与下一 token，没有像链接那样检查前驱是否 `!`。
改进：脚注分支开头加与链接同一条的前驱检查（`!` 或 `]` 前驱时放弃），判据共用一个小函数，让 `![^…` 保持原始文本交给图片路径。

**P128｜shortcut/collapsed 引用保存时被改写成 full 形式，与往返不变量冲突**（inline/links.rs:78-125；inline.rs:234-242；类别：缺陷（不变量冲突，行为被测试固化）｜低）
现象：`[shortcut reference]` 与 `[collapsed reference][]`（CommonMark 合法，引用链接最常见形态）解析后序列化一律写 full 形式 `[x][x]`。未编辑内容保存即被改写；label 很长时文件体积膨胀。该行为被测试显式断言——有意为之，但与「parse→serialize 必须逐字节还原未编辑部分」的不变量直接冲突，属于「写法落盘」维度（collapsed vs full）未建模。
原因：InlineLink::Reference 只存 label+destination，没有记录源码用的是 full/collapsed/shortcut 哪种形式；序列化模板固定 full。
改进：InlineLink::Reference 加形式枚举，序列化按形式写回；或至少在不变量文档中把该改写列为已知豁免并在 CHANGELOG 说明。

**P129｜`<br/>`、`<br />`、`<BR>` 未编辑保存统一改写为 `<br>`**（inline/tree.rs:565-568/741-750；类别：缺陷（写法未建模）｜低）
现象：源文件里的 `<br/>`、`<br />`、`<BR>`（HTML 习惯写法各异）加载后不编辑直接保存全部变成 `<br>`。注释声明序列化统一写回 `<br>`——有意为之，但与「写法落盘/逐字节还原」不变量冲突；且换行写法（行尾两空格、反斜杠）在模型里有区分记载，唯独 br 标签自身的大小写与斜杠形式没有。
原因：line_break 只是一位布尔，解析时把整段 HTML token 塌缩，原始写法即被丢弃；序列化端是字面常量，偏移映射也按 4 字节硬编码。
改进：若接受规范化，把该行为写进不变量文档并在「内容未变则不重写」短路里豁免；若要保真，给 line_break 片段附带原始标签字符串，序列化与偏移映射按原文写回。

**P130｜escape_literal_text 的 visible→markdown 映射在多字节字符内部字节上是空洞（值 0）**（inline/delimiters.rs:288-324；类别：缺陷｜低，可达性待验证）
现象：对含多字节字符的文本，映射表只给字符起始字节赋值（循环跳过内部字节），初始化的 0 留在内部字节槽里——查询多字节字符第 2、3 字节得到 markdown 偏移 0 而不是该字符的位置，任何把非字符边界偏移喂进反查的调用方会把光标/高亮打到段首。同文件 code span 版本按字节恒等填充无此洞——两份实现填充口径不一致。
原因：循环体只在入口写字符起点槽，内部字节从未被写。
改进：内部字节槽也填上同一字符的映射值（与 code span 版本口径对齐）；补多字节文本逐字节查询单测断言单调不减且不回 0。

**P131｜脚注 run 偏移映射用比例插值，多字节脚注 id 会得到落在字符内部的偏移；公式还被抄了第二份**（inline/tree.rs:397-414；cursor.rs:178-192；类别：缺陷｜中低）
现象：脚注 `[^注意]` 的可见文本是 3 字节上标而原文 9 字节，映射按长度比例线性插值——插值结果正好落在「注」字内部。任何后续用该偏移对 markdown 源码切片的代码在 release 下直接 panic；即使不切片，IME 组合区间、搜索高亮也会错位。cursor.rs 又抄了一份同款插值公式，两处一旦一处先修就漂移，违反「一条判据只准一处」。
原因：脚注 run 可见文本与原文长度不等只能近似，但比例近似对多字节 UTF-8 没做字符边界收敛，而脚注 id 明确允许 CJK。
改进：插值结果统一过 clamp_to_char_boundary 再入表，并把「插值+收敛」抽成单一函数供两处共用；补偏移表单调性与边界性单测。

**P132｜定界符预扫描对每个开定界符全量扫剩余 token，未闭合时回滚重扫——长段落最坏 O(n²)，每键重复支付**（delimiters.rs:56-118；parse.rs:736-772；tree.rs:1145-1177；类别：性能｜中）
现象：段落里每出现一个可开定界符，闭合预判就从该位置线性扫到块尾（含逐 token 匹配与转义跳过）；预判命中但正文扫描拒绝时还要回滚并把同一段重新按字面输出。对 `*a*a*a*…` 这类长输入整体 O(n²)；编辑路径每敲一键都对整块重新解析，打字卡顿随段落长度非线性增长。
原因：预判没有对候选位置做索引或提前剪枝；同一 token 区间可能被外层、内层、回滚三条路径各扫一遍；块级无增量解析。
改进：预扫描改单遍预计算（一次线性扫描建 char→候选下标表），闭合预判变查表；回滚后记录「该定界符已判死」避免重复尝试；中期评估按编辑区间局部重解析。

**P133｜autolink 候选判定把剩余全部 token 拼成字符串并整体转小写——每个 `<` 一次 O(n) 分配**（inline/links.rs:344；类别：性能｜低）
现象：每个以 `<` 开头、走到 autolink 判定的 token 都会把当前 token 之后整段（可能数千字符）collect 成 String，再整体转小写再分配一份，然后只为一个 contains 子串查询。一段含 100 个 `<` 的 10k 字符段落约 100 次×20KB 的临时分配，叠加 P132 的每键全块重解析构成分配风暴。
原因：判据只需「后续是否出现闭合标签」，实现却先物化整个后缀字符串；大小写不敏感匹配用整体转小写而非逐字符比较。
改进：在 token 数组上做不区分大小写的滑动匹配（指针前进即可），或预扫一次建立闭合标签出现集合供同块所有候选复用；去掉 format! 临时串。

**P134｜markdown_offset_map 在每次光标移动/选区变化时重建两张全量 usize 表**（inline/tree.rs:374-527；类别：性能｜低）
现象：该函数为每个可见字节与每个 markdown 字节各建一张表（每项 8 字节×2）。光标移动、选区拖拽的每次调用都从零重建；一段 2KB 的段落约 32KB 临时分配外加 O(n) 填表，拖选长段落时每帧都在分配。注释承认该路径曾实测 779ms 并为序列化加了免映射快路径，但映射路径本身没有缓存。
原因：映射完全由 fragments 决定，块标题在纯光标移动期间不变；没有「树版本号→缓存映射」的记忆化，也没有惰性只算查询区间的实现。
改进：给 InlineTextTree 挂惰性缓存的映射表（代数失效），光标路径复用；或提供按需单偏移换算（沿片段推进）。搜索、选区、粘贴多条路径自动受益。

**P135｜粗体/斜体片段末尾键入延续样式，code/删除线/高亮却不延续——边界属性策略不对称**（inline/tree.rs:905-931；类别：体验｜低，意图待验证）
现象：光标放在 `**粗体**` 末尾直接打字，新输入继续是粗体；光标放在行内代码/删除线/高亮的末尾打字却是普通文本。用户在强调句尾补一个句号会得到粗体句号，需要手动切回——Typora 等主流编辑器在闭合记号之后插入的都是普通文本；而 code 族「不延续」说明该处明确做过选择，粗斜体延续更像遗漏而非策略。
原因：attributes_for_insertion_at 对「片段末尾且是最后一个片段」返回片段样式，仅对 code/strikethrough/highlight 特判返回默认；underline、script、bold、italic 都落入延续分支。
改进：若意图是粗斜体在边缘延续，在文档与测试中固化并把 underline/script 纳入同一口径说明；若意图对齐主流编辑器，把特判扩成非默认样式一律返回默认。

**P136｜`<u>`/`<strong>` 容器标签递归解析无深度上限，深嵌套粘贴可栈溢出**（inline/parse.rs:1174-1213/744-753；类别：缺陷｜低，需构造性输入）
现象：容器标签在容器体内重新进入递归解析，体内同名开标签又计深并再次递归。输入 `<u><u><u>…</u>…</u></u>` 嵌套数万层时递归深度与嵌套数成正比，栈溢出在 release 下表现为进程 abort——粘贴一段恶意/超长嵌套 HTML 即可触发。脚注/链接标签的重解析同样各自开辟新递归，放大深度。
原因：整个递归下降没有深度计数参数或显式栈化；标签过滤只限标签名不限结构深度。
改进：parse_until 增加深度参数（超 64 层直接按字面输出），容器/链接递归入口透传；或改显式工作栈。补深嵌套输入冒烟测试。

## 十、表格/链接/图片/HTML/高亮组件

**P137｜行内链接与图片目标不支持单引号/圆括号标题，标题整体混入目标**（markdown/link.rs:327-347；image.rs:721-741；类别：缺陷｜中）
现象：`[a](/url '标题')` 的目标被解析成整串，点击时被当相对路径去开本地文件；独立图片 `![x](i.png (title))` 的 src 变成 `i.png (title)`，图片必然加载失败。CommonMark 明确支持 `'...'` 与 `(...)` 两种标题写法。
原因：目标解析只有双引号一支做标题剥离，单引号与圆括号标题不识别，直接把整串当目标；而续行识别却认 `'` 与 `(` 包裹的写法——写法被承认，目标解析端缺对应分支，两端判据不齐。
改进：补齐 `'...'` 与 `(...)` 标题分支（闭合判定+开口查找对应版本），判据与续行识别共用同一张表；补往返用例。

**P138｜pipeless 表格把后续非表格行整段吞成表格行，与 GFM 断表规则相悖**（markdown/table.rs:878-892/836-849；类别：缺陷｜中）
现象：无外管道表格下方不空行直接写标题或普通段落时，`# 标题` 被并入表格区域并被 resize 补空格成一行表格数据，标题语义丢失；GFM 规定表格在「另一个块级元素开始处」断开。
原因：区域收集只以空行结束，没有任何块级元素边界判断；区域解析对每行无条件切格——无管道行返回单元素后被 resize 成整行格子而不是终止表格。
改进：扩展循环里对 ATX 标题、分隔线、列表、引用等块级起点提前 break；区域解析对少于 2 列且非空的非候选行返回 None 触发回退。

**P139｜引用定义扫描器在 link.rs 与 image.rs 整套复制约 200 行，违反单一判据不变量**（markdown/link.rs:180-406；image.rs:574-806；类别：缺陷（架构）｜中）
现象：围栏信息类型、容器前缀剥离、开/闭围栏判定、HTML 块起点识别、标题续行识别、字符查找及整个定义扫描主循环在两个文件里逐字重复约 200 行——仓库当前最大的判据抄写面。任何一侧修复（围栏判据、标题分支）都极易漏改另一侧，造成链接与图片对同一文档的引用表不一致。
原因：链接定义与图片定义扫描逻辑本就同构（normalize_reference_label、is_escaped、angle_bracket_destination 已是共享单点），但扫描脚手架各自成文。
改进：把扫描脚手架抽到共享模块（如 markdown/references.rs），两个入口只保留各自的定义行解析函数，以泛型/回调注入 destination 解析。

**P140｜引用扫描的围栏判据与 CommonMark 不一致（闭合须等长、不容缩进）**（markdown/link.rs:246-271；image.rs:640-665；类别：缺陷｜中）
现象：用 ```md 开栏、更长反引号串闭栏时，CommonMark 认为代码块已结束、其后是有效定义；本扫描器要求 run_len 相等，认为仍在代码内，定义被丢弃，引用链接降级为字面文本。≤3 空格缩进的合法围栏两函数都只 trim_end 不 trim_start，开/闭均不识别。
原因：闭合判定用相等而非「至少与开栏等长且剩余仅空白」；开栏判定遇前导空格直接返回 None。主解析器若按 CommonMark 处理围栏，两套范围就会漂移。
改进：闭合改为 ≥ 开栏长度且剩余仅空白；开/闭都允许 ≤3 空格前导缩进；判据与真正的代码块解析共用（随 P139 合并），补用例。

**P141｜表格/单元格图片的代码段切分判据与行内解析器不一致（闭合反引号前的反斜杠）**（markdown/table.rs:658-683；image.rs:321-348；类别：缺陷｜低）
现象：CommonMark 规定代码段内反斜杠转义无效，`` `a\`|b` `` 的代码段在第二个反引号处闭合、其后的 `|` 是格分隔符；本仓库表格切分扫描在搜索闭合时跳过转义字符，把整段当成未闭合代码，`|` 不切分，列边界与 GFM 不一致，保存后表格形状被改写。
原因：表格切分与单元格图片扫描的闭合搜索做转义判断；行内解析器判定闭合只看反引号串长度相等。同一「代码段闭合」语法存在两套判据。
改进：闭合搜索改为与行内解析器同判据（闭合候选不做转义判断；只有开栏反引号需要排除被转义者），或三处统一收敛到共享函数。

**P142｜parse_html_attrs 无引号属性值遇 `/` 截断**（markdown/html/parse.rs:363-372；类别：缺陷｜低）
现象：行内 HTML `<a href=x/y>` 的 href 被解析成 `x`，`/y` 里的 `y` 变成独立无值属性；`<img src=a/b.png>` 同理 src 变 `a`，行内图片语义全错。HTML 规范允许 unquoted 属性值包含 `/`（仅禁空白、引号、=、<、>、反引号）。
原因：unquoted 值扫描把 `/` 当终止符——这是为 `<br/>` 式自闭合尾斜杠准备的，但自闭合斜杠只在标签尾生效，值中间的 `/` 是合法字符；块级 HTML 用的 html5ever 会取完整值，两条 HTML 属性解析路径分叉。
改进：unquoted 值不把 `/` 当终止符，自闭合判定维持 trim_end().ends_with('/')；补用例；考虑让行内属性解析改走 html5ever 或共享。

**P143｜引用定义「标题续行」无缩进上限检查，可吞掉缩进代码块行**（markdown/link.rs:162-170/377-391；image.rs 同文；类别：缺陷｜低）
现象：`[a]: /url` 的下一行若是缩进 ≥4 空格的 `"quoted text"`，CommonMark 里那是缩进代码块，与定义无关；本实现对任意缩进量都返回 true（只查非零），定义行把它当标题吞掉，该行在渲染中消失。tab 缩进同样中招。叠加 P137（仅双引号标题可解析），单引号续行被吞后还会把 `'title'` 拼进目标。
原因：续行识别没有「续行缩进不得达到代码块阈值」约束；解析拿到 true 后无条件把下一行 trim 后拼进 target 并跳过该行。
改进：续行识别加缩进上限（≤3 空格或等宽 tab 折算），超过即判非续行；与 P137 一起补齐后再放开续行。

**P144｜code_highlight 多个 build_*_config 缺 feature 门，core-only 构建组合无法编译**（markdown/code_highlight.rs:388-561；类别：缺陷｜中，待验证）
现象：`cargo build --no-default-features --features code-highlight-core`（Cargo.toml 定义该 feature 可单独启用）时，17 个语言配置函数体引用的 optional 依赖未被启用，应报 unresolved crate 编译错误。作者明显知道要加门（rust/yaml/toml 都加了 cfg），中间 17 个漏了。
原因：这些函数定义不受任何 #[cfg] 保护，仅调用点在 registry 构建内被 cfg 包住；Rust 编译器仍会检查未调用函数体。
改进：为这批函数补与调用点一致的 #[cfg]；或在 CI 加 --no-default-features 两个组合的编译检查验证。

**P145｜表格列宽测量对全表每格做文本 shape，任一格编辑即全表重算**（markdown/table.rs:279-299/420-517；类别：性能｜中）
现象：上限规模 100 行×30 列的表格里，在任一单元格敲一个字符，列宽备忘以整个 TableData 为键立即失效，下一帧对全部 3000 个格子各调一次 shape+间距补偿，即使被编辑格之外 2999 个格子内容与宽度毫无变化；大表格打字延迟明显。
原因：测量无条件遍历所有行列，每格走完整 shape 管线；memo 键是整表值比较，无法复用未变动列的测量结果。
改进：首选宽度按列缓存（键含该列全部 cell 内容+字体参数），编辑只重测受影响列；或 memo 里做逐列指纹。可对视口外行延后测量。

**P146｜表格单元格图片加载失败的降级表现两套口径**（block/render/content.rs:130-141；inline_visuals.rs:646-661；类别：体验｜低）
现象：同一个解析不出来源的单元格图片，在标题整格渲染路径里失败时回退渲染整段 markdown 原文（含 `!`、括号），在行内分词路径里只回退渲染 alt 文本——用户在同一张表的不同渲染时刻会看到两种不同的失败表现。
原因：两处各自实现了失败分支，没有共享「图片不可用时的文本表示」判据。
改进：统一为一种降级（建议显示 alt 文本，与 CommonMark 图片语义一致），抽小函数共用，两条路径各补失败回退断言。

**P147｜data: URI 图片被安全判据误杀；协议相对 `//host/x` 落地成本地路径**（markdown/link.rs:7-18；image.rs:136-145；类别：体验｜低）
现象：`<img src="data:image/png;base64,…">` 因 data: 不在外链 scheme 白名单整块降级为 raw 源码文本无法渲染；markdown 写法的 data URI 被判成相对路径去文件系统找，必然失败只留占位。`//example.com/a.png` 协议相对写法被 is_absolute（unix 上 // 开头为真）判成本地绝对路径，同样失败。
原因：is_safe_export_url 对 HTML 属性按「scheme 必须在 5 个外链 scheme 内」一刀切，没区分 href（禁 data:text/html 合理）与 img src（data:image/ 无脚本风险）；远端判定只认解析出 http/https 的字符串。
改进：img src 白名单增加 data:image/（导出内联时同样放行）；对 // 开头且能解析出 host 的按 https 协议相对处理或给明确失败占位。

**P148｜frontmatter 闭合判定三处判据并存，非 key 行被静默并入上一个 key**（markdown/frontmatter.rs:10-53；export/html.rs:118-136；import.rs:257-266；类别：缺陷｜低）
现象：「--- 围栏对怎么认」存在三份实现。常规输入三者一致；但用户在 front matter 内手敲一行 `---`、`...`、`# 注释` 或任何无冒号顶层行时，行解析兜底把它整行当值 push 进上一个 key 的值列表，属性卡出现由分隔符/注释文本组成的假值行（导出 HTML 表格同步显示）。
原因：兜底分支对一切非 key 行照单全收，没有区分 YAML 文档标记（---/...）、注释（#）与真正的续行/列表项；闭合剥除侧依赖调用方保证无尾随换行，判据没有单点。
改进：兜底分支跳过文档标记与注释行；围栏闭合判定收敛为一个共享函数（导入、导出、body 剥除三处同源）。

**P149｜`<>` 空目标与 `<a<b>` 目标语义与 CommonMark 相悖**（markdown/image.rs:781-799；link.rs:435-436 测试固化；类别：缺陷｜低）
现象：`[a](<>)` 在 CommonMark 是合法的空目标链接；本实现对 `<>` 原样返回，目标变成字面字符串 `<>`，点击/加载按文件名处理。<a<b>（内部未转义 <）CommonMark 拒绝尖括号形式回退为非法目标（整段不成链接），本实现保留尖括号当文件名，产生一个永远打不开的假链接。
原因：注释自述「<> 是空目标，CommonMark 不认」——与 CommonMark 0.30/0.31 相悖（<> 显式列为合法 destination）。
改进：`<>` 返回空串让上层按空目标处理；含未转义 `<` 的目标在 parse 层判整体失败回退字面文本；同步修改固化的测试预期。

**P150｜行内链接 URL 扫描不感知引号，标题内括号会提前截断链接**（markdown/inline/links.rs:42-77；类别：缺陷｜低）
现象：`[a](x "ti)tle")` 的 URL 扫描在标题引号内的 `)` 处提前断链：解析出目标 `x "ti`，尾部 `tle")` 成为字面文本。CommonMark 允许带引号标题内出现不平衡括号。反例因括号恰好配对而侥幸正确。
原因：扫描只维护括号深度计数，进入引号标题后既不跳过引号区间也不暂停括号计数；与 P137 缺标题解析分支是同一根源的另一半。
改进：URL 扫描遇到未转义引号且其后存在同类闭合引号时进入标题跳过状态（或先探测「空白+引号…引号+)」尾形态再定终点），与 P137 一并实现互为校验。

## 十一、表格编辑与编辑入口 UI

**P151｜表格插入对话框是「假模态」——无焦点陷阱，按键直接打进遮罩后面的正文**（context_menu/render.rs:569-585；render/paint.rs:1186-1233；类别：缺陷｜高）
现象：对话框打开后全屏遮罩只挡鼠标不挡键盘。用户按 Enter 期待「确认」，实际是给遮罩后仍持焦点的段落/格子插了一个换行；开了 vim 时普通模式按键（如 `dd`）会直接删掉看不见的正文行；输入的字母进的是背后文档。Tab 也不能在「+/-」步进器间移动，对话框完全没有键盘可用性，只能鼠标点。
原因：overlay 只是 div+occlude+mouse_down，occlude 只影响鼠标命中测试；打开入口全程不碰 window.focus；按键拦截器只在 active_focus_trap 非空时让位，而这里没有 trap；它还画在补全/公式浮层之下。
改进：对齐 modal.rs 真模态的做法：给对话框建 FocusHandle，打开时保存 restore_focus 并聚焦对话框，或 intercept_keystrokes 级别拦下非 Esc/Enter 按键；Enter 绑确认、Esc 绑关闭、Tab 在步进器间轮转；调整叠放顺序。

**P152｜插入表格走 mark_dirty 整篇重投影，把全文的原文写法一起洗掉**（context_menu.rs:594-604；类别：缺陷｜中）
现象：从右键菜单「插入→表格」插一张 2×2 的表，整篇文档被块树重新序列化一遍：别处手写的 `__强调__` 变 `**强调**`、Setext 标题变 ATX、其它表格的列宽填充被重排、CRLF/末行换行被规范化。同一菜单里「插入代码块」等走区间写回保真，唯独插入表格永远整篇落笔。
原因：insert_table_from_dialog 在结构插入后直接 mark_dirty；窗口状态注释自证该路径代价「未编辑的块也被重新序列化一次（表格列宽填充、__ 强调这些写法就此改写）」。插入点信息其实齐备（After(entity_id) 有锚点根块，find_block_location+source_span 可算插入字节位）。
改进：仿照插入代码块的区间写回：按锚点根块 span 写入新表格字节并平移后续根块，成功后走 mark_dirty_written_back，算不出区间才退 mark_dirty。

**P153｜删最后一列会把每行的收尾管道一起剪掉，源码写法被破坏**（table_edit.rs:411-431；类别：缺陷｜中）
现象：对 `| a | b |` 删最后一列，结果行变成 `| a `（带尾空格）——收尾 `|` 全没了。GFM 仍能解析（尾管道可省），但用户原文的写法被静默改变；且此后这张表所有列级操作的 wrapped 判定恒为假，全部静默退化成整根块重写（撞上 P152/P154 的洗写法问题）。
原因：剪点定义为「这一格连同它右边那根竖线」，最后一列的右边那根竖线就是收尾管道，剪到行尾；插入列的对称路径却特意在末管道之前插、保住了它。
改进：删最后一列时改剪「格内容+左边那根竖线」（行尾保留收尾管道）；补「删末列后每行仍以 | 收尾」的往返用例。

**P154｜行尾带空格的表格行让所有列级操作静默退化成整根块重写**（table_edit.rs:340/414/529；类别：缺陷｜中）
现象：表格某一行末尾多敲了两个空格，之后对这张表的插列、删列、左右移列、两列互换全部失效——不是报错也不是部分成功，而是每次都退到整根根块重写：顶层表洗掉整根块内用户手写的列宽与 `__` 写法；表在容器里时容器内其它内容一并被重新序列化。行级操作（插行/删行）没有这个判定，同一张表行能用、列不能用，行为不一致且用户无从察觉。
原因：三处列操作共用的 wrapped 判定要求最后一根管道恰在行尾字节上，尾随空格使判定为假即整体 return false。
改进：判定前先 trim_end 并在修剪后的文本上量管道（或补偿尾随空白宽度）；补「行尾空格下列操作仍按行落笔」用例。

**P155｜状态栏「X / Y 词」两个数字用两把尺：整篇按源码数、选区按显示文本数**（status_bar.rs:98/162；类别：缺陷｜中）
现象：选中全文时状态栏同屏显示「选区词数/全篇词数」，但两个数口径不同：整篇数的是 Markdown 源码——表格行 `| a | b |` 会被数出 5 个「词」（分隔行同样），`**bold**`、`[text](url)` 的记号也混进词数；选区侧明确返回显示文本。结果含表格或强调记号的文档里，选区数与全篇数对不上，总量偏大且随记号数量漂移。
原因：两个不同口径的输入喂给同一个计数函数，而计数函数只按空白切拉丁词、逐字数 CJK，对记号无任何过滤。
改进：统一口径：整篇也按可见文本累计（可在既有缓存里对各根块 display_text 分词求和，仍走 250ms 静默窗口），或把选区也换成源码口径；至少把表格分隔行与管道记号从词数里排除，加含表格文档的口径回归用例。

**P156｜document_submenu_rows 产品路径用 expect 打开段落菜单**（context_menu/document_menu.rs:461-469；类别：缺陷（规约）｜低）
现象：渲染「段落」二级菜单的每一行时调用 `command.as_block_target().expect("这一档全是段落转换")`。这是产品路径（右键菜单每次渲染都会执行），目前数组里确实全是段落转换所以不会炸，但「数组内容全为段落命令」这个约定只写在字符串里：今后任何人往那两个数组里加一条非段落命令，一打开右键菜单就 panic，且 CI 没有任何审计测试拦这种模式。
原因：as_block_target 本来就返回 Option 正是为「格式/编辑行没有段落目标」设计的，这里用 expect 绕开了类型给的保护。
改进：改成 if let Some 否则不可用行，或该分支直接构造 BlockKindTarget 字面量数组；顺手在本目录加一条「产品路径无 unwrap/expect」的源码审计测试。

**P157｜模态按钮按「默认排最后」重排，删除确认的破坏性按钮成了右下主按钮**（modal.rs:372-375；workspace/prompts.rs:40-47；类别：体验｜中）
现象：ModalSpec.buttons 的数组顺序与屏显顺序会分叉：排序把默认按钮稳定挪到队尾（右下）。工作区「删除确认」spec 是 [确认, 取消]、默认下标 0，屏显变成 [取消, 确认]——「确认删除」成了右下高亮主按钮且是 Enter 默认，一次手滑回车就删了文件/目录。多数模态恰好把默认写在最后所以从没暴露。
原因：布尔键排序使默认项移到末尾；回车路径随之触发数组下标 0 的删除分支；is_destructive 只对窗口关闭/拖放文件两种模态生效，General 模态没有破坏性标记。
改进：ModalSpec 增加 destructive 标记，破坏性按钮不做默认、渲染 danger 样式且排非默认侧；或直接修订删除确认的 spec（buttons: [取消, 确认]），并在 modal.rs 注释写明屏显顺序规则。

**P158｜仅剩表头的表上，单元格菜单「删除行」静默无效，与轴菜单同路径不同结果**（context_menu/render.rs:374-380；table_edit.rs:1293-1297；类别：体验｜低）
现象：先删掉某表全部数据行（允许留下仅表头的表），再右击表头单元格：菜单里「删除行」是亮着的，点下去什么都不发生、菜单收起、无任何提示。而右击第 0 行的行柄打开轴菜单做同样的事却会走「删除整张表」。同一语义动作在两个菜单里一个静默无效、一个删表。
原因：cell 菜单该行的 enabled 只看 writable，不看表头之下还有没有数据行；点击后空表守卫直接 return。轴菜单路径则显式处理了 rows==0 时删整表。
改进：on_delete_table_cell_row 对表头格复用轴菜单判定；或构造菜单项时按 rows.len() 决定行为/文案，保证两菜单同一动作同一结果。

**P159｜单元格菜单插入行/列后，焦点落点与右击位置脱节**（table_edit.rs:905-913/964-971；类别：体验｜低）
现象：在第 3 行第 2 列的格里右击「在下方插入一行」，焦点跳到新行的第 0 列；右击同一格「在左侧插入一列」，焦点跳到表头的新列格。用户的心智位置（右击的那格）两次都被丢掉，连续操作会打错地方——尤其插列后光标落在表头，第一下输入污染的是表头而不是正文格。
原因：插行只按行取首格，插列只按列取表头格；cell 菜单入口手里明明有右击格的 position，却只把 column 传下去不传行。
改进：给两个 insert 函数加可选「锚点位置」参数：插行后聚焦 (新行, anchor.column)，插列后聚焦 (anchor.row, 新列)；轴柄入口保持现有落点。

**P160｜右键菜单每次渲染重建行数据、逐行实测文字宽度、每帧读两次剪贴板**（context_menu/render.rs:84-97/731；document_menu.rs:985-1003；类别：性能｜低）
现象：正文菜单开着期间的每一个渲染帧都重跑：重建全部菜单行、对面板每行 label/快捷键/箭头各做一次 shape、子菜单展开时再来一整遍；顶部图标条每颗调可用性判定，其中粘贴两颗各触发一次剪贴板读取。背后聚焦块的光标每秒闪烁两次就 notify 两次，悬停高亮变化更是每动一次鼠标一帧全部重算。
原因：几何与可用性都没有按「菜单打开期间不变」缓存——几何每帧现算，shape 逐串分配。
改进：在菜单状态里缓存（rows, panel_geometry, submenu_geometry），打开与语言/主题变化时失效；剪贴板可用性在打开时读一次存进状态。

**P161｜选中工具栏每帧深拷贝整个 I18nStrings（约 567 个 String）**（selection_toolbar.rs:273；context_menu/render.rs:76/492；类别：性能｜低）
现象：只要屏上有一段选区，编辑器每个渲染帧都执行 `strings().clone()`。I18nStrings 是含约 567 个 String 字段的普通 struct，这次 clone 是几百次堆分配+拷贝；拖动选择期间鼠标每动一帧都来一遍，工具栏菜单展开的帧还要再叠加两处同款 clone。约零点几毫秒/帧，纯属浪费。
原因：strings() 返回引用，下游签名本可借用作罢，却先 clone 成 owned 再借出去。
改进：改为借引用（注意 listener 闭包捕获字段级 String 的局部拷贝）；或 I18nStrings 换 Arc 存储、strings() 返回 Arc，clone 变一次原子计数。

**P162｜表格结构操作前从 record 同步模型，IME 组合中的输入可能丢**（table_edit.rs:881/938/1049 等 8 处 + rebuild_table_runtimes；类别：缺陷｜中，待验证）
现象：用户正在某格里打 IME 组合串（尚未提交、该格未发出 Changed 事件），此时右击格子菜单点「在下方插入一行」：同步函数从各格 record 读模型，读到的是不含组合串的旧值；随后表格运行时重建把全表格子实体换新，旧实体（连同其未提交输入）被抛弃。轻则插入行的模型里少了那串字，重则组合串提交时旧实体的写回路径已失效。常规路径下点击菜单会先让格子失焦、blur 提交大概率兜住，但需要实测确认。
原因：同步的唯一来源是 cell.record.title，没有「块内缓冲区里还有未落 record 的文本吗」的兜底；rebuild 又是全量换实体。
改进：各结构入口 sync 前显式调「冲刷活动格」（对活动格强制提交输入并等 Changed 同步），或让 sync 读取块的实时文本缓冲而非 record 快照；补「组合中插入行不丢字」的 IME 用例。

**P163｜轴菜单顶部「表头行」开关是全局偏好，却长在单张表的右键菜单里**（context_menu/render.rs:292-299/14-26；类别：体验｜低）
现象：右击某一张表的行柄第 0 行，菜单第一项是带勾选态的「表头行」，看起来是「这张表显不显示表头」的开关；点它改的却是全局偏好，所有文档、所有表格的表头渲染一起消失/出现。用户想临时折叠一张大表的表头时，会意外改掉全局阅读体验，且菜单里没有任何「全局」字样。
原因：checked 读全局配置，toggle 直接写回全局再全窗重绘。
改进：二选一：给 TableData/BlockRecord 加每表 header_visible 字段，菜单只改本表；或保留全局偏好但文案改成「显示表格表头（全局）」并在 tooltip 说明影响范围。

**P164｜状态栏代码块行数硬编码中文「行」，英文界面漏翻**（status_bar.rs:151-158；类别：缺陷｜低）
现象：光标落在代码块里时状态栏显示「12 行」；切换到英文语言后其它条目都走 i18n 表，唯独这个「行」字写死在 format! 里，英文用户看到中英混排。
原因：format!("{lines} 行") 没有从 strings 取词；同文件其它条目均用 strings 字段，i18n 机制现成，只是这一处漏接。
改进：i18n 表增加代码行数后缀字段中英文各补一条，分支改用模板；顺带 grep format! 里裸写 CJK 的同类点统一收口。

**P165｜正文自绘菜单与 PopupMenu 系菜单键盘行为不一致：前者不接管焦点**（context_menu/render.rs:145-193 对比 201-481；类别：体验｜低）
现象：表格轴菜单、单元格菜单、图片菜单基于 PopupMenu，打开即抢焦点、方向键/回车/Esc 都由它处理；正文右键菜单（编辑/格式/段落/插入/视图）是自绘面板，不碰焦点。前者开着时键盘完全服务于菜单，后者开着时按字母、⌘Z、方向键全部继续作用于背后正文——两套右键菜单行为割裂，且正文菜单开着时撤销/输入会改用户看不见（被遮罩挡着）的内容。
原因：Document 分支的 overlay 只有 occlude 与鼠标事件，没有 focus_handle；Esc 走全局关闭才收得掉，其余按键无拦截。
改进：最小改法给正文菜单也建 FocusHandle，打开时聚焦、关闭时归还，至少让打字类按键不落进正文；完整做法是迁到 PopupMenu 或补方向键导航。

## 十二、撤销、保存与缓冲区

**P166｜撤销/重做每次整篇重建块树，大文档一次 ⌘Z 冻结秒级**（history.rs:888-958/687-727；类别：性能｜高）
现象：撤销/重做都走 rebuild_document_from_buffer：渲染态整篇重解析建根块 + 重建表格运行时（其内部还要刷引用注册表，作者自测 10 MiB 单价 766ms）；再加捕获行布局对每个可见块 format 一份含块源码全文的签名字符串（10 万块 = 10 万次堆分配+全文切片）。撤销在大文档上是主线程秒级冻结。
原因：撤销重放只动了已知区间，但树重建却按「缓冲区全量」来；签名恢复也按全量可见块做。
改进：HistoryEntry 的 edits 自带区间，撤销本可只对受影响区间做区域重解析（区间外块实体原样保留）；签名恢复只对重建的块做。

**P167｜IME 组合收尾为判 noop 克隆整个缓冲区**（history.rs:342-359/143-155；类别：性能｜中）
现象：每次中文输入法提交（CompositionChanged 且无 marked text）都做 noop 判定 → `buffer.clone()`（10 MiB 文档 = 一次 10 MB 堆拷贝）+ 反序重放 + 全文比较。中文用户在大文档上每次提交候选词都付这笔。
原因：noop 判定以「整份缓冲区副本上真重放一遍」为正确性代价，没有按组大小分档。
改进：组内只有一条且区间很小时在切片上判；或记录「本组各 edit 后的文本哈希」做快速否决，仅哈希相等才做完整重放。

**P168｜finalize_pending_undo_capture 的 6 处 expect("checked above")**（history.rs:388-418；类别：约定｜低）
现象：依赖上方 is_some_and 守卫写 expect；仓库约定要求「为什么不可能失败」写清——"checked above" 算最简理由但脆弱（后人重排代码即失守），且与「产品路径禁 expect」的成文不变量字面冲突。
原因：合并分支连续六次取 last_mut，作者用 expect 表达「守卫已保证」。
改进：把 `self.undo_history.last_mut()` 的结果在守卫处一次取出存进局部变量，后续分支用 if let Some(entry) 表达，既消除 expect 又少六次重复调用。

**P169｜恢复目录里一个文件读失败，全部未保存草稿的恢复整体放弃**（config/recovery.rs:51/59-64；main.rs:123-129；类别：缺陷｜高）
现象：启动恢复流程读 recovery 目录时，只要有一个快照文件 IO 读失败（权限被改、Windows 上被杀毒/同步盘独占、坏扇区），整个读取返回 Err，恢复流程直接 return，本次启动所有未保存文档一个都不恢复。无效 JSON 反而有容忍（跳过），唯独 IO 错误一票否决，且失败文件不删除，下次启动同样失败，形成永久性恢复不可用。
原因：两个 `?` 把单条目错误提升为整目录错误；消费端 Err 分支只写 stderr 不降级、不部分恢复。与同文件里 JSON 解析失败仅跳过的处理策略自相矛盾。
改进：单条目失败记日志后 continue，聚合计数；若全部失败再报错。把反复读取失败的快照改名隔离（如 .bad 后缀），避免永久卡死恢复链路。

**P170｜进程被杀时保存临时文件永久残留在用户文档目录，无启动清扫**（persistence.rs:51-111/135-144；类别：缺陷｜中）
现象：保存临时文件建在目标文件同目录（即用户文档目录/工作区内）。Drop 只覆盖正常 drop（取消、失败、panic 均能清理），但进程被强杀、断电、崩溃在 create 之后 rename 之前时，含全文内容的临时文件永远留在用户目录里。全仓确认启动路径没有任何清扫；残留文件会被同步盘上传、出现在文件管理器里，还会被 watcher 当作变更事件（见 P175）。
原因：prepare 的 create→write→sync→rename 序列跨多次系统调用，窗口内进程死亡无人兜底；Drop 是语言层机制，进程消失即失效。
改进：启动时对配置目录做一次临时文件模式清扫；对工作区内的残留，在启动重扫时对匹配该模式且 mtime 早于本次启动的文件静默清理（或至少在文件树中隐藏）。

**P171｜rename 之后不 fsync 目录，断电时「已保存」可能回滚为旧内容**（persistence.rs:115-117；config/mod.rs:70-78；类别：缺陷｜中）
现象：三处原子写都是「临时文件 sync_all → rename」就结束，rename 之后不再 fsync 父目录。文件内容已落稳，但目录项变更本身可能未持久化：断电重启后文件可能回到旧版内容，而 UI 已经清了脏标记、恢复快照也在成功路径被删——用户「保存成功过」的那一版凭空消失且无任何副本。
原因：commit() 是单行 rename，前后无目录句柄 sync；POSIX 语义下 rename 原子性只保证「旧或新二选一」，不保证「新」一定落盘。
改进：unix 下 rename 成功后对父目录 sync_all（macOS 用 fcntl F_FULLFSYNC）；可只在手动保存/关窗保存这条强持久化路径做目录 fsync，autosave 保持现状并注释取舍。

**P172｜一篇文件的外部修改冲突会冻结整个窗口所有文档的自动保存与恢复快照**（persistence.rs:282-289；workspace/documents.rs:471-473；类别：缺陷｜中）
现象：外部修改冲突是窗口级单槽位，autosave 一旦撞上冲突就记下它；此后只要冲突未解除（用户点「继续编辑」或 Esc 关掉冲突框、或始终不处理），schedule_autosave 直接 return——包括恢复快照在内的所有写入全部停摆。此时用户在另一篇继续打字，该文档既不落盘也不写恢复快照，崩溃即丢全部未保存内容。快照本是崩溃保护的最后防线。
原因：冲突判据与脏检查同处一个早退分支，冲突被放大成全窗口暂停；冲突解除只依赖三个显式入口，用户不操作就一直挂着。
改进：把冲突判据从「窗口级是否存在」收紧到「本篇是否冲突」：快照照常写（快照不覆盖磁盘、无覆盖风险），只跳过冲突文件的磁盘提交；冲突文件持续被编辑时给出更强的提醒而不是无限静默冻结。

**P173｜文件监听器建立失败只有 eprintln，外部修改检测整体静默失效**（watcher.rs:46-56；类别：缺陷｜低）
现象：watcher 创建失败或 watch(root, Recursive) 失败时，函数 eprintln 后直接 return，外部修改完全不会被推送到界面：干净文档永不重载，用户看到的一直是打开时的旧内容，也没有「文件已在磁盘上被修改」的任何提示。唯一兜底是保存时的版本闸门。文件描述符耗尽、网络盘不支持递归监听等真实场景都会走到。
原因：两个错误分支都只写 stderr 就结束，没有走应用内上报通道，违反「错误要送应用内模态/可见位置」。
改进：失败时向侧栏红字或一次性模态报告「无法监听文件变更，外部修改将不会自动刷新」（给出手动刷新入口），并记录失败状态避免每次切根重复静默尝试。

**P174｜监听溢出（need_rescan）只刷新文件树，不复查已打开文档，外部修改漏报**（watcher.rs:23-27/66-72；session_watcher.rs:267-275；类别：缺陷｜中）
现象：watcher 溢出时映射为 Rescan，处理只做侧栏树刷新。溢出窗口内丢失的变更事件若涉及打开中的干净文档，该文档内容停留在旧版且无任何提示——树是新的、正文是旧的，用户毫无感知地基于陈旧内容继续阅读甚至编辑，直到某次保存被版本闸拦下弹冲突才暴露。
原因：Rescan 分支只调树刷新；树刷新只重扫目录列表，不遍历打开文档做磁盘版本比对。
改进：Rescan 时对每个打开文档补一次后台读盘比对（复用既有重载判重逻辑，挪后台线程），脏文档跳过、干净文档版本不一致才走重载。

**P175｜每条监听事件都在主线程同步全量读盘+解码重载，无事件合并**（watcher.rs:63-77；tabs.rs:295；类别：性能｜中）
现象：监听泵对每条事件在主线程调重载，后者同步执行全文件 read+编码探测+GB18030/UTF-16 解码+规范化+全文哈希。外部程序分片写大文件（重定向、curl、同步盘增量上传）会连发 N 次事件，即 N 次主线程全量读盘；树刷新路径专门做了 250ms 合并，文档重载却没有等价机制。每次读失败还会排一个 400ms 复查定时器，事件风暴时定时器串成长链。
原因：逐条转发无节流；IO 在 cx.update 闭包（主线程）内同步执行。大文档一次解码可达几十毫秒，连续事件直接卡输入。
改进：给文档重载加与树刷新相同的 250ms 合并（按路径收集 pending，一轮一读）；读盘挪后台 executor；复查定时器按路径去重。

**P176｜手动保存整条链路同步跑在主线程：三次全量读盘校验+两次 fsync**（persistence.rs:817-848；类别：性能｜中）
现象：Cmd+S 对已有文件保存时，主线程依次执行：版本校验（第 1 次全量读+解码+哈希）→ prepare（capture_baseline 可能第 2 次全量读并写历史；正文阻塞式 fsync）→ commit_checked 内再校验（第 3 次全量读+哈希）→ rename。多 MB 文档、UTF-16/GB18030 解码、Windows Defender 实时扫描叠加时，一次保存可卡 UI 数百毫秒。autosave 路径已经把同样动作放进后台线程，手动路径没有复用。切工作区时逐脏标签同步写盘的路径同理。
原因：手动保存是纯同步函数，prepare/commit/verify 均在 UI 线程执行。
改进：复用 autosave 的模板：主线程取字节与身份号后转后台做 prepare+verify，回到主线程只做修订号校对与 rename；baseline 读取挪后台。

**P177｜debounce 到点后发现 IME 组合中会静默取消本轮 autosave 且不重排**（persistence.rs:300-303/344-347；类别：缺陷｜低）
现象：修订号不匹配的两条失败路径都会重排下一轮，唯独「到点发现 marked text」这条返回 None 后只把任务清空就结束，不重排。若用户在输入法组合态停手离开（组合一直挂着），本次 autosave 取消后没有任何后续触发，恢复快照与文件写全部停更，直到下一次编辑事件才恢复。
原因：返回 None 的两种成因（marked text / 无脏文档）没有区分，前者本应重试。
改进：快照闭包区分返回值：marked text 时返回「稍后重试」标记，主线程再排一轮（可带更短延迟）。

**P178｜autosave 后台一轮对同一文件最多三次全量读+哈希，外加整批文档深拷贝**（persistence.rs:349/360-381；类别：性能｜低）
现象：autosave 后台循环对每个待写文档：verify（读+解码+哈希）→ prepare（baseline 可能再读一遍）→ 又一次 verify——提交阶段还要再 verify 一次。同轮三次全量读盘+哈希，大文档/多脏标签时 IO 放大明显。另把整批文档的 markdown+bytes 深拷贝一份传进后台闭包，多 MB 文档时内存翻倍且每次防抖周期都发生。
原因：第二次 verify 的结果被丢弃（只取 ?），且提交校验本来就要再做一次，同样的检查做了两遍。
改进：删去中间那次 verify（由提交校验统一兜底）；baseline 与第一次 verify 的 raw 共用；文档批次改 Arc 或按值 move 免克隆。

**P179｜关窗保存失败时错误双路上报，且外部修改不走「重载/另存为」冲突入口**（persistence.rs:596-605/671-679；类别：体验｜低）
现象：关窗保存的后台写失败：主线程回调里先报侧栏红字，返回错误详情后闭包外又弹模态——红字+模态双路。更关键的是 external_change 分支只给一块纯展示的消息模态，不像 autosave 冲突那样走带「重载/另存为」解除入口的冲突记录，与该错误类型的文档注释自相矛盾。用户只能靠随后重新弹出的未保存对话框绕行。
原因：该分支无条件报侧栏红字，再分流到两个纯展示模态，全程未设置冲突槽位（于是 autosave 也不会因此暂停，用户可以再次「保存并关闭」重复撞墙）。
改进：该分支改用冲突记录入口（记录槽位+冲突模态），删除闭包外的重复模态；窗口已销毁的情形保留跨窗口静态入口兜底。

**P180｜另存为时目标相关的外部修改错误被当普通「保存失败」弹出，无解除入口**（persistence.rs:885-889/1120-1136/952-968；类别：体验｜低）
现象：「另存为」落到一个已在本编辑器其他标签打开的文件时，会对其做版本校验；若该文件已被外部改动，产生 ExternalChange 错误并沿结果通道冒泡，最终一律按「保存失败」弹普通模态——不判错误类型、没有「重载/另存为」入口，与手动保存对同类错误走冲突框的处理不一致。
原因：三个失败源（身份不匹配、目标被占、写盘失败）共用一条错误通道；上层错误分支只看字符串、不 downcast ExternalChange。
改进：错误分支补类型判别并转冲突记录入口；或让目标占用检查自己先弹冲突框再把错误降级为普通失败。

**P181｜watcher 事件不过滤自身临时文件，autosave 每轮都给文件树制造一次刷新噪声**（watcher.rs:29-43；session_watcher.rs:278-289；类别：性能｜低）
现象：autosave 每轮 prepare/rename 都在工作区内创建并删除临时文件，watcher 对 Create/Modify/Remove 各发事件并原样转发；树刷新对这些路径不做模式过滤，全部塞进 pending 并触发 250ms 后的读取更新（含 symlink_metadata+两次 canonicalize）。持续打字时每轮 autosave 制造 3-6 个事件→一轮树刷新；temp 通常已被 rename 掉，读出 NotFound 再走一遍空更新。与 P175 叠加放大主线程与后台 IO。
原因：事件转发层与消费层都没有识别自身临时文件模式。
改进：在监听闭包或入口按文件名匹配自身临时文件后缀直接丢弃（写临时文件的进程本来就是自己）；树刷新同样加一道防御性过滤。

**P182｜verify 与 rename 之间的 TOCTOU 窗口，外部写入可被最后一次提交静默覆盖**（persistence.rs:130-131；类别：缺陷｜低，设计固有）
现象：提交前的版本校验与 rename 是两个独立系统调用，之间若外部写者恰好改写目标文件，rename 会把外部内容覆盖掉且事后无任何检测。窗口极窄（微秒级），且没有跨进程文件锁的通用解法，属设计固有局限；但当前实现在校验失败时能弹冲突、在窗口内失败时无感知，二者行为不对称。
原因：校验与提交两步间无原子性保障。
改进：至少在 rename 后做一次廉价的 mtime/len 复查（rename 前记 metadata，rename 后比对，不一致则记日志并按外部修改处理）；或接受现状但在注释中明示这是已知窗口。

**P183｜has_marked_document_text 每次调度全量扫描所有块与表格单元实体**（persistence.rs:256-270；类别：性能｜低）
现象：该函数对全部可见块加全部表格格子逐个实体读，只在有 autosave 任务挂着时被短路。autosave 完成后到下一次编辑期间任务为 None，于是每个按键（每次调度）都要 O(块数) 次实体读取；万块级长文档每键多扫一万次实体表，快照闭包里还要再扫一遍。
原因：marked text 标记挂在块实体上，没有窗口级的「存在组合中」总开关，只能逐块查。
改进：编辑器层面维护「任意块带 marked text」的计数（组合开始/结束增减），先查 O(1) 计数，仅非零时才全量定位；快照闭包内信任调度入口刚查过的结果。

**P184｜恢复快照列表按 UUID 排序，多个草稿的恢复顺序与保存时间无关**（config/recovery.rs:73；类别：体验｜低）
现象：快照 id 是纯随机 UUID，按 id 排序得到的是随机顺序。崩溃时有多个未保存草稿的用户，每次启动恢复窗口/合并标签的处理顺序都不稳定，用户看到的恢复序列与崩溃前的标签顺序、时间线都无关。
原因：快照结构未存时间戳，读出后只能按 id 排序求确定性；文件 mtime 虽在但未使用。
改进：快照增加 saved_at 字段（或读取端用文件 mtime 兜底旧快照），排序改按时间；恢复多窗口时按崩溃前会话里的标签序还原更好。

**P185｜TextBuffer::edit 用 assert 处理越界区间，坏区间直接崩溃而不是降级**（buffer.rs:661-668；类别：健壮性｜中）
现象：`assert!(range.start <= range.end && range.end <= total)` 在产品路径 panic。上游有大量「过期区间」守卫，但任何一处漏守卫（新调用方、defer 回调的交错）都会整窗崩溃；同函数的字符边界只 debug_assert，release 下错位字节会静默写进多字节字符中间。
原因：唯一写入口选择了 fail-fast 策略，但没有给调用方留降级通道。
改进：断言改为钳位+日志或返回 Result，越界交给上层兜底档（整篇重投影）；至少把字符边界检查升级为 release 也生效的钳位。

**P186｜shared_changes 只增不清时无上界**（buffer.rs:345-348/714-730；类别：性能/内存｜低，待验证）
现象：有共享视图的缓冲区每次 edit 都 push 一条变更记录，只有显式 clear 才清。若分屏会话长期开着且消费方只读不 clear，记录 Vec 随编辑次数无界增长（每条数十字节×编辑次数）；长会话 + 高频编辑下是慢性内存增长。
原因：变更队列的生命周期与消费方解耦，没有「消费即清」或容量上限。
改进：给队列设容量上限（如 4096 条，超出丢最旧的并置「需要全量重同步」标志）；或在消费方读取后自动清空，明确「拉取即消费」语义。

## 十三、搜索、大纲、快速打开与命令面板

**P187｜「全部替换」确认计数在 UI 线程同步读盘整个工作区，且与执行阶段重复 IO**（workspace/find_replace.rs:691-722；search_backend.rs:248-288；类别：性能｜高）
现象：工作区范围点「全部替换」时，确认弹窗还没弹出来 UI 就先冻结。计数阶段对工作区每个文本文件逐个同步读盘并解码；两千文件、每份 100KB 的 vault 就是一次数百 MB 的阻塞 IO+解码，全部发生在按键处理线程上。而用户确认后，执行阶段又把同一批文件再读一遍——同一份数据两次全量 IO。
原因：计数分支只对活动文档和打开的脏标签走内存，其余一律同步读；既没走搜索那套带 mtime/len 校验的内容缓存，也没走后台线程。
改进：计数与执行共用内容缓存（首跑读盘并入缓存，确认后命中缓存零拷贝）；或把计数挪到后台线程，先弹无计数确认框、计数异步补上；至少复用一次读盘结果。

**P188｜全部替换的读-替换-写全链路同步跑在 UI 线程**（workspace/find_replace.rs:772-853；类别：性能｜高）
现象：确认弹窗点「替换」后，遍历所有文本文件的读取、正则整文替换、原子写盘、历史记录全都在模态回调里同步执行。工作区搜索本身特意做了并行分片+后台线程+panic 隔离，替换却把同等量级的工作全部压回主线程；大库上确认后 UI 卡死数秒到数十秒，期间连进度都没有。
原因：逐文件循环没有任何后台分片；每文件依次做 stat→读→解码→正则匹配→重建→临时文件写→rename→记历史。
改进：逐文件处理放进后台并行分片（照抄搜索的分片模板），UI 侧按代数收结果、分批刷新；冲突与写失败仍走既有冲突上报。

**P189｜⌘P 与 ⇧⌘P 两套全局浮层互不关闭对方，后开者只抢焦点不渲染**（quick_open.rs:252-258；command_palette.rs:49-56；render/paint.rs:790-811；类别：缺陷｜中）
现象：⌘P 打开快速切换后按 ⇧⌘P：命令面板状态被置位、焦点被移到它的输入框，但屏幕上快速切换面板纹丝不动——用户继续打字，字符进了那个不可见的输入框，列表毫无反应，看起来像「⌘P 坏了」。反向同理。只有 Escape 才能解开。
原因：浮层收口的通用函数不含这两个浮层；两个 toggle 又都只调它；渲染侧是 if/else-if 互斥渲染，先开者赢，后开者成了「有状态、有焦点、无 UI」的幽灵浮层。
改进：两个 toggle 互相关闭对方（或并进通用收口）；渲染 else-if 保留为兜底断言；确保窗口焦点落在实际渲染的浮层上。

**P190｜双链补全无匹配时 Enter 被整体吞掉，无法在 [[ 后换行**（workspace_index.rs:118-180；类别：缺陷｜中）
现象：输入 `[[不存在的名字` 后补全浮层照常打开（列表为空），此时按 Enter 想正常换行——什么都不会发生：换行没插入、浮层还关了。用户必须先 Escape 再 Enter，或手动敲 `]]`。空结果场景下是纯粹的按键劫持。
原因：enter 分支无条件 stop_propagation 后调确认函数；确认函数第一行先把会话拆掉，随后对空表取不到结果只 notify 就返回——既没插入文本也已把 Enter 消费掉。上下键分支都有非空守卫，唯独 enter 没有。
改进：enter 分支先判结果非空：为空时不消费直接放行让块处理换行；或把会话拆掉的动作移到确认成功之后。

**P191｜双链补全先截断再排序，候选取决于走盘顺序而非匹配质量；与可打开范围口径不一致**（workspace_index.rs:56-73；find_replace.rs:302-311；类别：缺陷｜中）
现象：补全列表永远最多 8 条，但这 8 条是「文件枚举顺序里最先匹配到的前 8 个」再按文件名排序——不是全库里最优的 8 个。枚举序不保证字典序，同名相近的文件可能整体缺席：库里有 notes/alpha.md，若枚举序里排在前面的 8 个文件都含查询字母，alpha.md 永远不出现。另外补全只列 Markdown，但确认后打开逻辑能按 stem 打开代码/文本文件——补全看得见的范围比链接真正能打开的范围窄。
原因：take(8) 挂在 filter 之后、排序之前；过滤只有子串包含，没有前缀/词边界优先级。
改进：先收集全部匹配再评分排序后截断（前缀命中 > 词边界 > 子序列，同级按文件名）；补全过滤范围与打开逻辑对齐。

**P192｜源码模式下搜索导航没有活动命中快速路径，每次 F3 整篇清高亮并重建映射**（workspace/tree_sync.rs:36-129；类别：性能｜中）
现象：同一份 10 MiB 命中密集文档，渲染模式按 F3 走快速路径只动一条标记（注释写明这是把 9.95 秒修成毫秒级的关键）；切到源码模式后同一个 F3 回到全量同步：先把上一轮每一根高亮块的区间逐实体清空（5 万命中就是 5 万次实体 update），再整篇重建源码映射，再逐块换算。源码模式连续按 F3 的代价与命中总数×文档大小成正比。
原因：快速路径判据把 rendered 硬编码进门槛；源码块按行切的投影位置不挂 span 上，整条优化被放弃而不是适配。
改进：给源码模式做同构快速路径：源码映射结果按（identity, revision, 分块版本）缓存，导航时只对活动命中所在 chunk 重算局部区间、只清/标那两块实体。

**P193｜超过 20MB 的文件正文被静默排除出工作区搜索，却仍参与全部替换**（workspace/search_backend.rs:249-251/415-417；find_replace.rs:707-719/805-808；类别：缺陷｜中）
现象：一份 25MB 的纯文本笔记在工作区搜索里永远搜不到正文，只有文件名能命中；界面没有任何「因过大跳过」提示。更糟的是口径分叉：点「全部替换」时确认框计数（无尺寸限制）会报告这文件里的命中数，替换执行也真的会改写它——用户在搜索面板里看不到的命中被批量替换掉了。
原因：缓存大小门槛被实现成「超限即 None」，搜索把 None 实现成「跳过内容搜索」；名单阶段的 skipped 计数只统计读不了的条目，尺寸淘汰发生在之后的读取阶段。
改进：超限文件不应静默跳过：绕过缓存整读搜索（一次性读 20MB 可接受），或至少计入「N 个大文件未搜正文」提示；替换侧与搜索侧共用同一份「可搜文件」判定。

**P194｜大纲节点 id 由行号生成，标题上方任何增删行都会整树重建并清空展开态/选中态**（workspace/input_handler.rs:519/267-280；tree_sync.rs:1131-1211；类别：体验｜中）
现象：用户把大纲 H3 展开到深层后，在任何一个标题上方敲一个换行：所有标题行号+1，原地换标签因行号对不上而失败，走整树重拼——新树节点 id 全部变了，展开集合按 id 清账、选中态归零，然后按默认只展开到 H1/H2。用户精心展开的大纲一瞬塌回默认，滚动跟随选中项也丢了。在长文档开头改字，每次按键都触发一次。
原因：增量缓存本身是对的，但身份标识用了易变的行号；展开/选中状态按 id 记账，id 换了状态就丢。
改进：节点 id 换成稳定标识（按「文档序中第 k 个标题」生成），重建树时按序对位保住展开/选中；或重拼后做一次新旧树按 level+label 序列对齐的 id 迁移。

**P195｜folded_headings_above 每次搜索跳转克隆整份可见块表**（workspace/tree_sync.rs:942/898-934；类别：性能｜中）
现象：10 MiB（约 10 万根块）文档上按 F3：跳转每次都调「展开命中所在章节」，对命中首尾各查一次，每次都先把整份可见块表 to_vec 克隆一遍再扫描。每根块是一个 Entity 克隆（引用计数+GPUI LeakDetector 登记），同形状克隆实测在 10 万根时「每帧 3.7-4.7ms」。一次导航最多付两遍，F3 连按时是纯白付的热路径成本。
原因：to_vec 只是为了绕借用，但循环体内只读实体，不可变借用完全可以直接跨越循环。
改进：去掉克隆直接迭代借用；或给 DocumentTree 增加「从头到某可见下标的折叠栈」批量接口，顺带把弹栈逻辑留在树内。

**P196｜文档范围结果列表 ↑/↓ 与 Enter 实际跳转所用的活动命中脱节**（workspace/render_search.rs:687-715；find_replace.rs:441-484；类别：缺陷｜低）
现象：侧栏列表获得焦点时按 ↑/↓，只有工作区范围消费的索引被更新，高亮行移动了；文档范围的活动命中纹丝不动。此时把焦点点回查询框按 Enter，跳转从上一次的命中继续前进，而不是从列表里高亮的那一行跳。工作区范围反而一致——同一套按键，两个范围语义不同。
原因：列表导航函数是两个范围共用的，却只维护了工作区范围消费的字段。
改进：导航函数在文档范围分支里同步活动命中区间/下标，并让正文的活动命中标记跟随列表；使 ↑/↓+Enter 与点击行为一致。

**P197｜文档范围的结果计数显示的是投影行数（≤200）而非命中表的真实命中数**（workspace/render_search.rs:431-436；document_matches.rs:242-280；类别：体验｜低）
现象：命中表可能有 5000 处命中，但侧栏投影只取前 200 行，右下角「N 个结果」显示的是被截断后的 200——用户以为全文档只有 200 处。而 Enter/F3 导航明明还能继续跳到第 201+ 个命中（有测试钉住），列表数字与导航能力互相矛盾；「全部替换」确认框又会报出真实的 5000——同一个查询两个数字。
原因：计数标签读的是投影后的行表；真实总数躺在命中表里没有被用。
改进：文档范围计数标签改为读命中表长度（可附「列表显示前 200 条」），两处数字同源。

**P198｜命令面板只按 AI 可用性过滤命令，只读/无文档上下文仍列出会静默 no-op 的写命令；且每帧为每条命令分配动作对象**（command_palette.rs:96-110/153-155；find_replace.rs:491-497；类别：体验｜低）
现象：面板的可执行性判定只有「需要 AI 而未启用则隐藏」一条。在只读文档、无文档窗口里，「替换全部」等命令照样列出——回车后面板关闭、然后什么也没发生（替换入口首行只读检查直接 return false，无提示）。这正是「可用判定与动作分叉」。另外过滤函数在每次渲染帧执行，对每条命令分配一个 boxed action+本地化标签，命令几十条时每帧白付几十次堆分配。
原因：命令规格上只有 AI 一个可用性位，没有按编辑器状态的判定；渲染函数直接调过滤没有缓存。
改进：命令规格增加按编辑器状态的 enabled 判定（只读、有无文档、有无选区），面板按它置灰或隐藏；条目按查询与上下文缓存。

**P199｜命令面板输入在 IME 组词期间没有 composing 守卫，与 ⌘P 的处理不一致**（command_palette.rs:58-77；对照 quick_open.rs:269-283；类别：缺陷｜低，待验证）
现象：用拼音输入法在 ⇧⌘P 面板打字时，每次组词中间态若触发 Change 事件，订阅就把它当成查询喂给列表过滤，候选列表随组词碎片闪烁；而 ⌘P 的同一位置明确检查了 marked text 并在组词中跳过刷新。两个面板对同一输入法行为不同：⌘P 稳、⇧⌘P 闪。最终确认两侧都另有确认期的 composing 检查所以不会误执行，但过滤体验分叉。
原因：两份代码是同一模式的复制粘贴，一份带了守卫一份漏了。
改进：补上同样的 composing 检查：组词中仍可更新查询显示但跳过列表过滤，组词结束的那次 Change 自然收敛。是否真会闪取决于 gpui-kit Input 在组词期是否发 Change（待验证），但两处行为应当统一是确定的。

**P200｜`[[目录/文件名]]` 形式的双链点击后静默无动作，且无工作区根时新文件落到进程当前目录**（workspace/find_replace.rs:291-342；类别：体验｜低）
现象：Obsidian 风格的子路径双链点击后什么都不发生：按 stem/文件名匹配（都不含斜杠，永远匹配不上子路径文件），找不到后名字合法性检查因含 `/` 拒绝，直接 return——没有错误、没有提示、没有打开任何东西。另外当工作区根为 None（单文件打开）时，创建分支落到进程当前目录，会在启动目录里悄悄生成 `<target>.md`。
原因：名单匹配只做了 stem 与 file_name 的全等比较，没有按 `/` 拆分做相对路径匹配；守卫失败路径没有接错误上报。
改进：匹配阶段支持相对路径（按 / 归一后与相对工作区根的路径比较，含无扩展名形式）；解析失败且名字非法时给可见提示；无根场景要么禁建要么以当前文件所在目录为基准。

**P201｜⌘P 每个按键克隆整份工作区文件名单，排序只按文件名长度**（quick_open.rs:308-339；类别：性能｜低）
现象：每敲一个字符，先把整个文件名单过滤后逐个克隆成新的 Vec（10k 文件就是 10k 次路径克隆+分配），再线性过滤；随后排序只按文件名字节长度升序。排序质量弱：查 sett 时 asset.rs（子串在中间、更短）排在 settings.rs（前缀命中）前面，因为只比长度不比位置；同长度并列时保底是走盘顺序，名单刷新后顺序可能变化，选中项感知上「跳来跳去」。
原因：名单每键全量重建无缓存；排序是刻意从简的策略，没有匹配位置、连续性、词边界这些 fzf/VS Code 基本评分维度。
改进：名单只读引用即可完成过滤（只 clone 进入结果的十几条）；排序引入轻量评分（首命中下标越小越优、词首命中加权、再按长度），同分保持稳定次序。

**P202｜双链补全浮层的贴底钳制不考虑面板自身高度，窗口底部附近被裁切；同一坐标算了两遍**（workspace_index.rs:222-242；类别：体验｜低）
现象：光标靠近窗口底缘时输入 `[[`，浮层 top 被钳到「视口高-48px」，但 8 行面板高约 216px，下半截溢出视口被裁掉，后几项（含选中项）点不到也看不到。另外面板原点与实际渲染的 left/top 是同一公式的两份拷贝，前者只进了点击判定用的 bounds，两处将来一改必漂。
原因：48px 是按单行菜单拍的高度余量，与真实面板高度没有关联；宽度钳制也各写了一份。
改进：top = min(caret_bottom+4, viewport.height - panel_height - margin)，必要时向上翻转到光标上方；把原点/尺寸算一次存进局部变量，bounds 与渲染样式同源。

## 十四、段落/格式/插入操作族

**P203｜插入块入口产品路径两处 expect，且可用性判定先于物化 flush，检查与执行不在同一份状态上**（insert_ops.rs:66-83；类别：缺陷｜高，可达性待验证）
现象：插入块的入口在产品路径上直接 `roots.last().cloned().expect(...)` 与 `.position(...).expect(...)`，违反「产品路径禁 expect」不变量，release 下 panic 即 abort。更重要的是时序：可用性判定用 flush 前的树，随后才 flush 物化、再重算根块列表——一旦物化改变了选区到根块的映射，重算得到空列表，expect 当场崩溃。
原因：可用性判定→flush→重算的顺序与同族段落操作的正确写法（先 flush 再判定，判定与执行读同一份后置状态）相反。
改进：把可用性判定挪到 flush 之后（或 flush 后重新判空），expect 换成 let Some else return false；顺带清理同函数里恒为 Some 的死分支。

**P204｜段落换种类全程不刷新引用注册表，代码块↔正文互转后图片/链接/脚注状态过期**（paragraph_ops.rs:59-87；类别：缺陷｜中）
现象：把一个含 `[logo]: /a.png` 或 `[^1]` 字面量的代码块换成正文（或反向）后，图片/链接引用定义表与脚注注册表不重建：引用式图片持续渲染失败、脚注序号错乱，直到用户再编辑某个「敏感块」才自愈。而插入、粘贴、引用、表格等所有其它结构路径都显式刷新了。
原因：换种类走的 set_kind_in_place 注释明说「不发事件、不开撤销组」，因此靠 Changed 事件触发的重建永远不会跑；元数据重建与缓冲区重同步也都不碰注册表。
改进：换种类的变更分支里，对被换块跑一次「含引用定义记号」判据：命中（或换出前命中）才整篇重建，否则至少对这几块同步运行时上下文。

**P205｜ensure_block_kind_boundary 用未校验可信度的 span 直接切缓冲区，坏 span 会 panic**（paragraph_ops.rs:170-189；类别：缺陷｜中）
现象：`buffer.slice(before.end..after.start)` 的区间来自块 span 表，只查了顺序与上界，没查字符边界；TextBuffer::slice 内部是裸字符串切片，端点落在多字节字符中间即 panic、release 即 abort。仓库在 runtime_context 已为同一类问题立了 span_is_trustworthy 这「唯一判据」——同族风险这里没接同一份防护。
原因：相邻块的 span 在撤销/重放、区间平移等路径上存在过期可能（仓内多处注释自认「区间不可信」），这里直接信 span。
改进：切片前加 span_is_trustworthy 检查，不可信直接 return（少补一个空行的代价远小于 abort）；判据仍收口在一处。

**P206｜图片落盘发生在撤销组开出之前，撤销插入不回收孤儿文件**（events/paste.rs:426-464；insert_ops.rs:156；类别：缺陷｜低）
现象：「插入→图片」先把图片文件写进磁盘，之后才开撤销组。用户按一次撤销，markdown 字节被精确回滚，但刚落盘的图片文件留在磁盘上；只有「插入彻底失败」这一分支会删除文件。反复插入+撤销会在附件目录积累孤儿资源，且这些文件还会被工作区索引/图片预览扫到。
原因：撤销系统只记账缓冲区字节，对文件系统副作用无感知；失败清理与撤销没有打通。
改进：把 created_path 记进撤销组附带元数据，撤销该组时删除仅在本次会话创建且未被引用的文件；至少在自动保存/关窗时对「存在但无任何 markdown 引用」的本会话新建图片做清扫。

**P207｜「插入→图片」选择器无扩展名过滤，选中不支持的文件静默返回**（insert_ops.rs:169-197；类别：体验｜低）
现象：文件选择器没有传任何类型过滤，用户可以选到 .txt/.pdf；回调里找不到支持项就裸 return——窗口没有任何提示，用户点了菜单、选了文件，界面毫无反应。而粘贴路径对同类失败是有应用内模态报错的，两条入口行为不一致。
原因：find 把「取消」与「选了不支持的文件」合并成同一条静默路径；选择器侧没把现成的图片扩展名表传给原生对话框做过滤。
改进：选择器按图片扩展名传白名单；至少「有 paths 但无支持项」时报错提示，取消才保持静默。

**P208｜`__强调__` 在「关一次再开一次」后丢失下划线写法；同一命令内会混出两套记号**（inline/stacks.rs:384-404；类别：体验｜低）
现象：用户原文 `__粗体__`：选中后加粗一次（移除）→ 再加粗一次（加回），存盘变成 `**粗体**`——违反「强调记号写法要保留/按用户已有写法套用」。另一面：对「`_斜_` 与普通文本」的混合选区一次性加粗，旧片段保留 `_`、新片段取 `*`，同一次命令产出混搭写法。
原因：关闭强调且无斜体时把 emphasis_marker 整个清成 None，重新开启时只会回规范星号；「写法」的存活期被绑在样式标志上，而不是跟着块/文档记。
改进：关闭强调时把 marker 降级为「块级最近一次使用的写法」而非清空（或在块记录上记 preferred_emphasis_marker）；同一命令批量翻转时先对选区内已有 marker 做多数表决再统一赋值。

**P209｜行内格式切换顺带跑了快捷语法检测，可把段落静默变成列表或标题**（block/runtime/text_ops.rs:253-263 → normalize.rs:10-72；类别：缺陷｜中，触发面窄但机制真实）
现象：对一个可见文本以 `# `/`- `/`1. `/`[ ] ` 开头但 markdown 源里是转义/字面的段落（如输入 `\#` 得到的显示文本，或粘贴为纯文本的 `* item`），执行加粗/清除格式会把它顺手转成标题/列表/任务项，并把格式区间按删掉的前缀长度平移。格式命令改变了块种类，属于禁止的「顺手规范化」。
原因：行内格式复用 apply_title_edit，而后者无条件跑快捷检测；该检测只看可见文本，与解析期基于源码（含反斜杠）的判定不同源——转义场景下两边判定必然分叉。
改进：apply_title_edit 增加允许快捷检测的开关（或拆出不做快捷检测的变体），行内格式与清除格式两条路径关闭检测；只有打字/粘贴路径保留。

**P210｜换种类后选区与 IME 跨度不重映射，code→paragraph 后选区可能指向越界坐标**（block/runtime/text_ops.rs:75-100；类别：体验｜低，越界后果待验证）
现象：代码块内容换成正文后显示文本缩短，但 set_kind_in_place 不收敛选区、不清 IME 标记（对比同族的进入代码块/变分隔线都做了），旧选区原样留在更短的坐标系里。后续大部分路径靠 min(len) 兜底，但光标偏移、行号定位、IME 标记等直接消费该值的路径行为未定义。
原因：换种类只做投影清理与模式同步，三个函数均不改选区；而编辑模式切换本身意味着显示坐标系整体变了。
改进：set_kind_in_place 末尾按新旧可见长度把选区两端收敛并重设光标亲和，标记置 None；与同族函数的收尾对齐。

**P211｜有序项换「任务列表」被静默降级为无序 `- [ ] `，族信息丢失**（paragraph_ops.rs:390-392；state.rs:918-929；类别：体验｜低）
现象：对 `1. 待办` 选「任务列表」，得到的不是 GFM 同样合法的 `1. [ ] 待办`，而是 `- [ ] 待办`——序号族被悄悄换掉。序列化侧任务项写死无序记号，record 里记的 `.`/`)` 分隔符被吞掉；转换时的记号继承也只认「无序+任务是一族」，有序邻项的记号抄不过来。
原因：块模型把任务项折叠进无序族，转换目标不区分当前是有序还是无序。
改进：若解析/序列化支持有序任务框则让序列化尊重记号分隔符；若产品明确只做无序任务，至少在菜单置灰有序项上的「任务列表」或在文档里写明降级行为。

**P212｜插入兜底分支（锚点无源码区间）提前返回，跳过聚焦与注册表同步**（insert_ops.rs:103-110；类别：缺陷｜低）
现象：锚点块没有源码区间（后台续建到一半）时只做补尾段落+mark_dirty 就返回成功——与主路径相比缺了两件事：新块不落光标；注册表不刷新。用户在超大文档尾部插入代码块时，表现为「插是插进去了，但光标没过去、引用状态没跟上」。
原因：该分支是为「字节没法按插入点算、整篇重投影兜底」设计的提前出口，没把主路径尾部的收尾一并带走；收尾清单靠手抄维护。
改进：把收尾清单抽成一个私有函数（补尾段落→按写回结果置脏→刷新注册表→聚焦→结算撤销组），主路径与兜底分支共用。

**P213｜runtime_context 同一段防护注释被完整粘贴了两遍**（runtime_context.rs:20-29；类别：可读性｜低）
现象：「区间不可信时不要拿它去切片……」这一整段论证出现两次，第二次还把判据来源改写成 span_is_trustworthy 的说法。注释重复掩盖了一次真实的修订痕迹（旧口径→新判据），后来者容易把两段当成两条不同约束去分别维护。
原因：引入新判据时的编辑把新说明追加在旧说明后面而没删旧的。
改进：删除旧段保留带新判据的段落；顺带确认 P205（paragraph_ops 侧）真的接上了这份判据——目前「判据只有一处」的承诺在该处是破的。

## 十五、公式编辑器、补全、预览与拖放

**P214｜块级 `\` 补全对未知命令残留一个空的悬浮面板**（latex_completion.rs:71-96/302-321；类别：缺陷｜中）
现象：数学块里输入无匹配前缀的命令（如 `\zz`）后，光标下方持续悬浮一个约 300×38px 的空面板：有底色、边框、阴影且 occlude。它不随继续输入消失，盖住下方正文的鼠标点击；上下键也被吞，只能 Esc 或删字收掉，观感是「幽灵空框」。
原因：更新函数拿到空结果后照常创建/更新会话，没有空结果检查；渲染侧把空列表也按 1 行算出实心面板。对照公式弹窗的补全刷新有空结果过滤，同一功能两条路径口径不一致。
改进：结果为空时关闭会话并提前返回（创建与更新两处都覆盖）；渲染侧去掉单行兜底双保险；补「未知命令不弹空面板」用例。

**P215｜键盘移动光标后补全锚点过期，Enter 确认会残留重复的命令文本**（latex_completion.rs:179-210；类别：缺陷｜高）
现象：补全面板开着时用 Home/End/←/→ 移动光标（这些键不产生文本变更事件，面板既不关闭也不重算锚点，但渲染上跟随新光标位置）。此时按 Enter/Tab 确认：若光标已移到原命令之前，模板被插进原位置而查询串原样残留，得到 `\frac{a}{b}\fr` 这类重复文本，光标还落在模板里，用户不易立即察觉文档已被悄悄改坏。
原因：确认时把光标偏移与锚点做 max 合成——光标小于锚点时被抬回，替换区间退化为空区间，替换只插入不删除。关闭会话只在点击面板外、Esc 等处调用，光标移动事件无任何处理；公式弹窗对方向键显式关闭补全，块级路径漏了同款防护。
改进：确认前校验光标是否仍在锚点区间内，不满足就关闭会话放弃确认；光标移动的事件分支对开着的光标上下文重判一次（光标不在查询尾即关闭）。

**P216｜草稿撤销合并不随光标移动断开，一次 ⌘Z 跳过中间状态**（formula_editor.rs:97-116/1043-1065/1194-1228；类别：缺陷｜中）
现象：弹窗里连打几个字、移动一下光标（方向键或点击换位置）再继续打字，随后 ⌘Z 一次会把「移动前+移动后」两段连续输入一并撤销，直接回到第一段之前；中间状态没有独立撤销步。删除键同理。
原因：合并条件里的标志只在撤销恢复时复位；移动光标与点击定位只改选区，从不触碰该标志，光标移动后的下一次单字符编辑被继续并入旧组。
改进：在移动光标、点击定位（以及 IME 收尾等只改光标不改文本的入口）补一行「断开合并」标志复位，确立「光标移动即断组」的标准文本框口径；补「打字-移动-打字-撤销两次」粒度测试。

**P217｜公式草稿每键同步渲染+写盘，全部压在主线程；磁盘缓存无清理入口**（formula_editor.rs:554-586；latex/mod.rs:397-483；类别：性能｜中）
现象：弹窗里每个字符输入（含 IME 组合期间）都同步执行「解析+排版+SVG 序列化+临时文件写入+rename」。简单公式无感，但矩阵/aligned 等复杂环境每键可达数十毫秒，长公式打字出现可感知卡顿；且打字的每个中间态都是缓存未命中，会在缓存目录为每个中间态永久新增一个 SVG 文件——进程内缓存有 LRU 淘汰，磁盘却没有任何清理入口。
原因：预览同步在四条编辑路径上即时调用；缓存键含公式体，打字时逐键必 miss，随后写盘全在主线程。
改进：渲染改防抖 100-150ms + 后台线程执行，完成后回主线程刷新预览，失败路径维持同步报错；磁盘缓存加容量/条数上限或启动清理。

**P218｜粘贴进公式草稿不剥离 \r，应用后把 CR 写进文档**（formula_editor.rs:1099-1112；类别：缺陷｜中）
现象：从 Windows/网页复制的 CRLF 文本在公式弹窗里 ⌘V 粘贴后，草稿混入 \r：渲染按 \n 切行，\r 留在行尾参与排版，视觉上几乎不可察觉；「应用」时草稿被原样写回块文本，\r 随之进入 markdown 源码，与全文档的 CRLF 归一化口径相抵，后续保存、搜索、diff 都可能踩到。
原因：粘贴路径直接取剪贴板文本无归一化；而同一草稿的 IME/系统插入路径明确做了剥 \r——同一份草稿的两条写入口径不一致。
改进：对粘贴文本做 CRLF 归一（与整篇导入同款），并把归一化收敛成共用函数供两条路径调用。

**P219｜遮罩点击与 Esc 无差别丢弃草稿，无任何确认**（formula_editor.rs:547-552/788-797/1787-1792；类别：体验｜中）
现象：弹窗打开时点击面板外任意一点立即关闭并整体丢弃草稿，没有「未应用的修改将丢失」确认；Esc 同样直接丢弃。用户拼好的长公式一次误触就全部蒸发，草稿撤销栈也随状态一起销毁，无法找回。
原因：遮罩挂着直接关闭的鼠标回调，而关闭函数只是把状态整个 take 掉；文档注释写明这是设计，但没有区分「有未保存改动」与「无改动」两种情形。
改进：关闭前比对草稿与打开时的快照，不同则弹应用内确认（应用/放弃/继续编辑）；或至少把遮罩点击从「关闭」降级为「不动作」，关闭只走 ×/取消按钮与 Esc。

**P220｜弹窗内全量 shape_text 在每帧与每次指针事件重复执行**（formula_editor.rs:1117-1173/1343/855/1184；类别：性能｜低）
现象：弹窗打开期间，渲染函数每帧对草稿每个硬行做带语法色的完整 shape；光标闪烁任务每 0.5s 两次 notify 也各触发一轮全量重排；拖动选择时每次 mouse_move 再量一遍。多行草稿下拖选跟手性下降，闪烁重绘成本按整份草稿计。
原因：「渲染、光标、补全浮层、命中四处共用一份布局」的实现方式是每次用到都重算，没有按（草稿内容、宽度、字体）为键的 memo。
改进：布局缓存进弹窗状态（草稿指纹+宽度+字体为失效键），渲染与命中共用；至少拖选路径让一次 pointer_move 只做一次测量。

**P221｜setext 标题恰好跨 512 行分块接缝时丢失标题着色**（markdown/source_highlight.rs:114/19-72；类别：缺陷｜低）
现象：源码文档按 512 行分块；若某段的段落行落在第 512 行、其 setext 下划线行落在下一块第 1 行，则前块把正文按普通段落着色收尾，后块因接缝状态里没有押着的段落把下划线当分隔线，整条 setext 标题都不出标题色——渲染视图里它是 H2，两侧不一致。
原因：接缝状态枚举只携带围栏/公式/frontmatter/HTML 注释，「段落候选」是扫描器私有字段跨块不传递；收尾 flush 把押着的候选行就地按正文消费。
改进：接缝状态增加「上一行为段落候选」变体（下一块首行先按下划线判定再回落正文）；至少补接缝 setext 的已知偏差测试防止无人知晓。

**P222｜引用内的围栏代码块不进状态机，围栏内容不获代码着色**（markdown/source_highlight.rs:267-276/427-476；类别：缺陷｜低）
现象：`> ```rust` 开头的引用内围栏：剥掉引用前缀后只按「标题/标注/列表/行内」识别，反引号串落进行内扫描；其后 `> let x = 1;` 等行也不会进入围栏状态，整块只有零散行内构造色，与渲染视图（引用内完整代码块、有关键字色）明显不一致。
原因：引用续行扫描没有围栏开栏分支，也不设置任何接缝状态；状态机遇到引用行直接消费整行返回，围栏开栏信息被丢弃。
改进：引用续行扫描识别围栏开栏，引入带「引用前缀宽度」的围栏状态变体，续行先剥引用链再按围栏内容处理；工程量大时先做保守版——识别到引用内围栏后整块按代码着色。

**P223｜`<!-->` / `<!--->`（HTML 允许的自闭合注释）误开跨行注释状态**（markdown/source_highlight.rs:235-250/370-378；类别：缺陷｜低）
现象：一行只写 `<!-->`（CommonMark/HTML 规格里这是完整的空注释）时，向后找闭合落空，扫描器进入注释状态；此后整篇直到真正出现 `-->` 的行为止全部着注释色，而渲染端多半把它当普通段落，两侧出现大面积串色。`<!--->` 同理。
原因：开栏判定只有「以 <!-- 开头+向后找 -->」两步，没有处理「<!-- 后紧跟 > 或 -> 即自闭合」的特例。
改进：进入状态前先判自闭合形状，按单行注释着色返回；与 CommonMark 的合法注释口径对齐，补测试。

**P224｜跨接缝的围栏内容各自半截递归高亮，语言内部跨行状态断裂**（markdown/source_highlight.rs:336-423；类别：缺陷｜低）
现象：一个 600 行的 rust 围栏横跨分块接缝：前块把前半段内容收进嵌套区域并立即递归交给代码高亮，后块拿到围栏接缝状态后把后半段再独立解析一次。跨接缝的块注释、未闭合字符串/括号的着色在接缝两侧各说各话——模块头宣称的「分块着色也能接得上」对围栏内部不成立。
原因：接缝状态只携带围栏字符/长度/语言，不含语言内部扫描状态；tree-sitter 逐块解析本就没有跨块上下文。
改进：短期接受为已知限制写入模块文档；若要真正修复，需要高亮管线支持「携带上一段解析状态续析」，成本较高，需与分块收益权衡后立项。

**P225｜解析失败的公式块「应用」时把整段 raw 再包一层 $$，产出嵌套定界符**（formula_editor.rs:500-503/726-752；类别：缺陷｜低，待验证）
现象：若数学块是解析不出分段的形状（如未闭合的 `$$\nx`），打开弹窗时草稿直接取整段 raw（含开栏 `$$`）；用户改动后按「应用」，写回分支把整段再包一层 `$$...$$`，块文本变成嵌套定界符——内容损坏且渲染必然报错。
原因：写回守卫只做「无变更」比对，假设该形状是纯正文但没有校验原文是否真的不含定界符；打开侧对解析失败的形状原样透传含定界符的 raw，两侧假设没有互相钳制。
改进：写回前检查原文是否已含定界符：含则先剥再包，或直接拒绝应用并提示；打开侧对「含定界符但解析失败」的块展示去定界后的草稿。

**P226｜弹窗打开期间切主题，公式预览停留旧主题色/字号**（formula_editor.rs:530/629/1037；类别：体验｜低）
现象：公式弹窗开着时系统自动切换深浅色：输入区文字与语法色实时跟随新主题，但上方预览仍是旧颜色/字号的缓存 SVG，直到下一次按键编辑才被重渲。mermaid 预览有专门的主题同步挂钩，公式弹窗没有对应机制，同类浮层行为不一致。
原因：预览同步只在打开、草稿编辑、撤销/重做、IME 插入四处调用，没有主题变化观察者；缓存键含颜色与字号但只在下一次调用时才生效。
改进：在编辑器的主题变化处理（mermaid 同步被调用的同一位置）对公式弹窗存在的情况重调一次预览同步；或抽统一的「浮层主题同步」入口同时管两类弹窗。

**P227｜mermaid 预览两条关闭路径不还焦点，与遮罩 Esc 行为矛盾**（image_preview.rs:98-165；类别：缺陷｜低）
现象：预览键盘处理的 Esc 分支直接把预览状态置空，跳过正式关闭函数的焦点交还逻辑；主题切换重渲失败同样直接置空。这两条路径下浮层焦点已从正文抢走，直接丢弃后焦点悬空，用户必须用鼠标点回正文才能继续打字；而遮罩层的 Esc 走正式关闭函数会正确还焦点——同一个 Esc 因焦点陷阱是否活跃走两条行为不同的路径。
原因：键盘拦截器在焦点陷阱活跃时提前返回走遮罩路径；陷阱未激活的窗口期走拦截器内的处理，其 escape 分支没有升级焦点交还。
改进：两条路径统一改为调用正式关闭函数，确立「关闭必还焦点」的单一出口。

**P228｜图片预览放大后无法平移，只能看到中心区域**（image_preview.rs:521-563；类别：体验｜低）
现象：滚轮缩放支持 0.1-8.0 倍，但容器是居中+裁切布局，图片放大超出视口后四周被裁掉，没有拖拽平移或滚动查看的手段；想看图片角落只能缩小到整体可见。与同一文件里 mermaid 预览（拖拽/平移/钳制一整套）能力不对等。
原因：外层容器只挂了滚轮缩放，没有鼠标拖拽状态；预览状态只有 zoom 字段，图片以绝对居中布局摆放。
改进：给图片预览增加 pan 与拖拽处理（照抄 mermaid 三件套），图片按 pan 定位；或最低成本把容器换成可滚动的 overflow_scroll。

**P229｜恰好 512 行且以换行结尾的源码文档多出一个空尾块**（file_drop.rs:32-41/326-340；类别：缺陷｜低）
现象：一个正好 512 行、末尾带换行的代码文件：按换行切分得 513 段（最后是空段），恰好越界进入分块，切出「前 512 行」与「空串」两块；文档替换分支的整文条件不成立，块树尾部多出一个空块，源码视图末尾多一条空行。
原因：行数判断被结尾换行符产生的空段骗过；尾空段来自结尾换行符本身，不是真实的第 513 行，也没有被并入前块。
改进：分块前剥掉结尾换行、重组时补回；或对「末段为空」特判并入末块。现有测试覆盖了 513/1024/2000 行档位，独缺「恰好 512 行+尾换行」这一档，补一条锁死。

**P230｜markdown 源码回退整文同步建块，不走代码文档的后台续建**（file_drop.rs:312-340；类别：性能｜低，待验证触发频率）
现象：代码文档超过 512 行时首块同步、余下经后台续建；但 markdown 源码回退（含歧义语法的文档强制源码视图）哪怕几万行也一次性同步建完所有块——同样的分块在这里不享受窗口化，大 markdown 切到源码视图时打开路径整体卡顿，与「大纯文本不整文件压单块」的性能目标相悖。
原因：后台续建守卫把「是代码文件」写死在条件里，markdown 回退分支虽然同样走分块扫描，却因不是代码文件被推进全量同步构建。
改进：守卫放宽为「还有剩余字节」不区分文件类型，统一两条大文档路径；需先确认续建完成后接缝状态重串的时序，以及撤销/选区快照在未物化尾块上的行为（代码侧已有全量物化兜底可复用）。

---


## 十六、vim 模式

**P231｜块状可视模式下 `r` 走错路径并插入杂散字符**（vim/operate.rs:2351-2383；visual.rs:154-172；类别：缺陷｜高）
现象：块状可视（ctrl-q）选中同一代码块的多个硬行后按 `r x`，不产生 vim 语义的逐行替换，而是在首行行首插入一个杂散字符；若选区跨块则静默无操作。块状可视的 r 功能整体不可用且有破坏性副作用。
原因：VisualReplaceChar 直接路由到字符替换函数，该函数没有像 operator 执行器那样把 Block 选区分流到块算子，而是复用统一选区换算；Block 分支返回的区间是「首块行首 → 末块 offset:0」的哨兵形状，同一块内多行选区时 end<start 被钳成空区间，但替换文本仍按至少一个字素生成，replace 变成插入。
改进：在替换入口加 Block 模式分流，仿照块算子逐行用块可视区间把每行选中段替换；同时让选区换算的 Block 分支不再返回 offset:0 哨兵值，避免其他调用方再踩坑。

**P232｜R 模式退格还原用「原字符宽度」当「当前文本宽度」，多字节覆写吞掉相邻字符**（vim/operate.rs:2386-2440；类别：缺陷｜高）
现象：R 模式下把一个多字节字符（如「世」，3 字节）覆写成单字节字符（如 x）后按退格，还原时会连带吃掉后面的字符——把 x 及其后 2 字节一起替换回「世」，后文丢失。反向（单字节覆写成多字节）则导致还原时区间重叠错乱。
原因：覆写用原字素宽度删除、以新字符宽度插入，文本总长度随之变化；还原时区间长度取自原字符的字节宽，而当前文本在原位置放的是宽度不同的替换字符，删除区间覆盖到了替换字符加后续内容。
改进：还原时先计算当前文本中该位置的实际字素宽度，用「当前宽度」做删除区间再插入原字符；或维护 (位置, 原字符, 替换宽度) 三元组按替换宽度回退。

**P233｜可视模式 `J`/`gJ` 忽略选区范围，只并 count 行**（vim.rs:2141-2154；operate.rs:2598-2659；类别：缺陷｜中）
现象：行可视选中 5 行后按 J，只从焦点所在行并 2 行（默认 count），选区其余 3 行原样保留；vim 的 {Visual}J 应把选区覆盖的全部行并成一行。选区越长偏差越大。
原因：可视解析里 J 只生成 count（来自按键前缀，无前缀时为 2），选区信息完全没进命令；执行端也只按 count 从当前光标向下并，从不读取选区目标。
改进：可视解析分支把选区目标一并传入（或解析时按选区算出行数填进 count），执行端对可视来源按选区行数从首行开始并「行数-1」次。

**P234｜行级可视选区的 `.` 重放按硬行计数，重放移动走视觉行，软换行下形状漂移**（vim.rs:1426-1493；motion.rs:902-954；类别：缺陷｜中）
现象：在被软换行拆成多个显示行的段落里，`Vj d`（选区跨到下一段落）之后用 `.` 在另一段落重放，重放只移动「硬行数」次 Down，而 Down 每次只走一个视觉行，结果重放选区比原选区小，删除内容与首次不一致。
原因：采集重放素材时按硬行差计数；重放时把该差值作为 Down 的 count，而垂直步进在非代码块中按视觉行步进，两个口径在软换行段落里不相等。
改进：重放改为直接以记录的目标位置（升级为 EntityId+偏移）落焦点；或提供「按硬行步进」的变体供重放使用，保证采集与重放用同一种行定义。

**P235｜`'<`/`'>` 与 gv 的 last_visual 存块下标，结构编辑后指向错误块**（vim.rs:227；visual.rs:47-65；operate.rs:2684-2690；类别：缺陷｜中）
现象：可视选中若干块后退出，再删除前面某个段落，随后 `:'<,'>s/a/b` 或 `gv`：按当前块下标取实体会命中平移后的错误块，替换/重选落在错的内容上。
原因：last_visual 用「导航块列表下标+偏移」保存，而块下标在结构编辑后整体平移；同一份状态里 marks/jumplist 都已改用 (buffer_identity, EntityId, offset) 的稳定坐标，唯独 last_visual 还是下标坐标，两套口径混用。
改进：last_visual 升级为与 marks 相同的稳定坐标端点；gv 重选前先校验身份一致、实体仍存在，否则放弃重选而不是落到错误块。

**P236｜可视 `p` 用行级寄存器替换字符级选区时，粘贴内容插到错误位置**（operate.rs:2505-2594；类别：缺陷｜中）
现象：在段落中间字符可视选中几个字，若寄存器内容是行级（如之前 `yy` 的整行），p 后行级内容作为新块插到当前块之前，被删选区前面的文本留在当前块里、排在粘贴内容之后；vim 语义是在光标处拆行插入，前后文保持原顺序。
原因：删除选区后行级内容一律走「插到活动块上方」的路径，而执行时活动块是删除后仍持有「选区前缀」的原块，前缀文本被压到粘贴块的下面。
改进：行级内容替换字符级选区时，先把活动块在删除点拆成两块（或记录选区所在行列），再把粘贴块插到前缀块之后，使顺序为 [前缀行][粘贴行][后缀行]。

**P237｜`b`/`ge` 会越过空块（空行），与 vim 的「空行是词停点」及自身注释相悖**（motion.rs:230-264/307-321；类别：体验｜中）
现象：光标在非空块行首按 b，若上一块是空段落，光标直接跳过这个空行落到再上一块的词首；vim 中空行是独立的 b/e 停点。`w` 与 `e` 都正确停在空块上（**有显式检查**），唯独反向动作不对称。
原因：底层图素回退特意实现了「空块停在其起点」，但 word_backward 拿到这个落点后进入空白类循环，把空块起点当空白继续前退。
改进：word_backward 与 word_end_backward 的空白回退循环中加入与正向相同的判据：「块首且空块」时返回。

**P238｜`H`/`M`/`L` 用整篇文档的首/中/末块近似「视口顶/中/底」**（motion.rs:1036-1058；类别：体验｜中）
现象：长文档里按 H 相当于跳到文首（≈gg）、L 相当于文末（≈G）、M 跳到全文档中间块，与 vim 的「视口内第 N 行/视口中线/视口底」语义完全脱节；只有文档整体小于视口时碰巧正确。
原因：函数拿到的是全文档导航块列表（并非视口裁剪窗口），代码只做了 first/last/middle 的下标运算——基准集合错了而非精度问题。
改进：从编辑器的滚动状态取视口内可见行区间，把 H/M/L 的候选集限制在当前视口渲染的块上；可复用光标可见性判定的几何。

**P239｜空文本对象（`i(` 作用在 `()` 上）会把闭括号选中并被操作符删掉**（operate.rs:786-801；visual.rs:385-396；类别：缺陷｜低）
现象：对文本 `()` 执行 `di(`：inner 区间为空，期望无操作，实际会把 `)` 删掉留下 `(`；`di"` 作用在 `""` 上同样删掉一个引号。
原因：inner 区间为空时两端相等，随后的可视扩展焦点回退逻辑使空对象膨胀成一个字符。
改进：文本对象解析对 inner 分支判空（起≥止时返回 None）；或可视扩展检测到空区间时直接返回。

**P240｜普通模式按键序列超过 10 键被静默清空，注释声称的最长序列长度不成立**（vim.rs:971-977；类别：缺陷｜低）
现象：`"a9999d9999w`（寄存器 2 键+两个 4 位计数+操作符+动作 = 12 键）这类 vim 完全合法的序列，在第 11 键时整个 pending 被清空，命令无声丢弃；用户需要重新输入整个序列。
原因：护栏写死 10 键上限，注释断言「最长的合法序列也远到不了 10 键」，但漏算了「寄存器前缀+两个独立计数字段」的组合（两个计数都可到 9999）。
改进：护栏放宽到 14-16 键，或改为「仅当当前 pending 已不可能成为任何合法前缀时才清空」；至少修正注释并让清空时给出可视反馈。

**P241｜插入会话的 `.` 素材记录与重放不对称**（vim.rs:688-757；operate.rs:835-856；类别：缺陷｜中）
现象：插入模式里用 Delete（前删）或 ctrl-r 粘寄存器后，`.` 重放得到的文本与首次输入不一致：Delete 键被记进素材但重放时是空操作（文本不回退），ctrl-r 的寄存器内容整段丢失；含方向键移动的会话重放时所有字符堆在同一位置。
原因：记录侧把 Delete/Home/End 等一并入素材，重放侧对非 Enter/Backspace/Tab 一律吞掉；ctrl-r 的寄存器内容两边都不入素材。
改进：重放侧补齐 Delete 与方向键的等价操作，或记录侧把无法重放的键排除出素材并视为「该会话不可精确重放」（放弃 dot 避免错位重放）；ctrl-r 至少把寄存器粘贴内容折算进重放素材。

**P242｜marks 与 jumplist 记录的字节偏移不随编辑调整，跨编辑后漂移**（operate.rs:2663-2673/2751-2784；motion.rs:1074-1096；类别：体验｜中）
现象：在某行 `ma` 后回到行首插入 20 个字符再 `` `a ``，光标落在距原位置偏左 20 字节处；jumplist 里 ctrl-o 跳回的位置同样偏移。块内多字节编辑后甚至可能落在字符中间，由块的落点钳制兜底但位置语义已错。
原因：标记只在设置时写入一次，之后所有块内编辑都不调整 marks/jumplist 中落在同一块、偏移之后的条目；vim 会在每次文本修改时对受影响标记做平移。
改进：在块编辑写回的公共出口按（块 EntityId, 插入点, 长度增量）平移 marks/jumplist/last_visual 中同块的偏移；至少对插入点之后的偏移加增量、覆盖区间的标记置失效。

**P243｜`:` 命令行对未支持命令与 `:.s` 前缀静默吞掉**（command.rs:442-548；类别：体验｜低）
现象：`:5d`、`:d`、`:sort` 等未实现命令输入后无任何反馈（vim 报 E492）；`:.s/a/b`（当前行替换）因 `.` 边界解析失败后整体回退，命令体以 `.` 开头不匹配任何分支，同样无声丢弃；`:s` 无命中也静默返回（vim 报 E486）。
原因：命令分发最后一臂是空匹配；范围解析在特定分支把范围字符混进了命令体；替换的零命中与正则构建失败都是裸 return。
改进：给命令行加与 vim 一致的错误回显通道（如状态栏显示 E492: Not an editor command）；修正 `.` 边界的解析；无命中给出提示。

**P244｜显式写 `"0`/`"1` 寄存器不进对应链，读回旧内容**（operate.rs:111-148；类别：体验｜低）
现象：`"0dd` 删一行后 `"0p` 粘出的是上一次 yank 的内容而非刚删的行；`"1dd` 后 `"1p` 同样粘出编号链里的旧条目。vim 中显式前缀写入会直接覆盖对应寄存器。
原因：write 只处理 unnamed 与具名 a-z/A-Z，显式传入编号时除 unnamed 外什么都没写，而 read('0') 读的是 yank 寄存器，读写不对称。
改进：write 对 '0' 直接覆盖 yank 寄存器、对 '1'-'9' 按下标写入编号链（或至少文档化该限制），保持 read/write 对同一名字空间对称。

**P245｜搜索导航每次按键全文重扫；块状可视区间解析 O(n²)；`9999.` 无步数护栏**（command.rs:262-270；visual.rs:207-210；vim.rs:1367-1389；类别：性能｜低）
现象：长文档中连续按 n，每次都触发一轮完整的工作区搜索调度（可以先查命中表是否已对应当前版本）；对大代码块做块状可视操作时，行号统计对每个候选行都从头数换行，整体 O(行数²)；`9999.` 会同步串行重放 9999 次完整插入会话，期间 UI 完全阻塞且没有像宏那样的步数熔断。
原因：搜索应用函数无条件重设查询并调度；行号统计用切片+逐次计数；重放循环只在宏路径有护栏。
改进：n/N 增加「查询与缓冲区代次未变则直接用现有命中表导航」的短路；行号统计改为一次性构建行起点表后二分；重放增加与宏一致的步数护栏并对超大 count 分批让出主线程。

**P246｜`:s` 的 `c`（确认）/`I` 标志与 `~` 重复替换被静默忽略**（command.rs:582-599；类别：体验｜低）
现象：`:s/a/b/c` 期望逐处确认，实际直接全部（或首个）替换完；`:s/a/b/I` 的大小写敏感标志、`:s//~/`（复用上次替换串）都不支持且无提示。用户以为带了确认标志，结果文本被一次性改掉。
原因：只取 g/i 两个标志，其余字符不校验静默丢弃；`~` 特殊替换串被当普通字符输出。
改进：至少对 c 标志给出「暂不支持逐处确认」的显式提示（与 P243 的错误回显通道共用），未知标志字符回显警告；`~` 支持前按不认识的替换语法拒绝而不是当字面量写入正文。
## 修复优先级建议

- **第一批（数据正确性/崩溃面，13 条）**：P1 P2 P119 P120 P169 P151 P215 P86 P63 P166 P187 P188 P203。这一批要么丢用户数据（P2/P119/P120/P169），要么崩溃/卡死（P151/P215/P86/P203），要么大文档核心操作秒级冻结（P63/P166/P187/P188）。
- **第二批（高频路径性能，约 20 条）**：P23 P24 P13 P21 P22 P73 P94 P101 P132 P145 P172 P175 P176 P194 P195 P50 P53 P68 P195 等；共同主题是「按键/点击/拖动热路径上的 O(文档) 操作」。
- **第三批（体验缺口，约 30 条）**：P41 P91 P92 P100 P87 P108 P228 P219 P227 P194 P198 P207 等。
- **第四批（低危/规约清理）**：其余 expect/吞错/死代码/注释重复类。

修复流程按仓库约定执行：每条先由修复 subagent 独立复核真伪（读代码构造触发路径，判定「不是问题」的条目须写明理由并跳过），确认后按 velora-tdd 先写会红的测试再动实现；收尾跑 `cargo build`（零警告）与 `cargo test --bin velora` 全量门禁。
