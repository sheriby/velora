# s2-buffer-source-of-truth 重构审查报告（v3 终版 · 深审完成）

> **v3（2026-10-04）**：审查基线 `5320e25`。v2 之后补做了 5 路深度审查（区域重解析与撤销、读取侧迁移、保存链路、表格写回、新增测试网），覆盖 `86bfe56..5320e25` 全部 34 个提交的**新增代码**。本版是修复工作的权威输入；修复计划与执行在新 worktree `s2-review-fixes`（见其 `FIXPLAN.md`）。
>
> 修复期间该分支仍在活跃提交（期间 `9c2f162` 强调记号进模型、`5320e25` 编码全矩阵测试落地，均已在下文反映）。

---

## 0. 总评

方向与写入层继续扎实：撤销反序重放、拆块接缝、`18 处 buffer.edit 调用点全部紧跟 record_buffer_edit`、`ListMarkerStyle` 三条接入路（导入/快捷转换/拆块继承）完整、图片粘贴写回链正确、读取侧「只认缓冲区」骨架立住且闸门（每键整篇序列化=0、整篇 mapping=0、静止帧全 0）是真闸门。

深审新发现集中在四处：**①单块映射的引用前缀硬编码会直达落笔写坏 buffer（坏数据）；②CRLF 文档粘贴带 `\r` 文本会写盘 `\r\r\n` 且版本号自误判锁死保存（坏数据）；③区域重解析的探针是死代码（防线未插电）；④autosave/关标签/session watcher 仍有四个写文本的旁路口**。另有约 20 项中低问题与 9 项测试缺口，全部列于下文。

---

## 1. 问题登记册（修复权威清单）

编号规则：A=坏数据级（高），B=保真/机制级（中高），C=行为级（中低），D=性能/结构，E=测试缺口。每项给位置、问题、修复方向。

### A 组：坏数据级

**A1 非规范引用前缀漂移直达落笔**（v1 M1 升级为高）
- 位置：`source_mapping.rs:291` `wrap_source_mapping_with_quotes(&full_text, "> ", "> ")`、`:427` 一带；消费链 `selection.rs:523` `caret_source_offset` → `mapping_source_offset` → `mod.rs:991` `buffer.edit(at..at)`。
- 问题：映射按规范前缀 `"> "`（2 字节）记账，`>引用二`（1 字节）、`>   x`（4 字节）下每行累积 ±1~2 字节偏移。落笔守卫（`mod.rs:984-989`）只查 span 包含与字符边界，**区间内错位直接放行** → 打字插错位，buffer 变 `>引甲用二` 而模型是 `甲引用二`，保存即落盘错字。`selection.rs:554` 的 `offset==0` 短路返回的是前缀之前的位置，同根因。搜索高亮/跳转/跨块端点同源波及。
- 修复：映射构建时从 buffer 原文实测每行真实前缀（数行首 `>` 及其后空格）；落笔前校验上下文字节（`buffer.slice(at-1..at)` 应为前缀尾字符）不一致退整块写回。
- 验收：`> 引用一\n>引用二\n>   引用三\n` 在第 2 行中间打字 → buffer/磁盘逐字节正确（现有测试只打第 1 行/行尾，盖不住）。

**A2 CRLF 文档粘贴 `\r\n` 文本 → 落盘 `\r\r\n` + 版本号锁死**
- 位置：`file_shape.rs:46-55` `encode` 只做 `\n→\r\n` 不先吃已有 `\r`；注入入口 `keys.rs:247`（跨块粘贴传原始剪贴板文本）与 `keys.rs:266`（raw 编辑分支直接 `replace_text_in_range`，多行分支 :277 规范化了、raw 分支没有）。
- 链条：buffer 出现 `\r` → encode 产出 `\r\r\n`（字节损坏、重开多空行）；且 `file_version = file_content_version(buffer.text())` 与磁盘字节归一哈希不一致（`a\r\n\r\nb`→`a\n\nb` vs 磁盘 `a\r\r\n…`→`a\n\n\nb`）→ 下次保存 `verify_file_version` 全线误报「外部修改」，文件锁死。单行粘贴恰好哈希一致，**两行起必炸**。
- 修复：`encode` 先把文本内 `\r\n`/`\r` 归一成 `\n` 再升格；两个粘贴入口照 :277 的先例规范化。
- 验收：CRLF 代码文档粘贴两行 CRLF 文本 → 保存 → 盘上恰为 `\r\n`；再保存不报外部修改。

