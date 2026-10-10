---
name: velora-code-review
description: 对 Velora(GPUI / Rust 桌面 Markdown 编辑器)的某个固定点做缺陷导向的全量或增量代码评审。四个维度按优先级并行:①仓库既有不变量与 GPUI 契约、②正确性/边界/并发、③安全、④性能与效率。用 git worktree 隔离该 tag,按模块切给并行只读子代理,主线程验证去噪后聚合成 docs/reviews/ 下的一份 Markdown 报告。只评审、不改代码。Use when 用户说"review 一下这个 tag/版本""帮我看看某个版本的全部代码""发版前做个质量关口""对比两个版本审一遍"。Triggers include "code review", "评审", "审一遍", "看看这个版本/tag 的代码"。
---

# Velora 代码评审(缺陷导向)

输入:一个固定点(tag / commit / 分支)与模式(全量树或两 tag 之间的 diff)。产出:`docs/reviews/` 下一份分维度、分级、带验证状态的 Markdown 报告。**评审只读,不改代码**;修复交由 `velora-gpui-bugfix`。

**缺陷导向**:风格、命名、格式交给 formatter 与 CI,人工评审只覆盖机器查不出的问题——逻辑错误、边界、并发竞态、资源泄漏、安全、性能悬崖,以及本仓库高发的「同一处口径被抄成第二份」。不导致崩溃、错误结果、数据丢失、安全事故或性能劣化的,不进报告。

**分维度**:同一段代码可能正确性无碍却引入重入 panic,或并发无碍却存在导出 XSS。维度分开报告、不合并排序,避免相互掩盖。

## 0 固定点与隔离

固定点由用户给定(`vX.Y.Z` / commit / 分支 / `HEAD~5`);未指定则询问。

```bash
git rev-parse <固定点>                       # 坏引用在此即失败
git log <固定点> -1 --format='%H %s'
```

**不在当前工作树 checkout 该 tag**:工作树可能有未提交改动,`git checkout <tag>` 会破坏它们。改用隔离 worktree,置于工作区内 `.review/worktrees/<tag>`;放在工作区外会使子代理的 Read/Grep 越界并触发权限中断。

```bash
git worktree add --detach .review/worktrees/<tag> <固定点>
```

- 评审一律读 `.review/worktrees/<tag>/` 下的树,不读主工作树 `src/`(可能已被改动)。
- `.review/` 在 `git status` 中为未跟踪项,不得 `git add`;如需长期忽略,单独一笔在 `.gitignore` 补 `.review/`。
- 结束清理:`git worktree remove .review/worktrees/<tag> && rmdir -p .review/worktrees 2>/dev/null`。

**不跑 `cargo build` 或全量 `cargo test`**:评审只读,按 AGENTS.md §4 不涉门禁,且 `cargo build` 为分钟级。仅当某条发现取决于能否编译或某测试红绿时,跑单条 `cargo test --bin velora <名字>`。

**模式**:
- **Diff**(存在上一个 release tag 时默认):审 `git diff <prev>..<tag>`,`prev` 取 `git describe --tags --abbrev=0 --exclude='*-beta*' <tag>^`。只看增量,读足上下文。
- **全量**(用户要求「全部代码」或无上一个 tag):审 `<tag>` 处整棵树。约 152k 行 / 273 文件,须先做风险排序、分块、并行子代理。

**报告写入主工作树**,而非 detached worktree(后者随 `git worktree remove` 一并删除)。`docs/reviews/` 需先 `mkdir -p`。路径:全量 `docs/reviews/vX.Y.Z.md`;diff `docs/reviews/vA.B.C..vX.Y.Z.md`。

## 1 评审计划:按模块切给并行子代理

全量模式按 `src/` 布局分块,每块 2k–5k 行相关代码,一块一子代理。`src/editor` 约 92k 行,须再拆。切分如下(行数以实际为准):

