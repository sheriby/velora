# Velora 搜索引擎方案：接入 ripgrep 库（grep-searcher / grep-regex）

状态：**待签字**。签字前不改业务代码。
基线：`main` @ `e8c8cf3` v0.2.0，`cargo test` = **1298 passed / 0 failed / 6 ignored**（2026-10-05 实测）。
本文所有数字来自 `/tmp/rgprobe`（release 构建，arm64 darwin，10 MiB 中英混合夹具：标题/表格/代码围栏/front matter/中文段落，17.5–19 万行）。探针脚本随本方案保留，阶段 0 会把其中的对照逻辑搬进仓库测试。

## 0. 结论速览

用户的诉求是"搜索居然没用 ripgrep"。诊断下来，这句话里包着三个独立问题，本方案分别回答：

1. **匹配层是手写的**。`search_backend.rs:96` 的 `case_insensitive_ranges` 自己实现大小写不敏感匹配，非 ASCII 分支（`:121-153`）给**每一行**分配一个 `Vec<(usize, char)>` 再做 O(行长²) 的逐字符折叠。实测 10 MiB 文档一次全篇扫描中文查询 **54.1 ms**，ripgrep 同任务 **2.1 ms**。
2. **跳转是 O(文档) 的**。`search_backend.rs:508` 的 `find_document_match_from` 每次按 F3 都把整篇重扫、把所有区间塞进一个 `Vec`、再线性找下一个；无后续命中要回绕时等于**两次全扫**。实测 10 MiB、20 个命中，单次跳转中文查询 **61.0 ms**；ripgrep 提前停止 **0.006 ms**。
3. **同一个查询被四条路径各算一遍**：结果列表 `search_document_source`、跳转 `find_document_match_from`、高亮 `tree_sync.rs:11 sync_document_search_highlights`、替换 `find_replace.rs:393 replace_all_document_matches`。四份实现、三套行/偏移口径，靠巧合保持一致——这是跳转类 bug 反复出现的结构性根因。

方案：引入 `grep-searcher` + `grep-regex` + `grep-matcher`（ripgrep 的本体库，不是外部 rg 进程），把手写匹配层换成它；把四条路径收敛成**一份按缓冲区版本缓存的命中表**；正则模式支持跨行匹配；坏正则在面板里报真实诊断。工作区文件遍历**不换** `ignore`（保持与侧栏可见文件严格一致）。

已定盘的八个岔路口：

| # | 议题 | 决定 | 拍板时间 |
|---|---|---|---|
| D1 | 范围 | **仅换引擎**，遍历不换 `ignore` | 2026-10-05 |
| D2 | 跨行正则 | **本期做** | 2026-10-05 |
| D3 | 坏正则 | **面板内报真实诊断**，不退化成字面量 | 2026-10-05 |
| D4 | 重复扫描 | **收敛成一份缓存命中表** | 2026-10-05 |
| D5 | 阶段切分 | 阶段 2 结束即发布，**阶段 3（跨行/跨块）可独立砍** | 2026-10-05 |
| D6 | 工作区跨行 | **两边都支持跨行**（文档 + 工作区），ordinal 按 §4.7 重定义 | 2026-10-05 |
| D7 | 源码模式跨块选区 | **做真跨块选区**（不钳在起点 chunk） | 2026-10-05 |
| D8 | `replace_all` 跨块命中 | **走跨块字节写回真替掉**，不跳过、不拒批 | 2026-10-05 |

D6/D7/D8 三格都是"取更正确的那条路"，代价因此比初稿高：阶段 3 的工作量从 ~400 行涨到 ~800 行，且新增两条风险（R12 ordinal 重定义、R13 源码模式跨块选区是新路径）。D5 已经把这一格隔离成可独立砍发布，这是 D6/D7/D8 可以放心取难路的**前提**——如果 D5 反悔改成"一起做完再发"，这三条的风险形状会立刻变差（跨块链路与引擎替换揉进一次发布，红了无法定位是哪一半的锅）。


## 1. 现状诊断

### 1.1 引擎其实已经是 ripgrep 的内核，缺的是外面那层

`search_backend.rs:23-29` 用 `regex::RegexBuilder`——和 ripgrep 同一个 `regex-automata` 内核。所以"换 ripgrep"不是换正则引擎，而是换**围绕引擎的那层**：行迭代、字节偏移记账、多行支持、词边界组合、模式编译诊断、二进制/编码处理。这些现在全是手写的。

### 1.2 实测出来的既有缺陷（都在搜索链路上，本方案顺带修）

| # | 缺陷 | 位置 | 证据 |
|---|---|---|---|
| 1 | 正则模式下 `whole_word`（"ab" 按钮）**被完全忽略** | `search_backend.rs:43-47` 拿到 regex 就提前 return，`:59-61` 的 retain 永远走不到 | 实测 `NeedleHere` 在开启单词边界时仍被当命中 |
| 2 | 正则模式下 `fuzzy` 同样被忽略（与 #1 同一条 return） | 同上 | 代码路径 |
| 3 | 坏正则**静默退化成字面量搜索**，用户以为在跑正则 | `search_backend.rs:26` `builder.build().ok()` | 实测 `a{2,` 退化后按字面量命中 |
| 4 | 跨行模式在结构上不可能：`split_inclusive('\n')` 逐行喂 | `search_backend.rs:389/475/524` | 实测 `needle\.\nsecond` 现状 0 命中 |
| 5 | 非 UTF-8 文件整体跳过内容搜索 | `search_backend.rs:314-317` `String::from_utf8` 失败即 `return None` | 实测：GB18030 的 `会议.md`，查询「中文」工作区搜索 0 条命中；同一份文本写成 UTF-8 就 1 条 |
| 5b | **（初稿记错了，实测后更正）** 打开侧也有一道同样的闸：`is_likely_text_file` 只看头 8 KiB 能否按 UTF-8 解释，GB18030 文件在工作区标签里显示「无法使用文本编辑器预览该文件」，**根本打不开** | `search_backend.rs:561-568` → `tabs.rs:290` | 实测：`is_likely_text_file(GB18030 文件) == false`，而 `decode_document_bytes` 对同一批字节能干净解出「# 会议记录\n\n中文正文与 English\n」。`encoding.rs` 模块说明称 GBK/GB18030 笔记「可以正常打开编辑」，这道闸与该说明矛盾 |
| 6 | 工作区结果每文件只收"每行第一个命中"，文档范围收"每个命中" | `search_backend.rs:392` vs `:478` | ordinal 语义因此是"第 k 个含词行"，两套口径必须一直对齐 |
| 8 | 内容缓存超预算时驱逐是 O(已缓存条目) 每次插入，热跑反而比冷跑慢 45% | `search_backend.rs:313-316`（`cache.values().map(len).sum()` 在驱逐循环里） | 实测见 §11 阶段 2 那张表；A/B 确认与本方案的引擎替换无关，登记不修，留给阶段 4 性能闸门 |

