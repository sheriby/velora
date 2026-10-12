# 分块 Attention + LSE 合并：正向恒等、反向不成立

> 分析对象：`common/transformers/models/qwen3_vl/transfusion_qwen3_unified.py`
> `Qwen3VLTextMoTAttention.forward` 第 742–794 行
> 对照实现：`transfusion_qwen3_unified_naive.py:696-705`（单次全量 `npu_fusion_attention`）

---

## 目录（大纲）

**第 1 章 · 问题描述与所处代码位置**

- 1.1 三块切分与四个区域（左上 / 左下 / 右下 / 右上为空）

**第 2 章 · 符号定义与统计量约定**

- 2.1 key 集合 U / G_i，score s_j，真值概率 p_j
- 2.2 分块统计量 m_A, Z_A, m_B, Z_B, α, β, Z
- 2.3 kernel 内部概率 p\^A_j 与真值概率 p_j 的换算关系

**第 3 章 · 正向推导：LSE 合并是恒等变形**

- 3.1 numerator / denominator 的展开
- 3.2 结论：正向无近似、无误差

**第 4 章 · 反向推导（核心）**

- 4.1 合并节点传给各块的梯度因子 c_A = ∂L/∂y_A
- 4.2 V 的梯度：链式因子与局部归一化恰好抵消（结论：正确）
- 4.3 Q/K 的梯度：kernel 内部减项用的是局部输出 y_A 而非合并输出 y（结论：错误）
- 4.4 误差项 dS\^true − dS\^split = p_j · dy·(y_A − y) 的物理含义

**第 5 章 · 自检不变量与误差量级**

- 5.1 softmax 雅可比的行和必须为 0 —— 分块后被破坏
- 5.2 误差何时退化消失（β→0 / dy ⊥ (y−y_A)）
- 5.3 误差是系统性偏差而非数值噪声

**第 6 章 · 影响范围矩阵**

- 6.1 三块 × (dQ/dK/dV) 的正确性表
- 6.2 受影响的具体参数（v_proj_moe_gen 正确，q_proj_moe_gen / k_proj / k_proj_moe_gen 错误）
- 6.3 und 分支为何完全不受影响

**第 7 章 · 数值验证方案**

- 7.1 最小复现实验设计（同一组 q/k/v 两条路径对比 .grad）
- 7.2 预期指纹：V 对齐、Q/K 不对齐
- 7.3 可直接运行的验证脚本

**第 8 章 · 修复方案**

- 8.1 方案一（推荐）：gen 两个块合并为单次调用（单 softmax，backward 天然精确）
- 8.2 方案二：自定义 autograd.Function，backward 传全局 (m, Z, y) 给 npu_fusion_attention_grad
- 8.3 两方案对照：改动量 / 精度 / 性能 / 风险
- 8.4 不推荐的路径：试图用梯度后处理修补

**第 9 章 · 结论与待确认事项**

- 9.1 一句话结论
- 9.2 需要用户环境确认的点（torch_npu 版本、npu_fusion_attention_grad 签名、act_seq\_\* 打包语义）

---

## 附录

- A. 关键代码片段摘录
- B. 符号速查表

## ---

# 正文

## 第 1 章 · 问题描述与所处代码位置

### 1.1 三块切分与四个区域

完整的序列排布是 `[und (N_und 个 token) ; gen (N_gen 个 token)]`，und 是文本侧（因果前缀），gen 是生成侧（图像 VAE token）。按行 / 列把注意力矩阵切成 4 块：

| 区域 | 行 | 列 | 是否计算 | 说明 |
| --- | --- | --- | --- | --- |
| 左上 | und | und | ✅ 块 1 | und 行只看 und 列 |
| 左下 | gen | und | ✅ 块 2 | gen 行看全部 und 列（und 全在前） |
| 右下 | gen | gen | ✅ 块 3 | gen 行看 gen 列 |
| 右上 | und | gen | ❌ 空 | und 行不允许看 gen（未来 token） |

这样做的动机很直白：**右上块整块为空，白白算它是浪费**；而且三个块可以各自用最合适的 layout（und 用 BNSD + 滑窗，gen 用 TND varlen 打包），比一次全量 kernel 更省显存也更快。

但代价是：