| 块 | 路径 | 本块专属高发缺陷 |
|---|---|---|
| 文档模型/解析 | `src/editor/{buffer,document,tree}` | 编码/BOM/CRLF 判据是否只有一处;Markdown 可转义集、标题 slug 有无第二份 |
| 选区 | `src/editor/selection`(含 `table.rs` `pointer.rs`) | 干净偏移与显示偏移两套坐标有无手算加减;表格两层(格子不在块树里)是否被「按可见块扫一遍」的代码漏掉 |
| 渲染管线 | `src/editor/render`(含 `paint.rs`) | prepaint 期切字符串的多字节边界;缓存键是否覆盖全部输入(文本代数/字号/字体指纹/主题代数/换行宽) |
| 工作区 | `src/editor/{workspace,workspace_index,window_state}` | 跨实体读写、渲染期写字段后的显式 notify;标签/搜索的口径单一性 |
| 编辑器其余 | `src/editor/{context_menu,events}` + 顶层 `src/editor/*.rs` | 动作四入口是否齐(键位/右键/选中工具栏/命令面板);一个动作只有一处实现 |
| 块组件 | `src/components/block` | 块级状态机、命中测试、`last_bounds` 依赖 |
| 富媒体组件 | `src/components/{latex,mermaid,markdown,actions}` | 公式/图表多行闭合、未聚焦块几何为空;行内标记输入跳位 |
| 导出 | `src/export`、`src/editor/export.rs` | 导出 HTML 的 XSS(用户内容未转义/未清洗);按行重猜块结构导致的口径漂移 |
| 配置/持久化 | `src/config` | config.toml 读写、自动保存不覆盖外部改动 |
| 网络/文件 URL | `src/net`、`src/file_url.rs` | URL scheme 校验、路径遍历、图片路径解析判据单一性 |
| 主题/i18n | `src/theme`、`src/i18n` | 主题代数缓存、文案缺失回退 |
| 应用外壳 | `src/{main,commands,app_identity,window_chrome}.rs`、`src/app_menu` | 系统原生弹窗(应一律应用内模态)、命令派发 |

**风险排序**(决定 top N 与跳过项):变更频率(`git log --since=90.days --name-only -- <path>`)、关键路径(解析/渲染/持久化/选区/撤销)、复杂度、影响半径、历史缺陷密度(`git log --grep=fix -- <path>`)。忽略 `vendor/gpui`、`target/`、lockfile、生成文件。**测试模块(`src/**/tests/`、`#[cfg(test)]`)不作缺陷审**,它们非产品路径;仅作为预期行为的证据参考(尤其 `fixtures/regressions/`、`author_report.rs`,见维度二)。

**预算**:全量默认审 top 10–14 块(覆盖上表),向用户说明跳过项及原因。

### 子代理派发

- 用 `Agent`(`Explore` 只读侦察,`general-purpose` 多步读),多个调用置于同一条消息并行发出。
- 子代理限定只读:brief 写明不改代码、不编辑文件,仅返回发现清单。
- brief 须自包含(子代理无本对话上下文):该块确切路径(`.review/worktrees/<tag>/...`)、四个维度的检查项、每条发现附证据(`文件:函数名` + 关键代码摘录 + 触发路径)与 confidence(high/medium/low)。
- 限制产出:每块至多 ~10 条发现,按严重度排序,只回结论不回文件转储,控制聚合阶段的噪音量。
- 子代理只侦察列清单、不深挖;高价值项由主线程确认(见 §3),避免误报直接进报告。

## 2 四个维度(按优先级)

### 维度一 · 仓库既有不变量与 GPUI 契约(机器难查,信号最高)

先读 `docs/architecture/overview.md` 的「关键不变量」与 AGENTS.md,再逐条对照代码:

- **一条语法只准有一处判据**:编码/BOM 字节表、Markdown 可转义集、图片路径解析、标题 slug、块结构判定——grep 有无第二份。本仓库多数报修表现为下游消费者出错,根因是口径两份漂移(如导出按行重猜块结构)。此项为 Velora 评审的首查项。
- **GPUI 实体重入**(高发运行时 panic):在实体自己的 `update` / `render` / `defer_in` 回调里再次 `update` / `read` 同一实体;`entity.read(cx)` 的 guard 未落地即进 `entity.update(cx, …)`。
- **借还锁 / 内层 cx**:闭包内层 `cx` 误用外层 `cx` 撞借用;`WeakEntity` 的 `read_with`/`update` 返回 `Result` 却按 infallible 用。
- **Task 生命周期**:`spawn`/`background_spawn` 返回的 `Task` 被丢弃即取消——应 `detach()` 或存字段却未存;`cx.notify()` 只排一帧,改动影响渲染的状态却未 notify。
- **产品路径的 `unwrap()`/`expect()`/裸下标**:AGENTS.md 禁止,除非写明不可能失败的理由;渲染期切字符串的多字节下标尤危。
- **静默吞错**:`let _ = …` 吞掉会失败的异步/IO;错误未送界面层;使用系统原生弹窗(应一律应用内模态,有源码审计测试守着)。
- **测试构建陷阱**:模块内 `use gpui::*` 后子模块 `#[test]` 被同名属性宏遮蔽、宏递归爆栈。
- **动作四入口**:键位、正文右键、选中工具栏、命令面板是否齐;一个动作只能有一处实现。