**A3 IME 多次组合更新丢撤销增量**（v1 H1，仍未修）
- 位置：`input.rs:215-221`、`runtime/code.rs:107-115`（组合中更新不开组）；`history.rs:100-107`（无组静默丢）；`history.rs:167-171`（finalize 只并紧邻一条）。
- 复现：`n→ni→nih` 提交 `你好`，undo 一次 → `笔记内容ih`。现有 IME 测试全部单次更新（5 处 `replace_and_mark_text_in_range` 调用核过），盖不住。
- 修复三层：组合中更新也 `prepare(ImeComposition)`；finalize 合并连续 ImeComposition run；`record_buffer_edit` 无组时 `debug_assert!`（**先修前者再开后者**）。
- 验收：多次组合更新 undo 一次退净 + 保存逐字节；中途取消 undo 退净。

**A4 normalize 把已摘除块当写回锚点（潜伏坏数据）**
- 位置：`history.rs:428-436`（`root_ancestor_of(...).or_else(|| Some(anchor.clone()))`）；调用点 `block_event.rs:385`（MergeIntoPrev）、`:1000`（RequestDelete）、`:590`（PasteMultiline）传入的 anchor 已被 `remove_block_by_id_raw` 摘除。
- 两种坏结局：缓冲区未变时 `slice==序列化` → 返回 true 跳过 resync → **合并内容从未落盘**；缓冲区已变时 `write_minimal_diff` 把已删块的序列化**写回旧区间（删除被复活）**。当前靠「被删的是无 span 子块/容器不可删」两条外围约定侥幸不爆。
- 修复：normalize 改收树上现存的根块（RequestDelete 已有 `region_anchor`）；`write_back_block_source` 加「块必须在树内」守卫（`find_block_location` 探测）。

### B 组：保真/机制级

**B1 区域重解析探针是死代码**（v2 N1 确认为真）
- 位置：`import.rs:86-113`。lookahead=0 时窗口恰为 region 行数，`spans.last().end ≤ lines.len()` 恒成立 → `parsed_lines > region_lines` 永假 → **第一次迭代必返回，lookahead=1/2 不可达**。两方向结构分歧（窗口外吞行、全篇合并窗口内外行）都检测不到。四种分歧形状：段落懒续行合并、Setext 相邻、未闭合围栏、未闭合 math/fenced div。当前唯一调用方（引用）靠「序列化每行带 `"> "`、导入器引用不懒续」自闭合而安全——**防线看着在、没插电**。
- 修复：固定带 2 行探针的窗口跑一次，`parsed_lines ≤ region_lines` 才接受；删死迭代与无人使用的返回载荷。补 None/退整篇路径测试（现在零覆盖，编辑序列闸门也不计整篇重解析）。

**B2 写盘旁路仍写文本**（v1 H3 残留的完整清单）
- `persistence.rs:199`（autosave 写 `recovery.markdown`）、`:358`（关闭批量保存写 `document.markdown`）、`tabs.rs:594`（关标签 `fs::write` LF 文本、非原子、无 verify）、`session_watcher.rs:158`（同款）。
- 后果：CRLF/GB18030 文档经任一路径即被洗成 LF/UTF-8。
- 修复：统一 `bytes + write_atomic`；后台标签无 shape 的退化为文本写入并注释（完整修法=tab 携带 shape，独立工作项）。

**B3 外部重载不重挂 FileShape**
- 位置：`tabs.rs:76/78` 只 `read_document_string` 扔掉 raw 字节 → `replace_document_content → reset_buffer_for_text` 后无人 `attach_file_origin` → 重载后编辑保存整文件转码。
- 修复：reload 改 `load_document`（raw+shape）并 attach。对照：拖拽打开 `file_drop.rs:195`、工作区打开 `tabs.rs:391` 都有 attach，唯独这条漏。