#1–#4 是换引擎白捡的修复；#5 是本次要补的口径统一；#6 是要**保留**的既有语义（ordinal 跳转依赖它，见 §4.7）。

初稿把 #5 写成「编辑器能打开、只有搜索搜不到」，实测不成立——两处（打开与搜索）都被非 UTF-8 挡住了，于是拆成 #5（搜索侧，阶段 2 修）与 #5b（打开侧，按 §2.3 的既定边界**本次不做**，登记在册）。#5b 的实测断言钉在 `tests/workspace_scan.rs::defect_the_text_sniff_still_refuses_a_gb18030_file`，谁修打开侧它会红。

### 1.3 为什么不能只改 `find_in_line` 就完事

`find_in_line(&str)` 这个签名本身就锁死了"行是搜索单位"。跨行匹配要求把"字节流 + 行迭代器 + 命中区间"作为一个整体来算，也就是 grep-searcher 的 `Searcher` + `Sink` 模型。所以接口要下沉一层：从"给我一行我还你区间"变成"给我整段字节我还你带绝对偏移和行号的命中"。

## 2. 三本账

### 2.1 能做到什么（可感知收益，全部实测）

**全篇扫描**（结果列表 + 高亮，每敲一个键付一次）：

| 查询形态 | 现状 | ripgrep | 倍数 | 命中数是否一致 |
|---|---|---|---|---|
| ASCII 大小写不敏感 | 14.5 ms | 9.6 ms | 1.5x | 81427 = 81427 |
| ASCII 大小写敏感 | 8.5 ms | 2.6 ms | 3.3x | 43881 = 43881 |
| **中文单字** | **54.1 ms** | **2.1 ms** | **25x** | 75092 = 75092 |
| **中文词** | **48.9 ms** | **1.5 ms** | **32x** | 38742 = 38742 |
| 单词边界 | 7.2 ms | 7.5 ms | **0.96x（退化）** | 75095 = 75095 |
| 正则 | 6.7 ms | 5.6 ms | 1.2x | 82623 = 82623 |

最后一列是本方案最重要的安全声明：**六种形态在 10 MiB 真实混合文档上产出的字节区间逐一相等**，换引擎在常见模式下是行为等价的。

**跳转**（10 MiB、20 个命中、从文档 10% 处按 F3）：

| 场景 | 现状 | ripgrep |
|---|---|---|
| ASCII 不敏感 | 9.50 ms/次 | 0.076 ms/次 |
| ASCII 敏感 | 7.28 ms/次 | 0.023 ms/次 |
| **中文查询** | **61.0 ms/次** | **0.006 ms/次** |
| 无后续命中需回绕 | 12.88 ms（两次全扫） | 0.079 ms |
| 密集命中、连按 50 次 | 337–2466 ms（6.7–49 ms/次） | <0.1 ms 总计 |

加上命中表缓存（§4.3），F3/F4 变成"对已算好的有序区间做一次二分"，即 **O(log n)**，且**与文档大小无关**——这是数字上比"提前停止"更稳的那一半收益。

**能力解锁**（现状结构上做不到的）：
- 跨行匹配：实测 `NEEDLE here\nthird` 在 54 字节小样本上命中 `18..35`，行式模式 0 命中；`first[\s\S]*?third` 命中 3 行 `0..35`。
- 真实诊断：ripgrep 给 `repetition quantifier expects a valid decimal` / `unclosed group` / `invalid character class range` / `Unicode property not found` 这类带插入符定位的报错，直接进面板。
- 编码口径统一（#5）：工作区内容搜索与编辑器打开走同一条解码路径。

### 2.2 取舍是什么（含不退步的地方，不粉饰）

**不会变好的地方，明说**：

1. **单词边界模式慢 4%**（7.2 → 7.5 ms）。原因：现状是"廉价字面量扫 + 事后过滤"，ripgrep 把 `\b` 编进模式，引擎负担更重。净收益仍是中文 25x 和跳转 1000x+，但这一格是退的。
2. **多文件工作区扫描基本没动**：2000 文件 × 5 KiB（约 10 MB）冷缓存实测 22.3 → 20.6 ms（读盘+引擎，1.08x）/ 19.1 ms（让 searcher 自己读、走 mmap，1.17x）。**I/O 主导，换引擎救不了**；这块的收益继续来自已有的内容缓存，不来自本方案。
3. **命中区间要算两遍**。grep-searcher 的 `SinkMatch::bytes()` 返回的是**命中所在的整行（含终止符）**，不是命中本身——实测确认：28 字节的跨行命中，`bytes()` 长 36（两整行）。所以精确区间必须再跑一次 `matcher.find_iter()` 在它上面求，这正是 ripgrep 自己做高亮的方式。全篇扫描没跑到理论上 3x 的原因就在这里，这个常数成本消不掉。
4. **`Sink::Error` 不接受 `anyhow::Error`**。实测编译失败；只有 `std::io::Error` 和 `Box<dyn Error>` 实现了 `SinkError`。本方案的 sink 不会失败（matcher 的 Error 是 `NoError`），用 `io::Error` 占位，属于已知约束不是坑。
5. **跨行匹配把五段跨块链路拖进来**（§4.6）。D6/D7/D8 三条都取了更难的路，这一段从初稿估的 ~400 行涨到 **~800 行**，其中"源码模式真跨块选区"和"跨块真替换"是**现状不存在的新路径**（不是改条件，是新写）。跨行匹配的日常价值（在 markdown 里搜 `标题\n下一段`）明显低于本体收益。**D5 已把它隔离成可独立砍的一格**：阶段 2 落地即发布，阶段 3 任一小节红了就停在那。
6. **`dot_matches_new_line` 是独立开关**：实测 `needle.{0,12}fourth` 在 `-U` 下仍**不跨行**，因为 `.` 默认不匹配 `\n`。跨行要么写 `\n`/`[\s\S]`，要么用户开 `s` 标志。行为和 ripgrep 一致，但和"开了跨行就什么都能跨"的直觉不一致，得写进帮助文案。

**工作量**：新增 ~500 行（引擎封装 + 命中表），改写 ~250 行（四个消费方接线），删除 ~300 行（手写匹配层），跨行/跨块 ~400 行，测试 ~1200 行。合计一个分支、五阶段、每阶段可独立发布。