- **und 行只落在一个块里**（左上），它自己就是一个完整的 softmax → 正确。
- **gen 行同时落在两个块里**（左下 + 右下），两个块各自算出**局部归一化**的结果，必须再合并回一个完整 softmax → 这就是第 3、4 章的主题。

### 1.2 关键代码走读（742–794 行）

```python
# 1) und@und — BNSD + full mask
y_und = torch_npu.npu_fusion_attention(
    q_und, k_und, v_und, num_heads,
    input_layout="BNSD",
    atten_mask=npu_mask_uu,      # [1,1,N_und,N_und]，True=被 mask 掉
    pre_tockens=max_und, next_tockens=0,   # 因果 + 滑窗
    scale=self.scaling, keep_prob=keep_prob,
)[0].permute(0, 2, 1, 3).reshape(bs, und_token_num, -1).contiguous()

# 2) gen@und — TND varlen，全可见模板 mask
y_gu, max_gu, sum_gu = torch_npu.npu_fusion_attention(
    q_gen.float(), k_und_tnd.float(), v_und_tnd.float(), num_heads,
    input_layout="TND", softmax_layout="TND",
    atten_mask=gen_mask_gu,       # [max_gen, max_und]，全 0 = 全可见
    scale=self.scaling, keep_prob=keep_prob,
    actual_seq_qlen=act_seq_qlen_gu,
    actual_seq_kvlen=act_seq_kvlen_gu,
)[:3]

# 3) gen@gen — TND varlen，模板 mask
y_gg, max_gg, sum_gg = torch_npu.npu_fusion_attention(
    q_gen.float(), k_gen.float(), v_gen.float(), num_heads,
    input_layout="TND", softmax_layout="TND",
    atten_mask=gen_mask_gg,       # [max_gen, max_gen]，全 0 = 全可见
    scale=self.scaling, keep_prob=keep_prob,
    actual_seq_qlen=act_seq_qlen_gen,
    actual_seq_kvlen=act_seq_kvlen_gen,
)[:3]

# ---- LSE 合并 ----
max_gu = max_gu[..., 0:1].float()   # [N_gen, H, 1]
max_gg = max_gg[..., 0:1].float()
sum_gu = sum_gu[..., 0:1].float()
sum_gg = sum_gg[..., 0:1].float()

m     = torch.maximum(max_gu, max_gg)
alpha = torch.exp(max_gu - m)
beta  = torch.exp(max_gg - m)

numerator   = alpha * y_gu.float() * sum_gu + beta * y_gg.float() * sum_gg
denominator = alpha * sum_gu + beta * sum_gg
y_gen_out   = (numerator / denominator).type_as(y_und)
```

三个 kernel 调用的输出约定：

| 调用 | 取用输出 | layout | 形状 |
| --- | --- | --- | --- |
| 块 1 | `[0]` | BNSD | `[B, H, N_und, D]` |
| 块 2 | `[:3]` | TND | `y[N_gen,H,D]`, `max/sum[N_gen,H,8]` |
| 块 3 | `[:3]` | TND | 同上 |

注意 `max/sum` 最后一维是 8（kernel 内部对齐），代码只取 `[..., 0:1]`。

### 1.3 硬件背景：辅助统计量是"不可导"的

这是整篇分析的地基，必须先确认。

`npu_fusion_attention` 返回 4 个输出：

```
attention_out, softmax_max, softmax_sum, softmax_out
```

其中 `softmax_max` / `softmax_sum` 是 kernel 在 forward 里**为了不做二次遍历而顺手存下来的中间统计量**（算子融合的副产物），torch_npu 通常把它们标记为**不可导**（`output_differentiability = [True, False, False, False]`）。

**后果**：代码里 `alpha * y_gu * sum_gu` 这种写法，autograd 只会沿着 `y_gu` 回传，**不会**沿着 `sum_gu` / `max_gu` 回传。也就是说，`Z_A`、`m_A` 在反向里被当作**常数**。

这一点是否成立，直接决定第 4 章的推导。一行代码可以验证：

```python
out, mx, sm, _ = torch_npu.npu_fusion_attention(q, k, v, H, input_layout="BNSD")
print(mx.requires_grad, sm.requires_grad)   # 期望 False / False
```

如果实测是 `False`，那么第 4 章的结论成立；反之整个分析需要重做。

### 1.4 一个重要的前提说明