### 维度二 · 正确性、边界与并发

**先走 Markdown 往返保真清单**:本仓库破坏性缺陷的主要来源(单次评审曾出 15 条)。判据:同一段 Markdown 在三个 surface——内存文档模型、UI 渲染、单文件导出——须一致,且 `import → 编辑 → serialize/export` 往返不丢文、不串味。凡「按行/按字符重新解析」处,核查是否与主解析器口径漂移(见维度一)。逐项对照:

- **编码**:UTF-16、UTF-8 BOM(BOM 后首行仍须当标题)、CRLF、硬换行(`10-line-break`)。
- **数学公式**:`$`/`$$` 行内与块级、数字括号公式 `(1,2)` 可渲染、引用块内公式导出仍是公式、公式尾随文本存活、多行闭合不吞下一段/下一公式、编辑公式保尾文本与相邻公式(`03-*` `04-*` `05-*` `07-*`)。
- **代码块**:缩进代码、围栏内再出现 ```` ``` ````(inner fence)、超长闭合行、代码与链接目标里的 `$` 和反斜杠保持字面、不被当公式/转义(`06-*` `09-*`)。
- **表格**:格内反斜杠/代码/公式往返存活、`<br>` 为换行且保持、跨格选区(`02-*` `11-*`)。
- **标题**:setext(下划线式)标题导出为标题、标题 id/slug 与 `[TOC]` 在导出单文件里可跳转(`13-*` `14-*`)。
- **转义**:可转义集判据单一,UI 渲染与导出一致(`10-escape`)。
- **图片**:本地图片各写法(相对/绝对/带空格/URL 编码)均内联且加载到真实文件(`12-images`)。
- **告示块**:具名 callout 保持类型与标题(`15-callout`)。

**验收语料**:`fixtures/regressions/*`(每条对应一个历史破坏性写法)与 `src/editor/tests/author_report.rs`(`itemNN_…` 按报告编号)。以此二者为「已知会坏的写法」清单,逐条核当前代码判据是否单一、三个 surface 是否共用同一处解析。片段单测全绿不等于整篇写法无缺陷:破坏性问题常只在整篇语料中显现。

其余通用正确性:

- **边界**:off-by-one、空块/空文本、极值、整数溢出;块首/块中/块尾;含隐藏记号(`**`、`` ` ``、`[](…)`);多字节字符(CJK/emoji)的字节与字符偏移。
- **两套坐标**:干净偏移与显示偏移的换算是否只走 `clean_to_current_*` / `clean_range_to_display_range`,有无手算加减。
- **表格两层**:表格块自身无文本元素(`last_bounds` 恒空、`clean_visible_len()` 为 0、`index_for_mouse_position()` 恒 0);任何「按可见块扫一遍」的逻辑对表格内容均失效。
- **错误路径**:异常被吞、错误分支返回不合法值、写操作中途失败留脏状态、自动保存覆盖外部改动、幂等性。
- **并发与生命周期**:并发读改写、TOCTOU、乐观更新未回滚、过期结果覆盖新状态;对象 drop / 窗口关闭后异步回调仍访问它;`AsyncApp`/`AsyncWindowContext` 跨 await 的持有。
- **缓存**:key/generation 是否覆盖全部输入,漏一个即下次「不刷新」;跨实体写字段后是否显式 notify。

### 维度三 · 安全(桌面应用范围)

- **导出/复制的 HTML**:用户 Markdown 里的 `<script>`、`on*=`、`javascript:`/`data:` URL 是否转义或清洗——单文件导出内嵌内容,是主要 XSS 面。断言导出只看 `<body>` 之后(内嵌主题 CSS 含 `.vlt-inline-math`、`[TOC]` 等字面量,对整份文件做「不该出现」断言会假红)。
- **路径**:图片/文件路径解析的穿越(`../`)、`file_url.rs` 的 scheme 校验、拖入文件的处理。
- **网络**:`src/net` 的请求是否校验来源、是否将 token/PII 写入日志或错误消息。
- **外部输入**:粘贴/拖拽的富文本与文件类型校验。

### 维度四 · 性能与效率

- **复杂度悬崖**:大文档下的 O(n²)、热路径深拷贝、循环内重复计算、每帧全量重建。
- **渲染期热点**:prepaint 内重复 shape_text、缺缓存或缓存失效逻辑错、渲染期跨实体读。
- **阻塞**:CPU 密集工作跑在前台线程(应 `background_spawn` 却用 `spawn`)。
- **参考基线**:`docs/architecture/performance.md` 与 `src/editor/tests/perf_budgets.rs`;性能断言用形状/相对关系,不用绝对墙钟(并行跑测会假红)。

## 3 验证 pass(去噪,不可省)

子代理输出含假阳性,进报告前须验证:

- 对每条 confidence 为 medium/high 的发现,主线程(或再派短验证子代理)按 `文件:函数名` 与声称的触发路径核查:路径是否可达、有无被漏看的既有守卫、口径是否其实只有一处。
- 验证不了的降级为【存疑】或丢弃;低置信度归入「未验证 / 值得一看」。
- 区分「可证明不可失败」与「运行时可能失败」:并非每个 `unwrap` / TODO 都是缺陷。

## 4 总则与报告格式

- **仓库规则优先于基线**:AGENTS.md / `docs/architecture/` 认可的写法不报;工具已把关的(编译警告、类型)不报。
- **不报**:风格、命名、文档、格式、「可以更简洁」、规范符合度。
- **分级**:【必修】导致崩溃 / 错误结果 / 数据丢失 / 安全漏洞 / 性能劣化;【建议】有更稳健写法;【存疑】需主 agent 或作者确认,不替作者猜。
- **引用位置用 `文件:函数名`**(AGENTS.md:行号会漂);tag 为固定点,需要时可附该 tag 处行号。
- **每条发现**:`[级别] 文件:函数名 — 问题 — 触发条件(具体输入/代码路径)— 预期 vs 实际 — 建议修法 — 置信度 — 验证状态`。
- **聚合**:跨块合并,按根因去重(同一处口径被多块报同一问题合成一条,列出全部受害消费者),维度内按级别再按文件排序。
- **不做跨维度总排名**:分维度即为避免相互掩盖。

报告骨架:

```markdown
# Velora 代码评审 · <tag>(或 <prev>..<tag>)

- 固定点:<commit hash> <subject>;模式:全量 / diff;评审块数 N,跳过:<列表+原因>
- 总体结论:四个维度各自统计(必修/建议/存疑 各几条)+ 各维度最严重的一条

## 维度一 · 仓库既有不变量与 GPUI 契约
## 维度二 · 正确性、边界与并发
## 维度三 · 安全
## 维度四 · 性能与效率
## 未验证 / 值得一看
```

**复审只看修复点**:确认必修项已处理、未引入新问题即通过,不从头再评。

**趋势线**:报告存于 `docs/reviews/`;下次同模式运行时与上一份 diff,得出「新引入 / 已修复 / 仍存在」的缺陷。

## 5 使用方式

- 全量:`review tag vX.Y.Z`(或「审一遍 vX.Y.Z 全部代码」)→ top-N 模块,报告 `docs/reviews/vX.Y.Z.md`。
- 增量:`review vA.B.C..vX.Y.Z` → diff 模式,报告 `docs/reviews/vA.B.C..vX.Y.Z.md`。
- 无 tag 或要求「整个仓库」:全量模式审 top-10 风险块,明确告知跳过项。
- 发版前质量关口:配合 `velora-release`,在打 tag 前对候选 commit 跑增量评审。

## 6 检查清单

- [ ] 用工作区内 worktree(`.review/worktrees/<tag>`)隔离该 tag,未碰主工作树改动,未 `git add` `.review/`
- [ ] 坏引用已在第 0 步拦截
- [ ] 全程未跑 `cargo build`/全量测试;需要时仅跑单条 `cargo test`
- [ ] 模式与 `prev` tag 判定正确;报告写入主工作树 `docs/reviews/`(非将被删的 worktree),已 `mkdir -p docs/reviews`
- [ ] 子代理均只读、brief 自包含、要求证据 + confidence、限 ~10 条产出
- [ ] 维度一已查(口径单一性 + GPUI 重入/借还锁/Task/unwrap),未止于通用四项
- [ ] 维度二走了 Markdown 往返保真清单,对照 `fixtures/regressions/*` + `author_report.rs` 核三个 surface 判据单一
- [ ] 验证 pass 已跑,假阳性降级或丢弃
- [ ] 每条发现引用 `文件:函数名`,分级、触发条件、建议修法齐全
- [ ] 报告按根因去重,四维度分开不合并排序
- [ ] 收尾清理自开的 worktree;全程未改产品代码