**B4 段+空段+段：删空段后树/buffer 结构分歧**（v2 N2 确认）
- `RequestDelete`（`block_event.rs:936`）删空段只删块收分隔，接缝两侧同类段落根不合并 → buffer 相邻行、树仍两根；下次整篇兜底把空行写回（**字节回魂**）。现有测试夹具是表格+空+段（不同类不合并），盖不住。
- 修复：删空段后接缝两侧同类段落 → 树内合并 + 区域重投影（或退整篇），配成对 golden。

**B5 CRLF 表格列操作降级且洗行尾**
- `table_edit.rs` 加/删/移列的 `wrapped` 检查（:299/:371/:491 `pipes.last() == text.len()-1`）在行尾带 `\r` 时必失败 → 退 `write_back_block_source` 整表重投影，`block_markdown_source` 以 `\n` join → 行尾 `\r` 被洗、列宽填充丢失。`realigned_delimiter_cell`（:1448-1477）对齐末列把 `\r` 计入宽度、重建时吃掉。行级四操作（line_range 制）CRLF 安全。
- 修复：列操作行文本剥 `\r` 再量、写回时保 `\r`；对齐重建排除 `\r`。补 CRLF 列操作正向+undo 测试（现有 CRLF 删列测试只断言撤销复原，把降级掩盖了）。

**B6 容器内表格结构操作退整篇**
- `write_back_table_structure_edit`（table_edit.rs:591-601）把表块（非根祖先）传 `write_back_block_source` → 无 span → 整篇重投影；:583-590 注释承诺的「只在自己区间内」对嵌套表不成立。修复：传 `root_ancestor_of`。

**B7 结构粘贴尾部空段落无 span**（v1 M3）
- `structural.rs:304`、`paste.rs:60`、`table_nav.rs:247`：只插树不写 buffer 不挂 span → 首次击键必退整篇（触发闸门、全文被洗）。修复：插入时写分隔字节 + 挂零宽 span。验收：结构粘贴到文末 → 新段落打一字 → 保存除新段落外逐字节不变。

**B8 replace_all 每命中付两次整篇映射 + 非规范前缀下替换变插入**
- `find_replace.rs:388-420` 每个命中走 `apply_selection_snapshot_in_current_mode` → `selection.rs:341` 整篇 build，失败后 `history.rs:253` 又一次整篇 build；`history.rs:254-279` 的 exact 往返校验在非规范前缀块必失败 → 退 caret 兜底把选区塌成光标 → **替换变插入但 `replaced` 照常 +1**。删快照（99d4aed）后不再要求先落地，触发面变大。
- 修复：同块命中走单块映射；exact 失败时该命中保守跳过并计数，不伪成功。

**B9 rebuild_image_runtimes 落笔前调用**
- `quotes.rs:243`、`structural.rs:163`、`block_event.rs:434`、`:573`：结构路径在写回**前**刷新图片/链接注册表，读到的 buffer 还是旧文本 → 注册表晚一拍。f6298ea 只修了 Changed 与 RequestNewline 两路。修复：统一挪到 `write_back_structural_change` 之后。

### C 组：行为级（中低）

- **C1** `***`/`___` Separator 被洗成 `---`：`state.rs:686` 硬编码。照 `ListMarkerStyle`/`emphasis_marker`（`2010db9`/`9c2f162`）模式：解析期记 marker 进模型。
- **C2** 大纲 Setext/front matter 与导入器不一致：`input_handler.rs:374/426` 自写判定（无最小长度、任意缩进、不跳 front matter → YAML 笔记出幻影标题）。复用 `parse_setext_underline`（state.rs:405）+ 把 front matter 判定抽成 import.rs 公共函数。
- **C3** prepare 后早退漏 finalize → 空撤销组把下次动作并进步（选区快照也错）：`block_event.rs:646/676/690`、`paste.rs:359`。早退路径补 finalize。
- **C4** 图片粘贴失败留孤儿文件：`paste.rs:418` 先写文件、`:426/:359` 失败路径不回滚。落文本失败时删除已写资源。
- **C5** 另存面板期间 buffer 变化 → bytes（:584 面板前取）与版本号（:499 写后取）错位。写入时一致化（同一时刻取）。
- **C6** `write_back_table_cell_source`（table_edit.rs:45-54）无 stale 区间守卫 → 过期 span 撞 `buffer.edit` assert panic。补 `write_back_block_source` 同款守卫。
- **C7** 删空段 blank-run 不对称（`mod.rs:1179-1181` 两侧多空行时留一个）/ `write_back_blank_run` 换入空段无 span：自愈型偏差，测试钉住行为 + 注释。