**本推导只依赖一个结构事实**：gen 行能看到的 key 集合被**划分**成两个不相交的子集 `U`（und 列）和 `G`（gen 列），两个 kernel 各自在**自己的子集内部**做了一次独立的 softmax，然后用 LSE 合并。

它**不依赖**每个块内部是因果还是双向、不依赖 TND 打包成几段、不依赖 mask 模板的具体形状。所以无论右下块是下三角（因果）还是全可见（双向），第 4 章的误差项形式都一样。

（关于右下块 `gen_mask_gg` 传全 0、因果性从何而来，见第 9.2 节待确认清单——那是正向正确性的问题，与本文的反向问题相互独立。）

---

## 第 2 章 · 符号定义与统计量约定

为简洁，下面固定**某一行 gen query、某一个 head**来讨论（其余行 / 头完全独立、同理）。行下标 `i` 省略，只在需要区分"别的行"时显式写出。

### 2.1 基本量

| 符号 | 含义 |
| --- | --- |
| $q$ | 该行的 query 向量，$\\mathbb{R}^{D}$ |
| $U$ | und 的 key 下标集合，$\\lvert U\\rvert = N_{und}$（或滑窗内的可见 und） |
| $G$ | gen 的 key 下标集合中**该行可见**的部分 |
| $k_j, v_j$ | 下标 $j$ 的 key / value 向量 |
| $s_j = \\text{scale}\\cdot q\\cdot k_j$ | attention score（scale 已含在 `self.scaling`） |
| $D$ | head_dim |

### 2.2 真值（不切分时的完整 softmax）

$$m_{all}=\max_{j\in U\cup G}s_j,\qquad Z=\sum_{j\in U\cup G}e^{\,s_j-m_{all}}$$

$$p_j=\frac{e^{\,s_j-m_{all}}}{Z}\quad(j\in U\cup G),\qquad y=\sum_{j\in U\cup G}p_j v_j$$

$y$ 就是**不切分**时这一行的输出，也是切分方案必须复现的目标。

### 2.3 分块统计量

块 A = 左下（gen@und，key 集合 $U$）：

$$m_A=\max_{j\in U}s_j,\qquad Z_A=\sum_{j\in U}e^{\,s_j-m_A},\qquad y_A=\sum_{j\in U}\frac{e^{\,s_j-m_A}}{Z_A}v_j$$

块 B = 右下（gen@gen，key 集合 $G$）：

$$m_B=\max_{j\in G}s_j,\qquad Z_B=\sum_{j\in G}e^{\,s_j-m_B},\qquad y_B=\sum_{j\in G}\frac{e^{\,s_j-m_B}}{Z_B}v_j$$

合并用到的中间量（代码里的 `m / alpha / beta / denominator`）：

$$m=\max(m_A,m_B),\qquad \alpha=e^{\,m_A-m},\qquad \beta=e^{\,m_B-m}$$

$$Z=\alpha Z_A+\beta Z_B\qquad\text{（= 代码的 `denominator`）}$$

### 2.4 kernel 内部概率 $p^{A}_j$ 与真值 $p_j$ 的换算（后面反复用到）

注意 $(k_A,k_B)$ 用局部峰值 $m_A$ 归一化，而 $p_j$ 用全局峰值 $m_{all}$ 归一化。两者差一个指数平移：

$$e^{\,s_j-m_A}=e^{\,s_j-m}\cdot e^{\,m-m_A}=\frac{e^{\,s_j-m}}{\alpha}$$

$$\Longrightarrow\quad p^{A}_j\;\equiv\;\frac{e^{\,s_j-m_A}}{Z_A}=\frac{1}{\alpha Z_A}\cdot\frac{e^{\,s_j-m}}{1}\cdot Z\cdot\frac{1}{Z}=\frac{Z}{\alpha Z_A}\,p_j$$

记

$$\boxed{\,c_A\equiv\frac{\alpha Z_A}{Z}\,}\qquad\Longrightarrow\qquad p^{A}_j=\frac{p_j}{c_A}$$

其中 $c_A$ 有一个非常自然的解释：**块 A 在全局 softmax 里占的"注意力质量"**（即 $\sum_{j\in U}p_j=c_A$，因为 $\sum_{j\in U}e^{s_j-m}=\alpha Z_A$）。同理 $c_B=\beta Z_B/Z=1-c_A$。

这个 $c_A$ 会在第 4 章反复出现——它既是合并节点给出的链式因子，又恰好是局部归一化的倒数。