### 2.3 本次不做什么（边界与复利成本）

| 不做 | 理由 | 复利成本 |
|---|---|---|
| 工作区遍历换 `ignore`（gitignore/隐藏文件/并行 walker） | 用户拍板。搜索结果必须与侧栏可见文件严格一致，否则出现"搜到看不见的文件"或"看得见的搜不到" | `.gitignore` 里的大目录（`node_modules`、`dist`）永远进不了搜索——但现状 `scan_workspace_dir` 已硬编码跳过 `.git/target/node_modules/.worktrees/dist`，所以这个缺口本来就在，不会因本方案变大 |
| 模糊搜索换 `nucleo` | ripgrep 没有模糊模式，这不属于"适配 ripgrep" | `fuzzy_subsequence_ranges`（`search_backend.rs:159`）继续是手写 O(行长²)，与中文全扫同一个病；日后单独立项 |
| 流式读缓冲区（免 `buffer.text()` 全篇拼接） | 实测拼接 10 MiB 只要 **0.26 ms**，不是瓶颈。原以为要先做，数据说不用 | 100 MiB 级文档才会重新变成问题，届时 `Searcher::search_reader` 能直接吃 `Read` 适配器，命中表接口不变 |
| 搜索历史、大小写智能（smart-case）、搜索面板多行输入 | 与本方案无关 | 无 |
| 非 UTF-8 文件的**编辑器内**行为改动 | 本次只统一搜索侧的解码口径 | **实测比初稿估的大**：挡住的是 `is_likely_text_file` 这道嗅探闸（#5b），后果不是「行为差异」而是 GB18030 文件**打不开**（只见「无法预览」占位）。要修就是改嗅探：8 KiB 窗口按 UTF-8 不通时，再试 GB18030 能否干净解码。搜索侧已经能吃这类文件了，所以这一格修完就是完整闭环，成本约一个函数 + 3 条测试 |

## 3. 选型与被否方案

### 3.1 定：`grep-searcher` + `grep-regex` + `grep-matcher`

库化 ripgrep，进程内调用。依赖成本实测：新增 3 个 crate；其全部传递依赖——`bstr 1.13.1`、`aho-corasick`、`regex-automata 0.4.18`、`regex-syntax 0.8.11`、`memchr`、`memmap2`、`walkdir`、`log`、`crossbeam-*`——**已在 `Cargo.lock` 且版本正好对得上**，零版本重复、零额外编码依赖（`encoding_rs` 也已在）。许可 `Unlicense OR MIT`，与 `Apache-2.0` 项目兼容。

模式组合全部交给引擎，与 ripgrep CLI 的 `--ignore-case` / `--fixed-strings` / `--word-regexp` / `-U` 一一对应：

```rust
let mut b = RegexMatcherBuilder::new();
b.case_insensitive(!opts.match_case);
if !opts.use_regex { b.fixed_strings(true); }   // 字面量模式：元字符不再需要手工转义
if opts.whole_word { b.word(true); }            // 修 #1：正则模式下也生效
if multiline { b.multi_line(true); }
```

### 3.2 否：只把 `regex::Regex` 换掉、其余不动

否。跳转的 O(文档) 与四路径重复计算都跟正则引擎无关，这个方案只能拿到 2.1 里中文全扫那一格，把两份更大的收益留在桌上。

### 3.3 否：外部 `rg` 进程

否。要绑二进制存在性、版本、启动开销（实测冷启动进程 + 管道对 2000 文件场景无优势），且内存文档（脏缓冲区）根本不在磁盘上——搜磁盘文件会与"搜索缓冲区这一事实源"的既定架构直接冲突。

### 3.4 否：继续修手写层

否。要在手写层里加跨行匹配、正确 Unicode 折叠、`\b` 组合、编译诊断，等于重写一个更弱的 grep-searcher；而且中文 O(n²) 那条（`:121-153`）是结构问题，不是能调优出来的。

## 4. 目标架构与数据流

### 4.1 模块

```
src/editor/workspace/
  search_engine.rs      【新】ripgrep 绑定层：编译模式、Sink、字节扫描、
                         字符边界过滤、增量 next/prev、诊断文本
  document_matches.rs   【新】DocumentMatchTable：按 (缓冲区版本, 查询, 选项)
                         缓存的唯一命中表 + 二分导航
  search_backend.rs     【瘦身】SearchOptions / SearchMatcher 外观（保持既有签名
                         以减少调用方震动）、fuzzy、文件名匹配、工作区文件收集、
                         内容缓存、WorkspaceSearchHit 产出
  find_replace.rs       调用方：命中表读、诊断存、替换走命中表
  tree_sync.rs          高亮走命中表 + 跨块切段
  render_search.rs      结果列表渲染 + 诊断显示 + 跳转
```

`search_engine.rs` 不依赖 GPUI，纯函数/纯结构，可在无窗口的单元测试里直接跑——这是测试能写详尽的前提。

### 4.2 引擎层 API

```rust
pub(crate) struct CompiledQuery { /* RegexMatcher + mode flags + error */ }
pub(crate) enum QueryError { InvalidRegex(String) }   // 携带 ripgrep 诊断原文

impl CompiledQuery {
    pub fn compile(query: &str, options: SearchOptions) -> Result<Self, QueryError>;
    /// 命中：绝对字节区间 + 首个命中行的行号
    pub fn find_all(&self, haystack: &[u8]) -> Vec<Hit>;
    /// 从 offset 起第一个命中（提前停止，O(距离)）
    pub fn next_hit(&self, haystack: &[u8], from: usize) -> Option<Hit>;
    pub fn prev_hit(&self, haystack: &[u8], before: usize) -> Option<Hit>;
}

pub(crate) struct Hit { pub range: Range<usize>, pub line: Option<u64> }
```

内部实现要点（全部已由探针验证）：
- `SearcherBuilder::line_number(true).multi_line(multiline).build()`；`Searcher::search_slice(matcher, bytes, sink)`。
- sink 里 **`matcher.find_iter(sink_match.bytes_without_terminator())`** 还原精确区间，`base = sink_match.absolute_byte_offset()`。多行模式下 `bytes()` 是覆盖命中的整块行，同样靠这一步取准。
- 多行模式必须先把 matcher 建成 `multi_line(true)`（其 `line_terminator()` 返回 `None`），否则 `Searcher::multi_line_with_matcher` 会静默退回行式——这是实测确认的陷阱。
- **所有命中出口统一做字符边界过滤**（§4.4）。