### D 组：性能/结构

- **D1** chunk 无限增生：`buffer.rs` 无合并（方案 §3 明文）。修：edit 后局部合并（锚点是绝对偏移，合并零影响），配「8MiB 中段连打 2000 字 chunk 有界」测试。
- **D2** revision 计数器：`TextBuffer` 加 `revision: u64`；字数（`status_bar.rs:84` 每次静默窗口到点 `buffer.text()` 整篇 clone）、大纲（`tree_sync.rs:458` 每帧 `matches_text` O(n) memcmp、`:454` `outline_source` 是换了名字的第二份全文副本）按 revision 键控缓存。
- **D3** `attach_root_spans` O(roots×chunks)：`mod.rs:931-934`。循环外一次预扫行首偏移表。
- **D4** perf 夹具静默跳过：`perf_budgets.rs:62/251` 夹具 gitignore、缺失 `return`。程序化生成或改 `panic!`。
- **D5** `skip_next_resync` 泄漏：`history.rs:459-461` normalize 尾部无条件置 true，会把调用方紧随的 mark_dirty flush 吃掉（当前靠运气不可达）。仅在写回/重投影成功时置；长期改一次性 token。
- **D6** 表格 9 函数前奏（span+table+量行+行数守卫）复制 7 份、`unescaped_pipe_offsets+wrapped` 3 份：提 `TableSourceView` 收编。
- **D7** 撤销整树重建（`history.rs:376`）：方案 §5.4 阶段 2 工作项，注释标记，本轮不实现。
- **D8** Source 模式选区快照锚 `first_root`（`history.rs:18-29`）：>512 行源码文件行列号/撤销选区锚在第一块。低，注释+阶段 2。
- **D9** `push_table_mappings_in_root` 表头启发（source_mapping.rs:441-458）同根围栏内同名表头可能误配：低，接受并注释。

### E 组：测试缺口（修复验收即清单）

1. IME 多次组合更新（A3）；2. 退格删除在 `__`/`>引用`/`1)` 的字节保真（`9c2f162` 后 `__` 应已保，需钉住）；3. reproject None/退整篇路径（B1）；4. 段+空段+段（B4）；5. autosave/关闭流 CRLF/GB18030 形状（B2；`5320e25` 只补了手动保存矩阵）；6. 表格：加行与调对齐 undo、容器内操作、首末行/单列边界、CRLF 列操作（B5/B6）；7. 引用行中打字（A1）；8. perf 夹具（D4）；9. round_trip 弱断言收口（`round_trip_fidelity.rs:386` starts_with → 整文件；`save_autosave_ime.rs:554/603` 缺 `buffer_text == FIXTURE` 对照）。

---

## 2. 已修复确认（对 v1/v2 的核验闭环）

`9c2f162` 强调记号进模型（H4 的 `__` 部分闭环——整块落笔不再洗强调写法；残留 `>引用` 前缀与 `~~~~` 围栏两种写法）；`5320e25` 保存路径编码/EOL 全矩阵测试。加上 v2 已确认的：H2（watcher 伪重载）、H5（标签快照）、M2（RequestNewline 顺序）、P3（每键映射单块化）、P2（last_stable 删除）、兜底末行换行、拆块接缝、matches_text panic、整篇序列化闸门空白名单。

## 3. 明确干净（深审确认，不要动）

表格 5 个多步函数的坐标策略（删列预量从后往前、移列每行重取、移行防御性重测）全部干净；表格撤销同组一次回退干净；转义 `\|` 几何精确；表头下移 0↔2 对换语义一致；窗口解析 ChunkCursor 三参数与全篇同源；reproject 后选区恢复/折叠保留正确；退整篇时已落之笔不被洗（两条路都以 buffer 为输入）；`replay_history_group`/redo 往返/选区快照语义正确；图片粘贴写回三布局正确；`RequestNewline source_already_mutated` 无漏组；`file_content_version` 与 `file_bytes` 在 CRLF/GB18030/BOM 自洽（除 A2 注入场景）；`replace_document_content` 清场清单完整（除 skip_next_resync 小残留）；外部修改比较 CRLF 归一后按版本号自洽；单块与全篇映射同源同口径（除共有 A1 漂移）；大纲/TOC/链接面板已全部走 buffer 行坐标。