### 2.5 关键差异：kernel 反向时用什么

**这是全篇最核心的一句话：**

> `npu_fusion_attention` 的 backward 并不知道"我还被别的块补充了 key"。它拿 forward 时留下的 $(m_A, Z_A)$ 去**重算**概率 $P^{A}_{ij}=e^{s_{ij}-m_A}/Z_A$，并把自己 forward 的输出 $y_A$ 当作"这一行的最终输出"来算 softmax 雅可比。

即，官方 backward 计算的是

$$dS^{split}_j = P^{A}_j\Big(\underbrace{dy_A\cdot v_j}_{\text{直接项}}-\underbrace{dy_A\cdot y_A}_{\text{与"所有" key 竞争项}}\Big)$$

而真值要求的是

$$dS^{true}_j = p_j\big(dy\cdot v_j-dy\cdot y\big)$$

**两者的差别就在第二个括号里 —— 用 **$y_A$** 还是用 **$y$**。** 第 4 章会把这个差别精确推出来。

---

## 第 3 章 · 正向推导：LSE 合并是恒等变形

### 3.1 第一个恒等式：合并峰值就是全局峰值

$$m=\max(m_A,m_B)=\max\Big(\max_{j\in U}s_j,\ \max_{j\in G}s_j\Big)=\max_{j\in U\cup G}s_j=m_{all}$$

这一步不需要任何条件，只要 $U\cup G$ 恰好是完整 key 集合、且 $U\cap G=\varnothing$。**它是整个正向成立的第一步。**

### 3.2 分子展开

$$\text{numerator}=\alpha Z_A y_A+\beta Z_B y_B$$

分别展开：

$$\alpha Z_A y_A=\alpha Z_A\sum_{j\in U}\frac{e^{\,s_j-m_A}}{Z_A}v_j=\sum_{j\in U}\alpha\,e^{\,s_j-m_A}v_j=\sum_{j\in U}e^{\,s_j-m_A+\ln \alpha}v_j$$

由 $\ln\alpha=m_A-m$：

$$=\sum_{j\in U}e^{\,s_j-m}v_j$$

同理

$$\beta Z_B y_B=\sum_{j\in G}e^{\,s_j-m}v_j$$

所以

$$\boxed{\ \text{numerator}=\sum_{j\in U\cup G}e^{\,s_j-m}v_j=Z\cdot y\ }$$

直觉：$\alpha Z_A y_A$ 这个乘积**取消了局部归一化**，还原成"未归一化的局部权重和"，再乘 $\alpha$ 只是把指数基准从 $m_A$ 换成 $m$。

### 3.3 分母展开

$$\text{denominator}=\alpha Z_A+\beta Z_B=\sum_{j\in U}e^{\,s_j-m}+\sum_{j\in G}e^{\,s_j-m}=\sum_{j\in U\cup G}e^{\,s_j-m}=Z$$

### 3.4 结论

$$y_{\text{gen\_out}}=\frac{\text{numerator}}{\text{denominator}}=\frac{Z\cdot y}{Z}=y$$

**正向与不切分完全等价，是代数恒等，不是近似。** 唯一的误差来源是 `.float()` / `.type_as()` 的精度转换，量级 \~1e-7（fp32）或取决于最终 dtype，可忽略。

### 3.5 为什么"正向没问题"会误导人

正因为正向是恒等变形，你会很自然地认为：**既然前向等价，反向自动等价**。但反向的等价性需要另一个前提——**反向 kernel 必须是"这个复合函数"的梯度**。而它并不是：它只是"独立 softmax attention"这个函数的梯度，它对 $m_A,Z_A$ 的内部使用方式是硬编码的，你没法通过 autograd 把"我还被另一个块补了 key"这件事告诉它。

**正向恒等 + 反向不恒等**，这两件事同时为真，正是本 bug 隐蔽的原因：loss 曲线前几步看起来正常，梯度已经在慢慢偏。

---

## 第 4 章 · 反向推导（核心）

### 4.1 合并节点传下去的梯度因子

合并表达式：

$$y_{out}=\frac{\alpha Z_A y_A+\beta Z_B y_B}{\alpha Z_A+\beta Z_B}=\frac{\alpha Z_A y_A+\beta Z_B y_B}{Z}$$