### 4.3 命中表与失效

`TextBuffer` 加一个 `revision: u64`，只在 `edit()` 里自增（O(1)，不读正文，与它现有的 `line_probe_bytes` 纪律一致）。

```rust
pub(crate) struct DocumentMatchTable {
    key: (u64, String, SearchOptions),   // 版本 + 查询 + 选项
    hits: Arc<[Hit]>,                    // 按 range.start 升序
}
```

`Editor.workspace.document_matches: Option<DocumentMatchTable>`。取用时 key 命中就复用，否则重算一次。四个消费方（结果列表 / 高亮 / 跳转 / 替换）**只读这一张表**。

收益不只是快。现在的结构性问题是"结果列表说有第 7 个命中"和"跳转落到第 8 个位置"可以来自两份独立计算——历史上这一类 bug 修过好几轮（`FIXPLAN.md` B8 replace_all 虚报、`render_search.rs:781` 的 canonicalize 兜底注释）。一张表之后，这类不一致**在类型上不可表达**。

跳转改为对 `hits` 做 `partition_point`：F3/F4 变成 O(log n)，回绕是头尾索引翻转，不再有"第二次全扫"。

### 4.4 硬闸门：字符边界过滤（本方案唯一必须新增的正确性防线）

实测确认：`grep-regex` 的 `find_iter` 在**零宽命中**上按**字节**步进，而 `regex` crate 按**字符**步进。样本 `second 中文 ok`（`中` = 字节 7..10，`文` = 10..13）配 `a*`：

| 引擎 | 命中 |
|---|---|
| 现状（regex crate） | `0..0 1..1 2..2 3..3 4..4 5..5 6..6 7..7 10..10 13..13 14..14 15..15 16..16` |
| grep-regex | 同上 **+ `8..8 9..9 11..11 12..12`** ← 落在字符内部 |

非 ASCII 多字节字符越多，越界命中越多；多行模式同样有（实测 `中 x a\n` 上 `a*` 给出 `1..1 2..2`）。不过滤的后果是具体而严重的：`find_replace.rs:408` 的 `line[found.start..found.end]` 直接 **panic**；`tree_sync.rs` 的换算会把非边界偏移喂进 `source_to_content`；高亮区间会给块组件后切片就崩。

措施：`CompiledQuery` 的每个出口过滤掉 `!haystack.is_char_boundary(start) || !…(end)` 的命中，**并在过滤处留一条断言测试**（正则 `a*` + 纯中文文档，断言零越界）。这条不允许写在调用方——只允许在引擎出口，否则四个消费方各漏一个。

顺带保留：`render_search.rs:824-827` 现有的 `is_char_boundary` 守卫在收敛到命中表之后可以删（引擎已保证）。

### 4.5 坏正则诊断（决策已定：面板内报真实诊断）

- `CompiledQuery::compile` 失败 → `SearchMatcher` 携带 `QueryError`，**不再退化成字面量**。
- `WorkspaceState.search_error: Option<String>`，`render_search.rs` 头部下方一行红字，取 i18n 文案前缀 + ripgrep 原始诊断（含 `^` 定位行）。
- 语义：**搜索不执行**，结果列表清空，`search_pending = false`。这点必须显式，否则"没结果"和"报错"在用户眼里一样。
- `replace_all_*` 在有诊断时**拒绝执行**并返回 0（配合 `replace_all_replaces_exactly_the_hits_it_reports` 的既有契约：计数不许虚报）。
- i18n 需新增 2 条文案（zh/en），走现有 `I18nStrings` 结构。

### 4.6 跨行匹配与跨块链路（本期最重的一段，代价全在这里）

调研结论（逐条核到行号）：**选区和替换已经支持跨块，高亮不支持，展开只看起点，源码模式会被钳。**

| 环节 | 现状 | 依据 |
|---|---|---|
| 渲染模式选区跨块 | **已支持**，两个端点各自解析后落到不同块，`cross_block_selection` 给每个被覆盖块上选区 | `selection.rs:328-372`、`:474-503`；已被 `cross_block_cut_writes_markdown_deletes_range_and_undo_restores` 钉住；`history.rs:13`、`window_state.rs:417/747`、`selection.rs:708/758`、`paste.rs:62` 都在用 |
| 源码模式选区跨块 | **被钳进起点所在 chunk**；且 `snapshot.range.start - chunk_start` 在映射取到起点之后的 chunk 时**下溢** | `history.rs:297-302`（注释自己写明"跨块的选区先落到起点所在的那一根"） |
| 高亮 | **静默丢弃**：`if hit.start < block_start \|\| hit.end > block_end { continue; }`；且命中是**逐根块切片**算的，跨两个根块的命中根本不会被产出 | `tree_sync.rs:120`（这道是主墙）、`:40-49`、`:141`、`:145` |
| 折叠展开 | **只看 `range.start`**，命中尾部所在章节不展开 | `tree_sync.rs:446` |
| 替换当前命中 | 走 `RequestReplaceCrossBlockSelection` → 缓冲区字节写，**可用**；例外是命中正好结束于块首时端点块被置空高亮，退成单块替换 | `find_replace.rs:372`、`components/block/input.rs:97-105`、`events/block_event.rs:17-32`、`selection.rs:676`、`:488-492` |
| 全部替换 | 映射查找是纯包含判断，跨块命中 `continue` **静默跳过**；单块内跨行命中会卡在 `block_text[visible] != matched`（display 文本软换行 ≠ 原始字节）而静默跳过 → 计数少报，不 corrupt | `find_replace.rs:428-431`、`:445` |