## 4. 修复执行

全部问题在 worktree `../velora-review-fixes`（分支 `s2-review-fixes`，基于 `5320e25`）按 `FIXPLAN.md` 执行；完成状态以该分支的提交与全量测试为准。

---

# v4 增量审查：5320e25..195fac9（62 个新提交，2026-10-04）

> **范围**：另一会话在评审期间继续推进的 62 个提交（+8687/-1152，63 文件）。三路主题审查：①解析期记号账本（~20 提交）、②源码区间表 + 缓冲区行号/字节报告、③大纲/行计划 perf + 格式化命令 + 杂项。
> **修复分支**：本节发现与我们旧登记册的修复，全部在新分支 `s2-review-fixes-v2`（worktree `../velora-s2-fixes`，基于 `195fac9`）上重新落地——**不 rebase 旧分支**，保证该分支可 fast-forward 到 s2。
> 基线：新树全量 1284 通过 / 0 失败。

## v4.0 这批提交做了什么

1. **解析期记号账本**（§4 路线的落地）：ATX/段落/列表记号/围栏三行/缩进代码/Setext/标注头/引用每行——记号宽度在解析期记进 `source_line_prefixes`/`source_fence_lines`，写侧与读侧用同一份账（不变式 23：重新分配区间就作废）。
2. **源码区间搬进块树一张表**（`116ba16`）：slotmap EntityId 键 + 单一真相，`record.source_span` 字段物理删除，平移一趟 O(#条目)。
3. **缓冲区重构**（`6d24e81`/`99a7f93`/`143533d`）：字节变更报告（dirty region）、每块行号表、byte_len O(1)、批量行号换算。
4. **大纲/行计划 perf**（`bc47d4f`/`c250d72`/`195fac9` 等）：大纲按根块摘要增量重建（删掉了每帧 memcmp 与全文副本）、围栏跨接缝分档、行计划行元数据快照。
5. **显式「格式化文档」命令**（`6b63a31`，方案阶段 3 项）+ 缩进代码/引用落位等杂项修复。

**对旧登记册的影响**：v3 的 §4 建议路线（fragment 级 source 区间）已被这批工作部分实现；我们旧分支上「映射实测真实前缀」等遗留项随新账本机制重估。

## v4.1 新发现（高危）

| # | 位置 | 问题 |
|---|---|---|
| N-高1 | `table_edit.rs:66` | 表格格子写回用 `applied.new_range.end`（**编辑后**坐标）当 `shift_root_spans_after` 的平移起点——契约要求编辑前坐标。往最后一个格子粘贴超过「格子起点到表尾」的内容时，后续根块区间整批漏移（过期区间仍在界内，越界守卫拦不住），下一次写回落错字节。六处手工记账唯此一处方向反了。**一行修复**：改传 `old_span.end`。 |
| N-高2 | `mod.rs:1157`（write_back_block_source）、`mod.rs:1614`（reattach_root_spans） | 不变式 23「重新分配区间就作废账本」**只兑现了 `write_back_root_region` 一个入口**：逐块写回与整篇 resync 同样重写字节、同样重挂区间，却不动作废/重记 `source_line_prefixes`。序列化仍会规范化的族（带子块引用、根级缩进块）在 resync/逐块写回一次后账即过期，下一次落笔漂移——旧病灶以「过期数据」形态从另外两个写回入口绕回来。 |

## v4.2 新发现（中低）

| # | 位置 | 问题 |
|---|---|---|
| N-中1 | `block_event.rs:85` + `tree.rs:234` | 行计划的行元数据不随「块种类就地变化」刷新：段首打 `# `/`- `/`> `（同实体换 kind，可见列表不变），行距停在段落档直到下次结构变化。 |
| N-中2 | `mod.rs:1213`（written_line_ledger） | 围栏族硬编码「顶格 (0,0)+零前缀」重记，与刚按旧账拼出的落笔文本矛盾：缩进围栏被结构写回扫一次后，下一次落笔洗掉用户缩进。应复用旧账（缩进族分支就是对的）。 |
| N-中3 | `tree.rs:1194` | 引用逐行账只覆盖「单段标题引用」：带子块（账只记标题行，body 行数 > widths）或嵌套 `>>` 仍整块退 `> {line}` 统一洗。测试夹具恰是无子块形状，掩盖缺口。 |
| N-中4 | `window_state.rs:498` | `refresh_source_line_starts` 每键逐根调 O(chunks) 的 `line_of` → 平方级；同树 `lines_and_line_starts` 已解同一形状。另有两处注释描述不存在的 Fenwick。 |
| N-低×6 | buffer.rs:400（dirty 不含删除段且不平移，`region_touches` 严格不等号会漏）、tree.rs:160（clamp 抹平错账信号）、buffer.rs:86（Anchor 成死机制、头注释误导）、source_mapping.rs:1451（callout 头空格数未记账）、source_mapping.rs:461（注释与事实相反）、测试盲区（resync/undo 后账本零覆盖；dirty/take_dirty_region 无直接单测） | — |

**干净清单（重点核过）**：区间表重构核心干净（EntityId 带代次、维护点齐全、编译期清除两套真相）；byte_len O(1) 与每块行号表的维护点齐全且有差分测试；解析期账的绝对口径与容器链（`origins` 逐级携带）逐位正确，行数不齐整账作废（正确的保守取向）；大纲增量重建失效键闭合（撤销/重载/区域重解析全走重解析或双保险）；围栏分档边界全对；格式化命令闭环正确且打开/保存无残留规范化；过期账最坏是行内错位被下一次写回自愈，不 panic 不越界。

## v4.3 旧登记册修复的移植处置（新分支 `s2-review-fixes-v2`）

| 处置 | 项 | 说明 |
|---|---|---|
| 直接移植（他们没修） | A2 encode 归一、A3 IME 记账（input.rs 原样未动）、A4 幽灵锚守卫（适配 `source_span_of`）、B1 探针死循环（原样在）、B2/B3 保存字节与重载形状、C2/C3/C4/C5、D5 | 新分支上重新提交 |
| 适配移植 | A1 上下文守卫（正好补 N-高2 过期账的裸奔路径）、B6+C6（并入 `grow_root_span_after_edit` API + 顺手修 N-高1）、B7（适配区间表）、B8、C1、D1 chunk 合并（适配每块行号表） | 语义不变、机制适配 |
| 淘汰（被新机制取代） | D2 大纲 revision 短路、D3 行首偏移表 | 增量大纲/行号表从根上取代（已核实 matches_text 每帧比较与全文副本移除） |
| 新增 | N-高1/高2/中1-4 及低项修复 | 本节发现，随移植批次落地 |

## v4.4 状态

- 新 worktree `../velora-s2-fixes`（分支 `s2-review-fixes-v2` @ `195fac9`），与 s2 分支构造性 fast-forward（merge-base = s2 tip）。
- 基线 1284 通过 / 0 失败。
- **修复完成（12 提交，`da549dd..9517d46`）**：N-高1/高2、A2/A3/A4、B1/B2/B3/B6/B7/B8/B9、C1-C6、D5 全部在新树重新落地；N-中1/中4（行元数据刷新、行号批量换算）一并修复；D2/D3 被上游新机制取代（已核实，无需移植）；逐项对账见该分支 `FIXPLAN.md`。
- **fast-forward 已验证**：`git merge-base --is-ancestor s2-buffer-source-of-truth HEAD` 通过；合并操作 = `git checkout s2-buffer-source-of-truth && git merge --ff-only s2-review-fixes-v2`。
- **终验**：新分支全量 **1295 通过 / 0 失败**（基线 1284 + 新增 11 测试）。
- **未解决去向**：N-中3 引用逐行账扩展（过期账已安全化）、D1 chunk 合并重估（上游消费方已批量化的缓冲区稳定后）、N-低系列（记录在案）。