设最终 loss 对该行输出的梯度为 $dy\equiv\partial L/\partial y_{out}\in\mathbb{R}^D$。

因为 $\alpha,\beta$ 是 $m_A,m_B$ 的函数而 $m_A,m_B$ **不可导**（1.3 节），$Z_A,Z_B$ 同样不可导，$Z$ 因此也视为常数。于是

$$\boxed{\ \frac{\partial L}{\partial y_A}=dy\cdot\frac{\alpha Z_A}{Z}=dy\cdot c_A\ },\qquad c_A=\frac{\alpha Z_A}{Z}$$

（$c_A$ 就是 2.4 节定义的那个量。）同理 $\partial L/\partial y_B=dy\cdot c_B$，$c_B=1-c_A$。

下面记

$$dy_A\equiv\frac{\partial L}{\partial y_A}=c_A\,dy,\qquad dy_B\equiv c_B\,dy$$

**注意 **$dy_A$** 已经带上了一个小于 1 的因子 **$c_A$**（块 A 的注意力质量）**，这正是后面 V 的梯度能对上的原因。

### 4.2 V 的梯度 —— 碰巧是对的

`npu_fusion_attention` 的 backward 在算 $dV$ 时，用保存的 $(m_A,Z_A)$ **重算**概率：

$$P^{A}_j=\frac{e^{\,s_j-m_A}}{Z_A}=p^{A}_j=\frac{p_j}{c_A}\qquad\text{（2.4 节）}$$

然后执行

$$dV_j=\sum_i P^{A}_{ij}\,\big(dy_A\big)_i=\sum_i p^{A}_{ij}\,c_A\,dy_i$$

把 $p^{A}_{ij}=p_{ij}/c_A$ 代进去，$c_A$ **约掉了**：

$$\boxed{\ dV_j=\sum_i \frac{p_{ij}}{c_A}\cdot c_A\,dy_i=\sum_i p_{ij}\,dy_i\ }\qquad\checkmark$$

这正是真值 —— 完整 softmax 的 $\partial L/\partial v_j=\sum_i p_{ij}\,dy_i$。

> **结论：块 A 和块 B 的 V 梯度都是精确正确的。**

为什么这么巧？因为 $dV=P^{\top}dY$ 里只用到概率本身、不涉及"归一化基准"这件事；而链式因子 $c_A$ 与局部归一化倒数 $1/c_A$ 天然互为倒数。**误差被结构性地抵消了。**

### 4.3 Q / K 的梯度 —— 错了

Q/K 的梯度要经过 score 的梯度 $dS$。完整 softmax 的雅可比为

$$dS^{true}_{j}=p_j\Big(dy\cdot v_j-\sum_{k\in U\cup G}p_k\,(dy\cdot v_k)\Big)$$

第二个求和中 $\sum_k p_k v_k=y$，所以

$$\boxed{\ dS^{true}_{j}=p_j\big(dy\cdot v_j-dy\cdot y\big)\ }\qquad(\forall j\in U\cup G)$$

而 kernel A 的 backward 做的是一模一样的推导，只不过它认为"这一行只有 $U$ 这些 key"：

$$dS^{split}_{j}=P^{A}_j\Big(dy_A\cdot v_j-dy_A\cdot o_A\Big),\qquad o_A=\text{kernel A 自己 forward 的输出}=y_A$$

代入 $P^{A}_j=p_j/c_A$、$dy_A=c_A dy$：

$$dS^{split}_{j}=p_j\,dy\cdot v_j-p_j\,\underbrace{c_A\,dy\cdot y_A}_{\text{注意这里}}$$

第一项 $p_j/c_A\cdot c_A\,dy\cdot v_j=p_j\,dy\cdot v_j$ ✓ 与真值一致。第二项 $P^{A}_j\cdot dy_A\cdot y_A=p_j\,dy\cdot y_A$，\*\*用的是 $y_A$ 而不是 \*\*$y$。

于是

$$\boxed{\ dS^{split}_{j\in U}=p_j\big(dy\cdot v_j-dy\cdot y_A\big)\ }$$

$$\boxed{\ \text{误差}\quad \Delta_j\equiv dS^{true}_j-dS^{split}_j=p_j\,dy\cdot\big(y_A-y\big)\ }$$

块 B 同理，只是把 $y_A$ 换成 $y_B$：