要做的事，按依赖顺序：
1. `sync_document_search_highlights` 的命中来源从"逐根块切片搜"改为**读整张命中表**（这正是 §4.3 的收敛带来的免费收益）。
2. `search_ranges_for_hits` 支持把一个跨块命中**按块切段**，每块拿到自己那截；端点块的可见区间由该块映射算。同时改掉 `tree_sync.rs:145` 对"活动命中"的同款单块包含判断。
3. `unfold_sections_covering_source_range` 同时看 `range.start` 与 `range.end` 所属章节。
4. **源码模式做真跨块选区**（D7）。现状源码视图的块是缓冲区切片、区间天然首尾相接，跨块比渲染模式更简单：把 `history.rs:297-302` 那段"钳进起点 chunk"换成**按 chunk 拆成多段选区**——锚块拿 `selected_range`，其余被覆盖的 chunk 各拿自己那截（源码视图是恒等映射，`content == source − chunk_start`，不需要走 `source_to_content` 表）。顺带修掉 `snapshot.range.start − chunk_start` 的**下溢隐患**（端点先 clamp 再减），并给它一条**不依赖跨行功能**的红测试（直接构造 `range.start` 早于最近映射起点）。
5. **`replace_all` 对跨块命中走真跨块写回**（D8）。现状是映射查找纯包含判断 → 静默跳过。改成：连续的同块命中合并成一组，**跨块的命中直接按缓冲区字节区间从后往前替换**（`buffer` 已经有区间写回的原语，见 [[project-buffer-source-of-truth]] 的 `write_back_*` 一族），替换后让受影响的根块重新投影。**"绝不许替换到错误位置、也不许计数虚报"这条契约不变**（`find_replace.rs:419-421` 注释）：所以这一格必须配一条"替换后重扫，命中数归零且字节差异恰好等于替换项"的守卫，以及撤销逐段退回的守卫（表格那批多编辑组命令已有同款先例）。
6. 单块内跨行命中的 `block_text[visible] != matched` 守卫（`find_replace.rs:445`）要改口径：比较对象从"块 display 文本"换成"缓冲区那截原始字节"——display 文本软换行不等于原始字节，这是现状跨行命中被静默跳过的真正原因，属于守卫选错了对照物，不是数据问题。

风险自认（这一格的形状在 D6/D7/D8 之后变差了，说清楚）：阶段 3 的工作量从初稿估的 ~400 行涨到 **~800 行**，其中第 4 项（源码模式跨块选区）与第 5 项（跨块真替换）是**两条现状根本不存在的路径**——不是改条件，是新写。跨行匹配的日常价值（在 markdown 里搜 `标题\n下一段`）明显低于阶段 1–2 的本体收益，却把选区/高亮/折叠/替换/撤销五套链路一起拖进场。**D5 已把这一格隔离成可独立砍的发布**：阶段 2 落地后已可发布，阶段 3 任一小节红了就停在那一小节，不回头牵连阶段 1–2 的成果。这是 D6/D7/D8 敢取难路的唯一理由，也是它的前提。

### 4.7 工作区范围支持跨行 + ordinal 重定义（D6）

工作区命中是**读磁盘**产出的，跳转时在编辑器文本里靠"该文件内第 k 个含词行"重新定位（`render_search.rs:806-819`）。不能用行号：脏编辑会把行上下推。跨行命中要进这条路径，就得给 ordinal 一个能跨行的定义。

**新定义：ordinal = 该文件内「第 k 个含命中起点的行」，命中取该行里第一个开始于该行的命中（连同它跨到的那段）。**

为什么这个定义对既有行为是**保守的**（不是折中，是可证明的等价）：现状是"每行只收第一个命中"（`search_backend.rs:392`），而任何不跨行的命中，它所在的行就是它开始的行——所以"第一个命中所在行" ≡ "第一个开始于该行的命中"。对**非跨行**查询，新定义与旧定义**逐位相同**，ordinal 序列与命中区间都不变。只有真跨行命中才走出新语义。

配套改动：
- 扫描侧：工作区引擎也开 `multi_line`（文档/工作区两路共用同一份编译与过滤代码，只是 haystack 不同）。产出按"命中起始行"分组、每组留第一条，ordinal 按组自增。`WorkspaceSearchHit.line` = 命中的**起始行**（与 ripgrep `SinkMatch::line_number()` 的语义一致，实测确认它给的就是跨行命中的首行）。`match_range` 保持"行内偏移"的旧口径不变，新增 `跨行数` 字段或把 `source_range` 复用为整段区间——**这条接口选择留给阶段 3 落地时定，两种都能满足跳转**。
- 重定位侧：`render_search.rs:806-819` 那段手写"数第 k 个含词行"的循环**删掉**，改为用同一个引擎在缓冲区文本上算一张同规则的命中表、取第 k 项。这是 §4.3 收敛的第二处红利：磁盘侧与缓冲区侧从此**用同一个规则**，不再需要两套代码保持巧合一致。
- 脏文件的极端情况：编辑把某个跨行命中的中间那行删了 → 该命中不再存在。重定位取不到就退回"第一个命中"（保持 `find_document_match_from` 现有的兜底形状），并在结果列表把该项标记为已失效。

闸门（这条必须钉死）：既有 6 个 `workspace_search_*` 测试**一个都不许改期望**——`workspace_search_matches_file_names_and_contents`、`clicking_a_search_hit_in_a_dirty_file_lands_on_the_match`、`same_file_workspace_hit_jumps_by_the_file_line`、`workspace_search_reports_all_hits_and_jumps_by_proximity`、`workspace_search_cache_picks_up_modified_content`、`workspace_search_returns_content_hits_after_typing`。它们全是非跨行查询，按上面的等价性论证必须原样通过。**红了一条就是等价性论证错了，不是测试写错了。**

顺带消失的口径分叉：初稿曾建议"工作区不做跨行"，代价是"当前文档搜得到、所有文件搜不到"这种隐性分叉。D6 选了难路，这个分叉就不存在了。

## 5. 分阶段迁移与闸门

每阶段结束：`cargo test` **全绿**（阶段 0 后基线数只会增加不会减少）、可发布、单独一个中文 commit。**不做长期双轨**——阶段 1 起手写匹配层逐块退役，不留开关。格式化只在自己改动的 hunk 内，不跑全仓 `cargo fmt`。

