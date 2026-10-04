# FIXPLAN v2：s2 最新树（195fac9）上的 review 修复对账

> **策略**（用户拍板）：不 rebase——直接在 s2 最新提交树上重新落地全部 review 修复，分支 `s2-review-fixes-v2` 必须能 fast-forward 到 `s2-buffer-source-of-truth`。已验证：`git merge-base --is-ancestor s2-buffer-source-of-truth HEAD` 通过，11 个修复提交线性叠在 s2 tip（195fac9）之上。
> 基线：新树 1284 通过 / 0 失败 → 修复后 **1295 通过 / 0 失败**（+11 个新测试）。

## v4 审查发现（62 新提交的三路主题审查）

| 项 | 状态 | 提交 |
|---|---|---|
| N-高1 表格格子写回编辑后坐标平移 | ✅ | `da549dd` |
| N-高2 账本作废只兑现一个写回入口 | ✅（block_source 按族重记 + 落笔前上下文核对兜底过期族） | `70b3dfc` |
| N-中1 行元数据不随就地 kind 变化刷新 | ✅ | `455ade5` |
| N-中2 围栏账硬编码顶格 | ✅（重记复用旧账） | `70b3dfc` |
| N-中3 引用逐行账只覆盖单段标题 | ◐ 部分缓解（过期账由 A1 守卫安全化；扩展账本属上游工作） | `70b3dfc` |
| N-中4 refresh_source_line_starts 平方级 + 幽灵 Fenwick 注释 | ✅（改 lines_and_line_starts 批量；注释随重写移除） | `455ade5` |
| N-低 dirty 不含删除段/clamp 抹平/Anchor 死机制/callout 头 | ◐ 记录在 review v4，未在本轮改（低危+上游领域） | — |

## v3 登记册在新树上的重落地

| 项 | 处置 | 提交 |
|---|---|---|
| A1 落笔上下文核对 | ✅ 适配移植（守卫改行内局部 + 代码块 display 口径），兼作过期账兜底 | `70b3dfc` |
| A2 CRLF 粘贴损坏 | ✅ cherry-pick `ea3d405` → `0fd8c39`（tree_files.rs 冲突手工解：双方测试都保留） | `0fd8c39` |
| A3 IME 多次组合丢撤销 | ✅ cherry-pick `5305aaa` → `cdf0140`（mod.rs 守卫冲突保新树改进版） | `cdf0140` |
| A4 幽灵锚守卫 | ✅ 随批次一（适配 `source_span_of`/`find_block_location`） | `70b3dfc` |
| B1 探针死代码 | ✅ cherry-pick `2780b7a` → `747a94d`（适配新 attach_root_spans 签名 `4c79682`） | `747a94d` |
| B2/B3 保存字节+重载形状 | ✅ cherry-pick `de5ea2f` → `666aad2`（无冲突） | `666aad2` |
| B4/B5 | ✅ v3 已证伪（无需修） | — |
| B6 容器表格根块写回 | ✅ 适配移植（root_ancestor_of） | `a3f3726` |
| B7 尾段落零宽 span | ✅ 适配移植（走 set_source_span） | `930a405` |
| B8 replace_all 虚报 | ✅ 适配移植（同一实现整体替换） | `a3f3726` |
| B9 注册表时序 | ✅ 适配移植（三处挪到落笔后） | `a3f3726` |
| C1 SeparatorMarker | ✅ 重新实现（state.rs/text_ops.rs/import.rs） | `7ca26b4` |
| C2 大纲 Setext/front matter | ✅ 适配移植（新 outline_headings 核心上） | `930a405` |
| C3 早退收组 | ✅ | `930a405` |
| C4 孤儿图片 | ✅ 3way 应用干净 | `930a405` |
| C5 另存版本号 | ✅ 3way 应用干净 | `930a405` |
| C6 cell stale 守卫 | ✅ | `a3f3726` |
| D1 chunk 合并 | ⏸ 被上游取代程度待评估：新树每块已有行号表与 O(1) byte_len，chunk 增生仍存在但消费方已批量——**待上游行号表稳定后重估** | — |
| D2 大纲 revision 短路 | ✅ 被上游取代（增量大纲+dirty region），不再需要 | — |
| D3 行首偏移表 | ✅ 被上游取代（每块行号表） | — |
| D5 skip 标志收紧 | ✅ | `930a405` |
| D6 TableSourceView | ⏸ 维持跳过 | — |
| D7/D8/D9 | ✅ 上游已做（D7 撤销注释/架构文档已更新；D8/D9 随新机制消失或已注释） | — |
| E1-E9 测试 | ✅ 随各提交重新落地（+11） | 各提交 |

## fast-forward 验证

```
git merge-base --is-ancestor s2-buffer-source-of-truth HEAD  # 通过
git rev-list --count s2-buffer-source-of-truth..HEAD          # 11
```
合并操作：`git checkout s2-buffer-source-of-truth && git merge --ff-only s2-review-fixes-v2`。

## 未解决与去向

1. **N-中3 引用逐行账扩展**（带子块/嵌套引用的写侧账）：过期账已由 A1 守卫安全化（退整块写回，最多洗写法不坏字节）；账本扩展属上游账本体系，建议上游继续。
2. **D1 chunk 合并**：上游 62 提交重构了缓冲区消费方（行号表/批量换算/byte_len O(1)），chunk 增生的消费面已大幅缩小；等上游缓冲区形态稳定后重估是否还需要合并。
3. **N-低系列**（dirty 删除段/clamp/Anchor 死机制/callout 头）：低危+属上游账本体系，已在 review v4 记录。