$$dS^{split}_{j\in G}=p_j\big(dy\cdot v_j-dy\cdot y_B\big),\qquad \Delta_j=p_j\,dy\cdot(y_B-y)$$

### 4.4 误差项的物理含义

把第二项拆开看：

$$\underbrace{p_j\,dy\cdot y}_{\text{应该减去的}} \quad\text{vs}\quad \underbrace{p_j\,dy\cdot y_A}_{\text{实际减去的}}$$

softmax 反向里的 $-p_j\sum_k p_k(dy\cdot v_k)=-p_j\,dy\cdot y$ 这一项，语义是"**这个 token 要和所有 key 竞争**"。

分块之后：

- kernel A 只知道 $U$ 这一半的 key，于是它减掉的是**只在 und 内部平均**的 $y_A$；
- 另一块的 key（以及它们占的注意力质量）**完全没参与竞争**。

**这就是 bug 的本质：分块破坏了 softmax 的"全局竞争"语义。** 而 $dy\cdot(y-y_A)$ 一般不是小量 —— $y-y_A$ 是一个完整的 $D$ 维向量差，量级与 $y$ 本身同阶。

另一种等价的写法（用权重表达更直观）：

$$y-c_A y_A-c_B y_B=\text{（各块输出相对于全局输出的偏离）}$$

$(dy\cdot)(\cdot)$ 把两个块的输出差异直接投影到梯度方向上。**只要两块输出不同（几乎必然），误差就非零。**

### 4.5 一个容易被忽略的细节：块 B 的 $y_B$ 也是错的

有人会想："那么块 B 的误差可以用块 A 的输出来补" —— 不行。$dS^{split}_{j\in G}$ 里减的是 $y_B$，误差是 $p_j dy\cdot(y_B-y)$，**和块 A 的误差方向不同、量级不同，不会自动抵消**。

两块误差要抵消，需要

$$\sum_{j\in U}\Delta_j+\sum_{j\in G}\Delta_j=0$$

但这一般不成立（见第 5 章的行和检验）。

### 4.6 由此传染到 dQ / dK

一旦 $dS$ 错了，下游全错：

$$dQ_i=\text{scale}\cdot\sum_{j}dS_{ij}\,k_j,\qquad dK_j=\text{scale}\cdot\sum_{i}dS_{ij}\,q_i$$

- $dQ$：gen 行的 query 同时从块 A 和块 B 拿梯度，**两块各自带着自己的偏差**；
- $dK_{und}$：只从块 A 拿，但 $dS$ 已经错了 → 错；
- $dK_{gen}$：只从块 B 拿 → 错；
- $dV$：**对**（4.2 节）。

**所以：V 的梯度对，Q/K 的梯度错。** 这个"V 对 Q/K 错"的指纹非常有辨识度，第 7 章就是拿它来做验证。

---

## 第 5 章 · 自检不变量与误差量级

### 5.1 softmax 雅可比的行和必须为 0

**不变量**：给一整行的 score 加上同一个常数 $c$（$s_j\mapsto s_j+c,\ \forall j$），softmax 概率完全不变，输出完全不变。因此

$$\sum_{j\in U\cup G}\frac{\partial L}{\partial s_j}=\sum_j dS_j=0$$

**真值检验**：

$$\sum_j dS^{true}_j=\sum_j p_j\,dy\cdot v_j-\Big(\sum_j p_j\Big)\,dy\cdot y=dy\cdot y-dy\cdot y=0\quad\checkmark$$

**分块检验**：记 $c_A=\sum_{j\in U}p_j$（块 A 的总注意力质量，2.4 节），$c_B=1-c_A$。则

$$\begin{aligned}
\sum_{j\in U}dS^{split}_j+\sum_{j\in G}dS^{split}_j
&=\underbrace{\sum_{j\in U\cup G}p_j\,dy\cdot v_j}_{=\,dy\cdot y}
-\sum_{j\in U}p_j\,dy\cdot y_A-\sum_{j\in G}p_j\,dy\cdot y_B\\[2pt]
&=dy\cdot y-c_A\,dy\cdot y_A-c_B\,dy\cdot y_B\\[2pt]
&=\boxed{\ dy\cdot\big(y-c_A y_A-c_B y_B\big)\ \neq 0\ }
\end{aligned}$$