| 阶段 | 内容 | 闸门（可验证，不靠形容词） |
|---|---|---|
| **0 表征测试**<br>（不碰架构，约半天） | 把 `/tmp/rgprobe` 的 old-vs-new 对照变成仓库测试：对现有 `SearchMatcher` 建立**行为快照**——6 种模式 × 中/英/表格/CJK/emoji/坏模式/零宽模式 × 期望命中区间。纯单元测试，无 GPUI。 | 新测试在当前代码上**全绿**（它们记录现状，包括现状的 #1/#3 缺陷）。缺陷类用例先写成"当前行为的快照"，阶段 1 里按新语义逐条迁移并在 diff 里点名 |
| **1 引擎接入 + 命中表 + 诊断**（文档范围，行式） | 引入三个 crate；`search_engine.rs`；`TextBuffer::revision`；`DocumentMatchTable`；`find_replace.rs` / `tree_sync.rs` / `render_search.rs` / `search_document_source` 四个消费方改读命中表；`QueryError` + 面板诊断 + i18n；字符边界过滤 | ①阶段 0 快照测试除"缺陷类"外全部保持绿；②**新增对照测试：同一语料、同一查询，新旧引擎命中区间集合相等**（复现 2.1 表的第一列）；③新增跳转测试：F3/F4 循环、回绕、编辑后重定位、脏文档；④零宽 `a*` + 中文文档断言零越界；⑤`cargo test` 总数 ≥ 基线+新增，0 失败 |
| **2 工作区范围接入**<br>（**到这里为止可发布**，D5） | `search_single_file` / `cached_file_source` 改吃引擎（**行式**，跨行留到阶段 3）；内容搜索解码口径统一到 `decode_document_bytes`（修 #5）；命中数与 ordinal 语义**逐位保持** | ①`workspace_search_*` 既有 6 个测试全绿不改期望；②新增 GB18030 文件内容命中测试；③CRLF 文件的行号/区间测试；④`search_bench.rs` 手动基准跑一遍并记录进 commit body。**阶段 2 收尾即一次发布**（`chore(release)` 笔法见 [[feedback-release-commit-shape]]） |
| **3 跨行匹配 + 跨块五链路**<br>（**可独立砍**，D5） | `CompiledQuery` 多行模式（文档 + 工作区两路都开，D6）；§4.6 的 1–6 项，含 D7 源码模式真跨块选区、D8 跨块真替换；§4.7 的 ordinal 重定义与重定位改造 | ①**等价性硬闸门：6 个 `workspace_search_*` 既有测试仍不许改期望**（它们全是非跨行查询，按 §4.7 的论证必须原样通过；红一条就是等价性论证错了）；②跨行命中在渲染模式：高亮、选区、滚动居中三项各有测试；③**源码模式跨行/跨 chunk 命中的真跨块选区**有测试，且端点下溢那条有独立红测试；④跨行命中跨**折叠章节**时两端都展开；⑤`replace_all` 跨块真替换：替换后重扫命中归零、字节差异恰好等于替换项、撤销逐段退回；⑥`dot_matches_new_line` 的 `.` 不跨行要有测试（防直觉）；⑦脏文件里跨行命中被编辑破坏后的退回行为有测试 |
| **4 收尾** | 删 `case_insensitive_ranges` / `is_word_boundary` / 手写 ASCII 滑窗 / `render_search.rs:824-827` 冗余守卫；补性能闸门；更新 `docs/plans` 与 README 搜索段落 | ①`grep` 确认手写匹配函数无残留调用者；②新增 `perf_budgets.rs` 闸门（见 §7.4）；③全量测试绿；④警告清零（对齐 `6e2472e` 的既有纪律） |

## 6. 模块清单：保留 / 重写 / 删除

**保留原样**：`WorkspaceSearchHit`（`workspace.rs:136`）字段口径；`WorkspaceSearchFile` + `collect_workspace_search_files`（树序是命中顺序的定义）；`search_content_cache` / `cached_file_source` 的 mtime+len 失效与 LRU 驱逐；`fuzzy_subsequence_ranges`；`matches_filename`；`is_likely_text_file` / `has_utf16_bom`；`search_file_label`；`schedule_workspace_search` 的 120 ms 去抖与"保留上一次结果不闪空"。

**重写**：`SearchMatcher`（外观保留，内部转 `CompiledQuery`）；`search_document_source`（读命中表）；`find_document_match_from`（删除，由命中表二分替代）；`search_single_file`（引擎扫描；阶段 2 行式，阶段 3 起开多行并按 §4.7 重定义 ordinal）；`sync_document_search_highlights`（读命中表 + 跨块切段）；`replace_all_document_matches` / `replace_all_workspace_matches`（命中表 + 跨块真替换，D8）；`render_search.rs:806-819` 的手写"数第 k 个含词行"循环（删除，改为缓冲区侧用同一引擎算同规则命中表取第 k 项）；`history.rs:297-302` 源码模式选区钳位（改为按 chunk 拆多段真跨块选区，D7）。

**删除**：`case_insensitive_ranges`、`case_insensitive_contains`（若仍有调用者则并入引擎）、`is_word_boundary`、`find_in_line` 的逐行手写分支、`search_backend.rs:26` 的 `.ok()` 静默退化、`render_search.rs:824-827` 的冗余边界守卫、`VELORA_SEARCH_JUMP_DEBUG` 那两处临时 eprintln（`find_replace.rs:113/164`）改走一次性的诊断字段。

## 7. 测试策略（"详尽测试"是本方案的一等交付物）

### 7.1 引擎层（纯逻辑，无 GPUI，快）

- **对照矩阵**：`{字面量, 正则} × {Aa 开/关} × {ab 开/关} × {多行 开/关}` = 16 组 × 语料 8 份（纯 ASCII / 中英混排 / 纯中文 / emoji / 表格行 / front matter / 空行密集 / 超长行），断言命中区间集合。
- **快照 vs 新语义**：阶段 0 生成的快照表逐条对照，差异只允许落在**已登记的四类**上：#1 词边界在正则模式生效、#3 坏模式报错不退化、Unicode 折叠（`İ`/`ß`）、零宽命中的边界过滤。任何第五条差异 = 红灯。
- **零宽/边界专项**：`a*`、`^`、`\b`、`\p{Han}*`、`x*` × 中文/emoji/组合字符（`é` 的 NFC/NFD）；断言每个区间的 `is_char_boundary` 两端都真。
- **多行专项**：`needle\nsecond`、`[\s\S]*?`、`A.+$` 跨块、`.` 在 `s` 关时**不**跨行、多行命中跨 CRLF 磁盘文件。
- **诊断专项**：`a{2,`、`(unclosed`、`a|*`、`\p{Bad}`、`[z-a]`、`(?i(a` —— 六种，断言报出诊断且**不执行搜索**。
- **行号/偏移换算**：`absolute_byte_offset` 与 `split_inclusive('\n')` 行号在 无尾换行 / 单 `\n` / 连续空行 / CRLF / 只含 `\r` 的老 Mac 形状 上逐一对齐。

### 7.2 跳转层（GPUI `TestAppContext`，用户点名）

机制已具备（调研确认）：真布局、真滚动。断言用的是 `editor.scroll_handle.offset().y` 实测像素 + `active_range_or_cursor_bounds()` 对 `scroll_handle.bounds()` 中线的 `drift <= 2.0`，配合 `window.draw` × 8–16 帧的 settle 循环。既有 9 个跳转测试就是这个写法（`workspace_search_jump_scrolls_to_unpainted_matches`、`document_search_hit_inside_table_jumps`、`cycling_hits_within_one_viewport_still_centers`、`first_click_into_a_freshly_opened_code_file_lands_in_the_right_chunk` 等）。

新增：

| 场景 | 断言 |
|---|---|
| F3/F4 循环 100 次 | 访问序列严格等于命中表顺序，回绕点正确，无命中丢失/重复 |
| 空命中 | 不跳转、不动选区、`search_active_index` 保持 |
| 命中表 vs 跳转一致性 | 列表第 i 项的 `source_range` == 第 i 次 F3 落点（**这条是收敛成一张表之后才可能写出来的测试**） |
| 编辑后跳转 | 命中表按 revision 重算；插一行/删一行/中文输入法组合后 F3 落点仍在命中上 |
| 表格单元格内命中 | 锚点校正为宿主表格块（沿用 `find_replace.rs:138-149`），跨行命中覆盖多 cell 时每 cell 高亮 |
| 折叠章节内命中 | 命中所在章节展开；**跨行命中两端章节都展开** |
| 大文件首块未物化 | 跳转前 `flush_pending_materialization` 生效，第一次点击就落对（既有 512 行 bug 的回归位） |
| 源码模式 | **真跨块选区**（D7）：跨行/跨 chunk 命中在每个被覆盖 chunk 上都落对；另加一条端点下溢的独立红测试（构造 `range.start` 早于最近映射起点，不依赖跨行功能） |
| 工作区跨行 ordinal 等价性 | **闸门级**：非跨行查询下新旧 ordinal 定义逐位相同——用同一批语料跑两套定义并断言序列相等；且 6 个既有 `workspace_search_*` 测试期望一字不改 |
| 工作区跨行 + 脏文件 | 跨行命中所在文件被未保存编辑改动后，重定位仍落对或按 §4.7 退回并标记失效 |
| 跨块真替换 | 替换后重扫命中归零 + 整篇字节差异恰好等于替换项 + 撤销逐段退回（三条同场，缺一不发） |
| 焦点 | 跳完焦点仍在查询框（`document_find_jump_keeps_the_query_field_focused` 同款断言） |
| 视图模式切换后 | 高亮与活动命中存活（`…survive_a_view_mode_switch` 同款） |
| **真实点击结果行** | 现状 1523 行的搜索测试里**没有一个**用 `simulate_click` 点过 `workspace-search-hit-{i}` 行（只点过文件头行）——补上，走 `open_search_hit` 的真实 UI 路径 |
| 工作区脏文件 | 磁盘行号 ≠ 缓冲区行号时按 ordinal 落对（既有 2 个测试保持不改期望） |
| 坏正则 | 面板显示诊断文本；结果列表为空；`replace_all` 返回 0 |
| 零宽中文 | `a*` 搜中文文档，连按 F3 不 panic、不高亮越界区间 |

### 7.3 全量一致性（本方案的独特红利）

一条属性测试：随机生成 markdown（含中文、表格、围栏、front matter），随机查询与选项，断言"结果列表、高亮、F3 落点、replace_all 计数"四者引用的命中集合**完全同源**。现状这条写不出来（四路径各算各的），阶段 1 之后可以。

### 7.4 性能闸门

按 `perf_budgets.rs` 现成笔法（`min()` 三次墙钟 + 计数器差值断言）加两条，fixture 缺失时跳过：
- `searching_a_10mib_document_stays_within_budget`：全篇扫描 ≤ 25 ms、单次 F3 ≤ 2 ms。现状分别是 54 ms 与 61 ms（中文）——**闸门设在中间，先把退步挡住**。
- `document_jump_does_not_rescan_the_buffer`：连按 50 次 F3，断言命中表重算次数 = 0（计数器差值），即跳转路径一次都不重新扫描。

### 7.5 覆盖矩阵交付

一份表格写进阶段 4 的 commit body：每条既有搜索测试 + 每条新增测试 → 钉住的行为 → 属于哪个缺陷编号。便于日后回归定位。

## 8. 风险登记册