因为 $y=c_A y_A+c_B y_B$（合并的定义），这个式子看似该为 0 —— 但$c_A$** 恰恰是不可导的那个 **$\alpha Z_A/Z$，而 $\sum_{j\in U}p_j$ 虽然数值上等于 $c_A$，在 split 的计算图里它们却是**两条不同的路径**。更直接地说：split 版本根本不存在"整行加常数"这个对称性，因为它把一行拆成了两个独立的 softmax。



> **这就是最干净的指纹：分块后，每行的 **$dS$** 之和不再为 0**

数值上这个残差就等于 $dy\cdot(y-c_Ay_A-c_By_B)$，是 $O(1)$ 的量。

### 5.2 误差的更紧凑形式

由 $y=c_Ay_A+c_By_B$ 得

$$y_A-y=y_A-c_Ay_A-c_By_B=c_B\,(y_A-y_B)$$

代回 4.3 节的误差式：

$$\boxed{\ \Delta_j=p_j\,c_B\,dy\cdot\big(y_A-y_B\big)\quad(j\in U)\ }$$

$$\boxed{\ \Delta_j=p_j\,c_A\,dy\cdot\big(y_B-y_A\big)\quad(j\in G)\ }$$

**这个形式把误差的三个因子拆得很清楚**：

| 因子 | 含义 | 何时为 0 |
| --- | --- | --- |
| $p_j$ | 该 key 的概率权重 | 只在极端尖锐的 softmax 下小 |
| $c_B$ / $c_A$ | **另一块**占的注意力质量 | 另一块几乎不被注意时 → 0 |
| $dy\\cdot(y_A-y_B)$ | 两块输出的差在梯度方向上的投影 | 两块输出恰好一致，或与 $dy$ 正交 |

### 5.3 误差何时退化消失

1. **一块绝对主导**：若 $\beta\to0$（块 B 的峰值远低于块 A），则 $c_B\to0$、$y\to y_A$，误差 →0。这在实践中可能发生，但**不可依赖**；而且如果模型"学会"了让某一块主导来回避这个误差，反而是一种被 bug 诱导出来的退化行为。
2. **两块输出恰好一致**：$y_A=y_B$ 是退化的巧合，一般不成立。
3. **正交**：$dy\perp(y_A-y_B)$ 同样不可依赖。

反之，**误差最大的情形恰恰是最常见的**：两块注意力质量相当（$c_A\approx c_B\approx0.5$）、且两块输出差异明显。对 und 是长文本、gen 是要生成的图像 token 的 T2I 场景，两块都很重要，通常落在"误差大"的区域。

### 5.4 误差是系统性偏差，不是数值噪声

- 它不是浮点舍入（那是 \~1e-7），而是**函数本身梯度不对**，$O(1)$ 量级；
- 它不会随 batch 平均而消失——同方向的偏置会**累积**；
- 它**不会随训练自然收敛掉**：优化器会老老实实沿着错误的方向下降，模型确实会收敛，但是收敛到"这个错误梯度场"的极小点，而不是真实 loss 的极小点。

**现象学表现**（供对照排查）：loss 能正常下降（前向等价），但收敛质量、生成指标、以及 attention 分布（尤其是 gen→und 与 gen→gen 的权重比）与不切分版本对不齐；梯度范数、参数更新方向与参考实现的差异无法用精度解释。

---

## 第 6 章 · 影响范围矩阵

### 6.1 三个块各自的梯度正确性

| 块 | dQ | dK | dV |
| --- | --- | --- | --- |
| 块 1 · und@und | ✅ 完整 softmax | ✅ | ✅ |
| 块 2 · gen@und | ❌ | ❌（$k_{und}$） | ✅ |
| 块 3 · gen@gen | ❌ | ❌（$k_{gen}$） | ✅ |

推导依据：

- **dV**：4.2 节已证 $dV_j=\sum_i p_{ij}dy_i$，与真值完全一致 —— 链式因子 $c_A$ 与局部归一化 $1/c_A$ 精确互消；
- **dS → dQ/dK**：4.3 节的误差项 $\Delta_j$ 污染所有经过 score 的路径。

### 6.2 传播到各张量的梯度

| 张量 | 来源 | 是否正确 |
| --- | --- | --- |
| $dq_{und}$ | 块 1 | ✅ |
| $dk_{und}$ | 块 1 ✅ + 块 2 ❌ | ❌ |
| $dv_{und}$ | 块 1 ✅ + 块 2 ✅ | ✅ |