| # | 风险 | 概率/影响 | 处置 |
|---|---|---|---|
| R1 | **零宽命中越界导致 panic** | 高/严重，已实测复现 | §4.4：引擎出口统一过滤 + 专项测试；不允许下放调用方 |
| R2 | 换引擎后命中集合发生未登记的偏移 | 中/严重 | 阶段 0 快照 + 阶段 1 对照测试；差异必须逐条点名，无第五条 |
| R3 | 命中表失效判定漏掉某个改动点，出现陈旧命中 | 中/严重 | `revision` 只在 `TextBuffer::edit()` 一处自增（单一入口，可推理）；测试覆盖编辑/撤销/重载/外部改动/切换标签 |
| R4 | 阶段 3 的 ordinal 重定义在非跨行查询上就改变了行为（等价性论证有漏洞） | 中/**严重** | §4.7 的等价性论证 + 闸门①：6 个 `workspace_search_*` 既有测试**不许改期望**，红一条即认定论证错、退回重做定义，**不允许改测试保绿**（对齐 [[feedback-no-violating-intermediate-states]]） |
| R5 | 跨块高亮切段引入新的显示错位 | 高/中 | 隔离在阶段 3（D5 可独立砍）；`tree_sync.rs:120` 的墙必须与 §4.6 第 2 项一起改，不能只放开条件 |
| R6 | 源码模式跨块选区是**新路径**（现状只有钳位一条），写错会污染视图切换/撤销/粘贴那几条共用 `apply_selection_snapshot_in_current_mode` 的链路 | 中/中（D7 之后升到 高/中） | 端点先 clamp 再减（顺手修掉下溢）；共用同一条 snapshot 入口的既有测试（`window_state.rs:417/747`、`history.rs:475`、`selection.rs:708/758`、`paste.rs:62`）全部列入阶段 3 闸门回归清单 |
| R7 | 跨块真替换（D8）写坏用户没碰过的字节，或撤销退回顺序错 | 中/**严重** | 三条守卫必须同时在场：替换后重扫命中归零、整篇字节差异 == 替换项集合、撤销逐段退回（表格多编辑组命令已有同款先例可抄）。任何一条不过就把这一小节停在"跳过并显式报数"，不发布半吊子的跨块替换 |
| R8 | 坏正则从"静默能搜"改成"报错不搜"，用户觉得功能变少 | 低/中 | 用户已拍板（D3）；诊断文本带 ripgrep 原始信息，可自助 |
| R9 | 词边界模式全扫慢 4% | 确定/低 | 已计入 §2.2，不处理；性能闸门阈值按最慢形态设 |
| R10 | `multi_line` 与 matcher 配置不一致被静默退回行式 | 中/中 | 构造期断言 + 一条"跨行模式确实跨行"的测试（实测陷阱） |
| R11 | 新增 crate 与 vendored gpui 的 feature 交互 | 低/低 | 三者均不依赖 gpui/tokio；阶段 1 首次 `cargo check` 即暴露 |
| R12 | `.` 在 `-U` 下仍不跨行，用户直觉认为"开了跨行什么都能跨" | 中/低 | 帮助文案 + 一条防直觉的测试（`needle.{0,12}fourth` 实测不跨） |
| R13 | 工作区/文档两边都开跨行后，脏文件里跨行命中被编辑破坏，重定位取不到 | 中/低 | 退回"第一个命中"（保持现有兜底形状）并在结果列表标记失效；阶段 3 闸门⑦有测试 |

## 9. 决策记录与待确认

**已定**（2026-10-05 用户拍板，八条，见 §0 的 D1–D8）：范围=仅引擎，遍历不换 `ignore`｜跨行正则=本期做｜坏正则=面板内报真实诊断｜重复扫描=收敛成一份缓存命中表｜阶段 2 结束即发布、阶段 3 可独立砍｜工作区也支持跨行（ordinal 按 §4.7 重定义）｜源码模式做真跨块选区｜`replace_all` 跨块走真字节写回替换掉。

**待确认**（签字时一并定，都是落地细节而非方向）：

1. **`WorkspaceSearchHit` 怎么携带跨行命中的整段区间**（§4.7 留的接口选择）：加"跨行数"字段，还是复用 `source_range` 表示整段。两种都能满足跳转，倾向后者（少一个字段，且文档范围本来就在用 `source_range`）。
2. **诊断文本是否原样透出 ripgrep 的多行报错**（含 `^` 定位那一行），还是折成一行"正则语法错误：<首行摘要>"。面板宽度窄，原样透出更难读但更可自助；倾向折一行 + hover 展开全文。
3. 非 ASCII **大小写智能**（smart-case）这次不引入——需要时是引擎一个开关（`case_smart`），只是会让"Aa"按钮语义变化，留给后续。

## 10. 依赖与构建成本

```toml
[dependencies]
grep-searcher = "0.1.17"
grep-regex = "0.1.14"
grep-matcher = "0.1.9"
```

- 版本由探针实测确认（`/tmp/rgprobe` 解析并 release 构建通过）。
- 传递依赖全部命中现有 `Cargo.lock`，`regex-automata 0.4.18` / `regex-syntax 0.8.11` 版本精确一致 → 零重复编译单元。
- 许可 `Unlicense OR MIT`。与现有 `Apache-2.0` 及 `vendor/gpui` 无冲突；需在 `LICENSE`/依赖清单里补一条（如项目有该文件；本次不改发布流程）。
- 是否给 `grep-searcher` 关 `mmap`：默认开（`search_path` 走 mmap）。本方案的文档范围只走 `search_slice`，不受影响；工作区侧 mmap 实测带来 1.17x，保留。

## 11. 阶段落地记录（做完才写，数字都是实测）

### 阶段 0 — 行为快照（`336dad4`）

32 条表征测试进 `tests/search_matcher_snapshot.rs`。其中 6 条 `defect_` 前缀钉的是
当时的真实缺陷；换引擎后有 3 条按新语义改写成「修复后的期望」
（`whole_word_option_applies_in_regex_mode_now`、
`invalid_regex_reports_a_diagnosis_and_stops_searching`、
`unicode_case_folding_now_finds_the_dotted_capital_i`），3 条仍是缺陷
（正则模式吃掉 `fuzzy`、模式不能跨行、文档范围序号不唯一）。
唯一的硬闸门 `snapshot_zero_width_regex_matches_only_char_boundaries` 保持原样通过。

### 阶段 1 — 引擎 + 命中表 + 诊断（`ea26e3d`、`243f5d0`）

- 文档范围的匹配改走 `grep-searcher` + `grep-regex`；坏正则报真实诊断并停止搜索。
- 四处重复计算收敛成一张缓存命中表（`DocumentMatchTable`），跳转改为表上取索引。
- 过程中被测试抓到两个真问题：
  1. 点结果行只设了字节区间、没设表上索引，「下一个」从第 0 个重新开始——
     区间回查 + 按位置兜底解决。
  2. 缓存键原本只有 (缓冲区版本, 查询, 选项)，而**两份从没编辑过的文档版本号都是
     0**，跨文档复用了上一份的命中表（现象：跳转后高亮整体消失）——补
     `TextBuffer::identity` 进键，并留了专门的回归用例。
- 测试数 1343 → 1350（阶段 1a）→ 1350（1b 净增 7 条，含两条性质测试）。

### 阶段 2 — 工作区范围接入（`8a312bf`、`f2f71ca`）：本方案的可发布点（D5）

- 内容搜索的解码口径并进 `decode_document_bytes`（#5）。GB18030 笔记的正文从
  「一条都搜不到」变成与 UTF-8 版本逐位相同的命中。
- 行切分与偏移记账交给引擎；每根含命中的行只留第一个命中、收满上限即停
  （`rg -m` 语义）。
- 闸门①逐条确认：6 个既有 `workspace_search_*` 测试期望值一行没改，全绿。
- 测试数 1350 → 1353 → 1361。编译零警告。
- **新增缺陷 #8（实测，本方案不修，登记）**：工作区内容缓存超预算时驱逐是
  O(已缓存条目数) 每次插入。基准里 2000 文件 / 约 140 MB 语料超过 128 MB 的
  `SEARCH_CACHE_MAX_BYTES`，于是每插一条都要重算一次全表字节总和并线性找最久未用
  的条目。表现为**热跑稳定比冷跑慢 45%**：

  | 轮次 | 冷启动 | 热启动 | 串行 |
  |---|---|---|---|
  | 阶段 2 后（引擎版） | 201.9 / 210.2 / 198.3 ms | 296.2 / 293.2 / 293.9 ms | 204.6 / 204.1 / 205.7 ms |
  | 阶段 2 前（手写层，A/B 基线） | 204.3 / 210.5 / 236.6 ms | 300.3 / 343.3 / 300.9 ms | 207.0 / 209.5 / 207.6 ms |

  两行同形状 → 这条与本方案的引擎替换无关，是既有缓存实现的账。顺带可读出的
  结论：**本方案在工作区扫描上没有回退**（debug profile 下平均还略快 3–8%），
  并行相对串行无收益也同样是既有现象。修法（记账总字节 + 按插入序 O(1) 驱逐）
  留给阶段 4 的性能闸门一起做。
