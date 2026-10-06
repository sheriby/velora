# 选中菜单与右键菜单（对齐 Typora）

日期：2026-10-05
状态：FP1–FP13 全部落地，§5 逐条记了实现位置与验收用例。剩 FP4b-4（段落转换的标注那一档）留档不做：标注自己带头部与子块，换出去要先安置子块，代价见 §6 那两条；今天的事实是 `BlockKindTarget` 里没有标注这一档（「段落」那十二行不含它），标注块自己在 `next_kind` 的拒绝名单里（src/editor/paragraph_ops.rs:335，经 `block_kind_conversion` :267 供置灰与派发共用），这一条由既有的 `paragraph_kind::a_callout_is_left_alone`（src/editor/tests/paragraph_kind.rs:239）钉住换标题与换正文两条。

## 1. 这一期要解决的问题

现在的状况：

- 选中一段文字后没有任何浮动的格式工具栏，只能靠键盘快捷键。
- 正文里右键只有一个「插入 › 表格」，段落转换、行内格式、剪贴板操作都进不来。

目标：正文的选中菜单和右键菜单做到 Typora 同级——常用格式、段落转换、插入、剪贴板都在鼠标够得着的地方；界面保持一行到两行、分节清楚、按使用频率排序，不堆按钮。

## 2. 现状盘点（带位置）

菜单的渲染方式全部是手写的 gpui `div` 浮层，没有用 `PopoverMenu`：

- 正文右键的入口：`Editor::on_editor_context_menu_mouse_down`（背景区）与 `on_block_context_menu_mouse_down`（图片块 / 可插入块，表格单元格跳过），`src/editor/context_menu.rs:276`、`:289`，挂在 `src/editor/render/paint.rs:329,373,423,476,574`。
- 菜单状态：`Editor::context_menu: Option<ContextMenuState>`（`src/editor/mod.rs:280`），`ContextMenuState` 三个变体 `Insert / TableAxis / Image`（`src/editor/context_menu.rs:23-45`）。
- 关闭方式：全屏遮罩吃左键（`render.rs:172`）、动作处理者自己关、Esc 走 `DismissTransientUi` → `on_dismiss_transient_ui`（`context_menu.rs:341`），优先级：模态 > 提示框 > 菜单栏 > 右键菜单 > 快速打开。
- 层级：`render/paint.rs:1042-1092`，右键菜单在表格对话框之下、模态在更上层。
- 行渲染：只有 `render_axis_menu_item`（`context_menu/render.rs:48`），参数是 `(theme, id, label, enabled, danger, fn 指针)`，没有快捷键文字列、没有分隔线、没有子菜单支持；子菜单是写死的一行「表格」（`render.rs:123-162`）。文件树菜单、标签菜单、菜单栏各自又抄了一份行渲染，共约 4 份，分隔线抄了 5 份。
- 尺寸令牌已有：`menu_text_size 12 / menu_panel_padding 4 / menu_panel_gap 1 / menu_panel_radius 8 / menu_item_height 28 / menu_item_padding_x 8 / menu_item_radius 5 / context_menu_panel_width 132 / submenu_width 148`（`src/theme/theme/de.rs:535-550`）。
- 文案位点 5 个文件：`src/i18n/strings.rs`（字段）、`strings_api.rs`（中文 `:290` 一段、英文 `:630` 一段）、`keys.rs`（语言包键名表）、`de.rs`（`Option` 字段）、`de_impl.rs`（回退）。

可复用的编辑动作：

- 行内格式：`Block::toggle_inline_format(InlineFormat)`（`src/components/block/runtime/text_ops.rs:97`），`InlineFormat` 目前只有 `Bold / Italic / Underline / Code`（`runtime/mod.rs:39`），且**只作用于焦点块自己的 `selected_range`**，跨块选区直接返回。模型层 `InlineTextTree::toggle_*`（`src/components/markdown/inline/tree.rs:824-843`）都走私有的 `toggle_style`（`:989`）；`toggle_strikethrough` 已存在但标了 `#[allow(dead_code)]`。
- 上下标：`^x^`、`~x~` 能解析（`inline/delimiters.rs:11-16`，`StyleFlag::Superscript/Subscript`），但没有从界面进去的路径。
- 高亮 `==x==`：整个仓库没有，分隔符、样式位、toggle 都要新增。
- 链接：`Block::paste_url_as_link(url, window, cx)`（`interactions/keys.rs:146`）写字面 `[选中](url)`。
- 段落转换：**没有** `Heading1..6`、列表、引用、代码块、分隔线的显式动作。现在只能靠敲前缀：`BlockKind::detect_markdown_shortcut`（`state.rs:375`）+ `normalize_after_title_edit`（`runtime/normalize.rs:10`）。显式改类型的只有 `convert_to_paragraph`、`convert_to_separator`、`enter_code_block(language)`、`enter_math_block(body)`（`text_ops.rs:13/21/49/73`）。
- 插入：表格走对话框 `insert_table_from_dialog`（`context_menu.rs:477`）；图片只有粘贴/拖拽（`events/paste.rs:345/369/429`）。缺链接输入、图片选择器、目录（`[toc]` 只能渲染 `tree.rs:243`）、Front Matter（`state.rs:161` 只解析）。
- 剪贴板与选区：块级 `on_copy/on_cut/on_paste/on_select_all`（`interactions/keys.rs:124/134/177/93`），跨块捕获 `on_copy_capture/on_cut_capture`（`src/editor/selection.rs:137/151`）、`select_all_rendered_document`（`:233`）；读选区用 `Block::selected_range`、`Editor::cross_block_selection`（`editor/mod.rs:285`）→ `normalized_cross_block_selection`（`selection.rs:434`）、`selected_markdown_text`（`:826`）。取焦点块：`current_edit_target_from_state`（`runtime_context.rs:53`）。选区屏幕位置：`Block::active_range_or_cursor_bounds`（`text_ops.rs:487`）。
- 拷贝为 HTML 已有：`Editor::copy_as_html`（`workspace/documents.rs:373`），⌘⇧C。缺「拷贝为 Markdown」「粘贴为纯文本」「清除格式」。
- 撤销分组：一次界面动作要用 `prepare_undo_capture(UndoCaptureKind::NonCoalescible)` 开组，缓冲区写入一律经 `record_buffer_edit`，收尾 `finalize_pending_undo_capture`（`src/editor/history.rs:120/146/172`）。这是不变式：漏了开组，撤销坐标会错位。

## 3. Typora 的对应关系

Typora 选中文字时浮出一行图标工具栏（粗体、斜体、删除线、行内代码、高亮、上标、下标、链接、清除格式），右键菜单分四段：编辑类（撤销/重做、剪切/拷贝/粘贴/全选）、格式类、段落类、插入类，另外有「拷贝为 Markdown / 拷贝为 HTML 纯文本 / 切换源代码模式」。

对齐结论（左边是本期要做，右边是不做的理由）：

| 本期实现 | 暂不做 |
| --- | --- |
| 选中栏：粗体、斜体、删除线、行内代码、高亮、链接、清除格式 | 选中栏放图片、公式、表格：这些是插入类动作，Typora 也放在右键/菜单里；选中栏放得越满越难指准 |
| 右键 段落：标题 1-6、正文、引用、无序列表、有序列表、任务列表、代码块、分隔线 | 段落子菜单的「提升/降低层级」：快捷键 ⌘[ ⌘] 已有价值，但本期先把基础项铺完 |
| 右键 格式：以上行内格式 + 上标 + 下标 + 清除格式 | 下划线 `<u>`：模型有 `Underline`，但 markdown 语义弱，键盘入口已有 |
| 右键 插入：表格、代码块、图片、链接、公式块、分隔线、目录、Front Matter | 脚注插入：仓库现在没有脚注渲染，做入口等于半个新功能 |
| 右键 编辑：撤销、重做、剪切、拷贝、粘贴、粘贴为纯文本、全选、拷贝为 Markdown、拷贝为 HTML 纯文本、切换源代码模式 | 「查找/替换」入口：面板已有 ⌘F 与侧栏 |

两处落位与上表不同，按实现记下来：「链接」在格式那一档（它改的是选中那几字的写法，与 ⌘K 同一条实现，FP5b-1 记的就是「格式 → 链接」），「分隔线」在插入那一档（那是插一块新内容，不是换这一段的种类）。两条判定的来源也不同：段落那十二行走 `BlockKindTarget::next_kind`（src/editor/paragraph_ops.rs:332），插入那一档走 `insert_block_target_is_available`（src/editor/insert_ops.rs:34）。「插入 → 代码块」与「段落 → 代码块」两条并存也是这个分别：前者在当前光标处多一块新的，后者把眼下这一段围栏起来。行序的可核对处：插入那一档七行在 src/editor/context_menu/document_menu.rs:360-392（表格、图片、代码块、公式块、分隔线、目录、Front Matter，没有链接），段落那一档十二行由 `PARAGRAPH_ROWS` 把守（src/editor/tests/document_context_menu.rs:94，含 `code-block`）。

## 4. 界面与交互约定

选中菜单：

- 一行，横向排布，方形按钮 26×26、间隙 2、内边距 4，圆角与描边沿用主题的 `menu_item_radius` 与 `dialog_border`，颜色 `dialog_secondary_button_text`，悬停 `dialog_secondary_button_hover`，按下 0.92 透明度反馈（与标题栏按钮同一写法）。格子里放符号与图标，不放词：B 加粗、I 斜体、U 下划线、S 盖一条横线、`</>` 行内代码、A 底下一道荧光笔色、A 右上角一枚小 `×` 清除格式；「段落」与「链接」是两枚描出来的 svg（`icon/editor/paragraph.svg`、`icon/editor/link.svg`，16 的框、1.5 的描边，与 `icon/workspace/*` 同一套画法）——「段落」那颗没有通行的字母可写，把中文词塞进一排字母里会读成一句话（用户报修：「段落不应该是汉字，应该是图标」）。三截之间各放一条 1px 分节线。字母字号 13.5（比菜单正文 12 大一档，才与旁边 16 的图标一样抢眼），`</>` 收小到 11 才塞得进 26 的方格。动作的全名与快捷键一律在悬停说明里。
- 位置：选区外接框的上方 8px；上方放不下就翻到下方 8px；左右按视口宽度夹紧（沿用 `context_menus.rs:99-107` 的 8px 夹紧写法）。
- 出现时机：鼠标或键盘产生非空选区后出现；选区塌成光标、切到源码模式、右键菜单或任何浮层开着时不出现。拖动的中间过程不浮出（`cross_block_drag` 还开着就按住），只在抬手之后定位一次。滚动时不做特殊处理：选区还在，面板就跟着选区重算一次位置（比原地钉住准），整段滚出视口才收起。「焦点离开文档就收掉」这条没做：这个应用一篇一个 Editor 视图，选区在模型里留着、面板也就留着，窗口失焦时是否要藏起来涉及所有浮层的统一口径，不在这一笔。
- 工具栏本身不吃键盘焦点（gpui 里按钮按下不影响块的焦点），保证按下去之后 ⌘Z、方向键仍然作用在文档上。
- 每个按钮有悬停说明，写明中文名和快捷键。

右键菜单：

- 面板宽度按这一列里最宽的一行来定（`estimated_menu_label_width` 那套字符宽度估算，标题栏菜单本来就在用），下限取主题的 `context_menu_submenu_width`。定死 200 在中文下偏空、在英文下会把「Toggle Source View」这类长标签截掉；算出来的宽度同时给落点夹紧用，两处不会各自理解一遍面板尺寸。表格轴菜单、图片菜单仍用现有宽度。
- 分节用 1px 分隔线，顺序固定：编辑 → 格式 → 段落 → 插入 → 视图。段落与插入内部用二级子菜单，避免一个 30 行的长菜单。
- 每行右侧显示快捷键（灰 `dialog_muted`），没有快捷键的留空位对齐。
- 不可用的项（例如没有选区时的「复制」「清除格式」）置灰不隐藏，鼠标位置不变——菜单宽度变化会让连点两次右键跳位置。置灰只看这件事的前提在不在：剪切/拷贝看有没有一段选区，撤销/重做看历史，粘贴看剪贴板，格式与段落看这份文档能不能按区间写回。
- 右键时如果当前有选区，先保住选区（现在切到别的块会清空），菜单动作全部围绕这段选区。

## 5. 功能点拆分（按提交顺序）

每个功能点自己跑定向测试 + 全量 + clippy，写完就提交一笔，不攒。

FP1 菜单行渲染收口（已落地）。新增一个通用行渲染：`(id, label, shortcut: Option<&str>, enabled, danger, on_click)`，支持分隔线与「右箭头 + 子菜单」；把 `render_axis_menu_item`、文件树菜单、标签菜单、插入子菜单里那 4 份重复行代码和 5 份分隔线代码收过来。验收：现有 3 个菜单测试仍通过；新增一条测试断言同一段代码渲染出的行高、内边距、快捷键文字位置一致。

FP2 动作层：选区上的一行内格式（已落地）。给 `InlineFormat` 加 `Strikethrough / Superscript / Subscript`，接上 `InlineTextTree` 已有的 `toggle_style`（去掉 `toggle_strikethrough` 的 `dead_code`），并把 toggle 入口提到 `Editor`：`toggle_inline_format_on_selection(InlineFormat, cx)`（`src/editor/format_ops.rs`），跨块选区按块逐个处理，全程一个撤销组；块的 `toggle_inline_format_in_range` 收**可见文本**坐标，块内自己换算到树内坐标，Editor 层不必知道标记占位。上标/下标按本仓库既有写法落 `<sup>x</sup>` / `<sub>x</sub>`，与 Typora 的默认输出一致（Typora 也认 `^x^` / `~x~`，读入路径已有）。置灰要用的判定函数（选区能否做行内格式）随 FP6 一起进，这一笔没有消费者，进来就是警告。验收：单块、跨块、空选区三类用例。

FP3 高亮 `==x==`。新增 `StyleFlag::Highlight` + 分隔符解析 + 渲染颜色（用主题的强调色，浅色主题黄底、深色主题低饱和黄底），加 `InlineFormat::Highlight`。验收：解析往返测试（`==x==` 读进来 → 存出去不变形），toggle 用例，已有 markdown 测试无回归。

FP4a 动作层：段落转换的第一段——标题与正文。`Editor::apply_block_kind_to_selection(BlockKindTarget, cx)`（`src/editor/paragraph_ops.rs`），`BlockKindTarget = Heading(1..=6) | Paragraph`；对已经是这一级的标题再点一次等于取消。块的种类就地换（`Block::set_kind_in_place`，不发事件、不开撤销组），一次命令一个撤销组，字节按「前一块（仅当它的写法与文件一致）… 最后改到的那块」这一段区段写回，接缝空行由区段序列化按渲染态规则拼；引用与标注是容器、表/代码/公式是原子结构块，本段直接不动它们。验收：单块、跨块、取消、接缝、写回不碰邻居写法（Setext 夹具）五类用例。

FP4b-1 动作层：段落转换的第二段——列表（已落地）。目标补 `BulletList | NumberedList | TaskList`（`src/editor/paragraph_ops.rs`）。三条口径：记号跟着同族邻项抄（`+ ` 不被换成 `- `、`1)` 不被换成 `1.`，见 `list_marker_for_conversion`），没有同族邻项才用块自己记过的那份；任务项再点一次是去掉复选框退回无序项，不是取消整个列表；带子块的父项只在列表这一族内部换，换成标题或正文会拒（`block_kind_conversion`，序列化时子块的缩进层数跟着父块的种类走，换出这一族会把子块写成与父块平齐的行，块树还是父子、文件已经是两个根）。菜单的置灰与命令的实际行为读同一条判断（`block_kind_target_is_available` 调 `block_kind_conversion`），三个入口（快捷键、右键「段落」档、选中工具栏的下拉）共用这份行数据。验收：`src/editor/tests/paragraph_kind.rs` 里每种目标一条（缓冲区字节 + 根块种类 + 撤销一步回到原样），外加记号继承、序号接所在组、列表组中间换一项后重读字节得到同一结构、三段正文一次撤销、带子块的父项两种结果各一条；`document_context_menu.rs` 两条（点「任务列表」写回 `- [ ] `、在无序项上点「无序列表」取消记号）、`selection_toolbar.rs` 一条（工具栏下拉给出的行与右键菜单同源且写得动同样的字节）。

FP4b-2 动作层：段落转换的第三段——引用（已落地）。目标补 `Quote`（`src/editor/paragraph_ops.rs`）。解析器把引用建成一根根块、整段文字存在它自己的标题里（`> 甲\n> 乙` 是一块两行，`source_line_prefixes` 记每行让开几字节），所以换进换出动的就是这一块自己的字节，不需要动子块：`next_kind` 接 `Quote ↔ 正文`、`标题 / 列表 / 引用` 之间互换，再点一次「引用」等于取消引用。两条护栏：带子块的块仍只许在列表一族内部换（`block_kind_conversion`），跨两行的引用不许换成标题——`# 甲` 后面那行会被读回成另一块，块树一根、文件两根。落笔后 `written_line_ledger` 认不出引用这一族（只认正文 / 标题 / 列表项），那份记号宽度账清空，位置换算交回按文件量那一条，直到下次解析重新记。验收：`src/editor/tests/paragraph_kind.rs` 里正文 ↔ 引用（含再点一次取消）、跨两行的引用换正文不拆块、跨两行的引用换标题被拒且菜单那一行同步置灰、单行引用换标题不碰邻块、列表项换引用补那行空行、跨两块一次包成两块引用且写下的字节读回两块、一步撤销各一条；`document_context_menu.rs` 一条（点「引用」写回 `> `、再点一次取消、一步退回上一次）；`selection_toolbar.rs` 那条同源用例把 `quote` 行一并点名。

FP4b-3 动作层：段落转换的第四段——代码块（已落地）。目标补 `CodeBlock`（`src/editor/paragraph_ops.rs`）。正文 / 标题 / 列表 / 引用整块换进围栏，正文原样进围栏（`language` 给 `None`，那对围栏由 `safe_code_fence_with_info` 定长：正文里本来就有反引号围栏时外侧改用 `~~~`）；再点一次退回正文，围栏行与信息串一起收掉。两条口径：换出代码块只给「退回正文」这一条出路（`block_kind_conversion`），换成标题、列表或引用会把围栏内的多行原文按另一族的记号重写；`Block::set_kind_in_place` 的闸门从「raw 编辑模式一律不动」收窄到「只有 `SourceRaw` 不动」（src/components/block/runtime/text_ops.rs），代码块这一族因此换得出去，同时换出时把 `code_is_indented` 与 `source_fence_lines` 两本账清掉，免得下一次序列化还按缩进或围栏的形状写。验收：`src/editor/tests/paragraph_kind.rs` 里正文 ↔ 代码块（字节 + 块树 + 再点一次取消 + 一步撤销）、带语言写的围栏退回正文（信息串跟着围栏一起没掉，并把写下的字节重新读一遍比结构）、缩进写法的围栏退回正文（四格去掉、账上不留 `code_is_indented`）、代码块只给退回正文（标题 / 列表 / 引用三种目标既拒又置灰）、正文含围栏行时外侧改用波浪号围栏；`document_context_menu.rs` 一条（菜单点「代码块」补围栏；退回那一半按编辑器层入口验，因为代码块上弹不出这套菜单）；`selection_toolbar.rs` 一条（选中代码块里的文字，工具栏下拉那一行把围栏收掉）。

分割线不进「换种类」这一档：正文换成分割线要把那一行文字丢掉，破坏性动作不该藏在段落菜单里。它改由插入那一档给（FP5 的「插入分隔线」，`make_separator` 那条路径本来就不需要正文入参）。

FP4b-4 动作层：段落转换的第五段——标注（`> [!note]`）（本期不做）。标注自己带头部那颗变体记号（`callout_marker` 记用户写法）与正文子块，换进换出要把子块安置进根序列或收进容器，`source_separator_bytes` 那本空行账也要跟着重记；本期不做这一档：`BlockKindTarget`（src/editor/paragraph_ops.rs:14）没有标注这一档，「段落」那十二行里也就没有它；标注块自己在 `next_kind` 的拒绝名单里（:335，经 `block_kind_conversion` :267 同时供置灰与派发判定），所以既点不动也写不下——`paragraph_kind::a_callout_is_left_alone`（src/editor/tests/paragraph_kind.rs:239）钉住换标题与换正文两条都返回 false 且字节不动。补这一档时要写的验收：标注 ↔ 正文、标注换引用、多层嵌套标注各一条。

FP5a 动作层：插入类的第一段——五类块（已落地）。新增 `src/editor/insert_ops.rs`：`InsertBlockTarget = CodeBlock | MathBlock | Separator | Toc | FrontMatter`，入口 `Editor::insert_block_after_selection`（一条命令一个撤销组）与 `insert_block_target_is_available`（菜单置灰与命令实际行为同一条）。落点在光标所在根块之后（跨块选区落在整段选区之后），Front Matter 例外——解析器只认第一行那对 `---`，它必须顶到 0 位，接缝空行补在它后面。字节只在插入点那一处写：`接缝 + 新块的 markdown`，写完把插入点之后的根块区间整体挪位、给新块挂自己的区间（不含两边接缝），算不出来（锚点块没有源码区间，后台续建到一半）才交回整篇重投影。新块是代码块 / 公式块 / 分割线这类「光标走过去就出不去」的形状时，跟着 `ensure_trailing_paragraph_after_structural` 补一块空段落当退路。插完光标交给新块：代码块落在围栏里那一行、公式块落在开栏之后、Front Matter 落在两条 `---` 中间那一行、目录落在 `[toc]` 末尾、分割线停在自己那行。两处实际形状靠探针量出来：空代码块序列化是 ` ```\n\n``` ` 三行；空公式块不能写成 `$$\n\n$$`（解析器会把它切成两块原始 markdown），只有 `$$\n$$` 读回来还是一块。验收：`src/editor/tests/insert_blocks.rs` 九条——每类各一条（缓冲区字节 + 根块种类顺序 + 光标落点 + 一步撤销），五类各插一次把字节重新读一遍比块树，Front Matter 那份「只能在最前面、一篇只能有一份」（第二份既拒又置灰），跨块选区的落点，没碰过的那块 Setext 标题字节不动（钉住不是整篇重投影），以及右键菜单点「目录」与编辑器层入口同一条（六行的行名全点名）。菜单行：`插入` 那一档从一行变六行（表格 + 五类），行名 `insert-code-block`…`insert-front-matter`；文案 4 个键 × 5 处（`insert_math_block` 公式块 / Math Block、`insert_separator` 分隔线 / Thematic Break、`insert_toc` 目录 / Table of Contents、`insert_front_matter` Front Matter），代码块那一行复用段落档已有的 `paragraph_code_block`。

FP5b-1 动作层：链接（已落地）。选中那段字外面包一层 `[文字]()`，地址留空由用户当场写：落笔后光标停在 `](` 之后。三个入口收在同一条实现上——`Editor::insert_link_on_selection`（src/editor/insert_ops.rs）由 ⌘K（新增 `ShortcutCommand::LinkSelection`，默认键 `cmd-k`/`ctrl-k`）、右键菜单「格式 → 链接」、选中工具栏那颗 `[]()` 按钮共同调用；菜单那一行的置灰读同一条 `link_insert_is_available`。单块交给 `Block::wrap_visible_range_in_link`（src/components/block/runtime/text_ops.rs:147），沿块自己那条 `Changed` 写回字节；跨块按可见块逐段包（切片口径与 `toggle_inline_format_on_selection` 一致），全程只开一个 `NonCoalescible` 撤销组，光标交给第一段。工具栏因此从七项变八项，那颗方形按钮上的字就是它写下的写法（本仓没有图标字体）。菜单行名 `link`，排在「格式」那一档末尾、与八种行内样式之间隔一条分隔线；文案一个新键 × 5 处（`insert_link` 链接 / Link），偏好页的快捷键表用同一个键。刻意不做应用内地址输入气泡：本仓的文本输入通道只有工作区那一条（`OverlayInputKind` 配 `EntityInputHandler`，src/editor/workspace/overlay_input.rs），为链接单开一档要把焦点归属、撤销分组与浮层渲染各走一遍，代价大于收益；外壳与落点先做对，地址直接敲就是最短路径。验收：`src/editor/tests/link_insert.rs` 七条——单块包住的字节断言 + 光标落点 + 焦点留在改过那一块 + 块树不拆 + 一步撤销；只有光标时既点不动也写不动，并断言菜单那一行的置灰与 `link_insert_is_available` 同源；跨块两段各包一层且一步同时退回；`- ` 记号与另一块字节不动；⌘K、菜单行、工具栏按钮三个入口写的字节一样。

FP5b-2 动作层：图片（已落地）。选文件那一步走原生文件选择器（消息框被 `app_source_never_uses_native_prompts` 禁掉，原生选择器在放行名单里），选完交回一条公共入口 `Editor::insert_image_at_caret`（src/editor/insert_ops.rs）——拖放那条路原本自己拼「焦点块 → 切开前后两段 → 交粘贴」，现在收在这里，两边同一形状。图片本身的落盘与路径写法沿用粘贴那条路径（`handle_paste_image_request` → `pasted_image_markdown`）：磁盘上的图片先收进文档旁边的 `assets`，写下的行是 `![文件名](./assets/文件名.png)`，光标所在段落被切成「前面 / 图片行 / 后面」三块，其余字节不动。三个入口：`插入 → 图片`（行名 `insert-image`，排在表格之后，置灰读 `image_insert_is_available`）、⌘⇧I（新增 `InsertImage` 动作与 `ShortcutCommand::InsertImage`，默认键 `cmd-shift-i`/`ctrl-shift-i`）、以及拖放。刻意不做：应用内的图片预览与「复制图片到文档旁边」的开关——那是偏好项，不在菜单这一档。验收：`src/editor/tests/insert_image.rs` 三条——插在段落中间产生的块序与缓冲区字节（含别处 `__下划线__` 写法不动、图片确实被复制进 `assets`、一步撤销把三块一起放回去）、写下去的字节重新读一遍还是同一套结构且那行带图片运行时、菜单那一行的行序与置灰口径同源于 `image_insert_is_available`。选择器那一步用例不点：gpui 的测试壳把 `prompt_for_paths` 写成 `unimplemented!()`（vendor/gpui/src/platform/test/platform.rs:334），一点就 panic。

FP6 右键菜单成形（已落地）。`ContextMenuState::Insert` 换成 `Document`（`open_submenu`/`hovered_submenu` 两个 `Option<DocumentSubmenu>` 取代原来那三个 bool），内容拆到 `src/editor/context_menu/document_menu.rs`：`document_menu_rows` 给编辑 → 格式 → 段落 → 插入 → 视图五段，格式八项与段落六加一项走二级面板，插入那一档目前只有表格（其余插入项在 FP5）。行点击走 `run_document_menu_command`：撤销/剪切/拷贝/粘贴/切换视图派发 gpui 动作，行内格式与段落转换调编辑器层入口（FP2、FP4a 那两条），也就是快捷键动作处理器所调的同一组函数。面板尺寸与落点在 `DocumentMenuGeometry::measure` / `document_menu_origins`：宽度按最宽一行估，右侧放不下时二级面板翻到主面板左侧，下沿越界时向上收，离窗口边缘留 6px。快捷键那一列取 `default_shortcut_key`（默认键位；用户自定的绑定在 FP9 与偏好页一并接）。`Insert`→`Document` 之后 `context_menu_panel_width` 这个主题字段不再有消费者，仍留着给表格轴与图片菜单那条路。验收：`src/editor/tests/document_context_menu.rs` 从真实右键事件起，断言全部行 id、两种置灰口径的行序行高不变、二级面板与父行对齐、点「加粗」与「二级标题」改字节且撤销一步复原、贴边右键整个菜单留在视口内、快捷键列按默认键位显示；`document_menu::tests` 两条纯函数用例盯落点夹紧与面板宽度。

FP7 选中菜单（已落地）。新增 `Editor::selection_toolbar: Option<SelectionToolbarState>`（src/editor/mod.rs:287）与 src/editor/selection_toolbar.rs：锚点是 `selection_toolbar_anchor`（:109）——单块用块的 `selected_range`，跨块逐块量再并起来，为此在 Block 上把 `active_range_or_cursor_bounds` 里那一段量区间的算术拆成 `visible_range_bounds(range)`（src/components/block/runtime/text_ops.rs:555）；出现/消失口径见 §4。面板 7 项：「段落」下拉（六档标题 + 正文，行数据与右键菜单同一份 `document_submenu_rows`）加六颗行内格式按钮；按钮派发走 `run_selection_toolbar_command`（:523），调 FP2 与 FP4a 那两条编辑器层入口。落点 `toolbar_origin`（:163）：优先选区上方 8px、放不下改下方、横向居中并按视口收回，离边 6px；「段落」列表贴工具栏下沿，下方放不下贴到上沿（`heading_menu_offsets` :189）。本帧面板边界记在 `panel_bounds`，正文那层的按下落在里面时不当成正文落点（src/editor/selection.rs:81），否则一次按下先把选区收成光标、面板自己先消失。悬停说明复用 `HoverPreviewTooltip`，文字是「名字 + 默认键位」。验收：`src/editor/tests/selection_toolbar.rs` 九条，含真实拖动后浮出、拖动过程中不浮出、塌成光标收起、点加粗改字节且焦点与选区都不丢、点档位列表转标题且撤销一步复原、矮视口里不越界、与右键菜单互斥、源码模式不出、以及落点与离屏判定的两条纯函数用例。

FP8a 动作层：剪贴板的两条变体（已落地）。「粘贴为纯文本」把块的粘贴主体抽成 `Block::paste_from_clipboard(plain_only, window, cx)`（src/components/block/interactions/keys.rs），⌘V 传 `false`、⌘⇧V 传 `true`；`plain_only` 下不做两件事——不读粘贴板的 HTML 味道（跳过 `maybe_markdown_from_clipboard`），也不把「选中文字后粘一个网址」改写成链接（跳过 `paste_url_as_link` 那一条）。剪贴板里只有图片没有文字时仍走图片那一条：那是唯一能落的东西，按键没反应更让人以为坏了。「拷贝为 Markdown」是 `Editor::copy_as_markdown`（src/editor/workspace/documents.rs），取 `selected_markdown_text`（无选区时整篇源码，与「拷贝为 HTML」同一条口径）写进剪贴板的纯文本味道；与「拷贝」的差别在内容来源——那一条拿渲染后的可见文本（`**加粗**` 只剩「加粗」），这一条拿文件里的那几个字节。键位两条：`cmd-shift-v`/`ctrl-shift-v` 与 `cmd-shift-m`/`ctrl-shift-m`（都已核对不与现有绑定冲突）；菜单行 `paste-as-plain-text`、`copy-as-markdown` 排在主菜单编辑那组「粘贴」之后，两行都靠派发同一个动作到达，与键盘同一条路径。文案两个新键 × 5 处（`context_menu_paste_as_plain_text` 粘贴为纯文本 / Paste as Plain Text、`context_menu_copy_as_markdown` 拷贝为 Markdown / Copy as Markdown）；偏好页那两行先借这两个键，独立的 `preferences_shortcut_*` 键在 FP9 一并整。验收：`src/editor/tests/clipboard_text.rs` 六条——「粘贴」在选中的文字上粘网址要包成链接（对照）、「粘贴为纯文本」同一场景落的还是那几个字且一步撤销复原、「拷贝为 Markdown」给的是带 `**` 的源码、无选区时给整篇、菜单两行各一条等价用例（其中拷贝那条同时钉住文档字节一字不动）。

FP8b 动作层：清除格式（已落地）。剥掉选区里的行内成对标记（`**`、`__`、`*`、`_`、`` ` ``、`==`、`^`、`~`），保留反斜杠转义与块级记号；`InlineTextTree::clear_styles_in_range`（src/components/markdown/inline/tree.rs）按选区把片段切开，只在覆盖到的那一段上把八种行内样式一次清掉，切法与 `toggle_style` 一致；块层入口 `Block::clear_inline_format` 与 `clear_inline_styles_in_range`（src/components/block/runtime/text_ops.rs）沿用开关一种格式那套坐标换算（屏幕坐标 → `current_to_clean_range` → 树内坐标），什么也没剥到就返回 false，因此不留空的一步撤销。编辑器层把 `toggle_inline_format_on_selection` 的切块与记账抽成 `apply_inline_selection_edit`（src/editor/format_ops.rs），两种行为共用同一段：跨块逐块处理、只开一个 `NonCoalescible` 撤销组、一次撤销两块一起放回。菜单行 `clear-format` 排在「格式」那一档末尾（与链接同一组分隔线之后），置灰与八种行内样式同源（要有选区且写得动缓冲区）；文案 `format_clear` 清除格式 / Clear Formatting × 5 处。刻意不做：键位（当时判断 gpui 的键名表不认反斜杠，FP9b 实测这个判断是错的——`Keystroke::parse("cmd-\\")` 通过，测试里 `simulate_keystrokes("cmd-\\")` 也派发到键位，键位因此在 FP9b 补成 ⌘\ / Ctrl+\）；链接不动（那是结构不是样式）；粘贴进来的 `<span style=…>` 那一族 HTML 行内样式不动；选中工具栏那颗按钮当时没加（本仓没有图标字体，方格里放不下第二个形状），FP11 按 `A×` 那颗格子补上，口径见本节末的 FP11 一段。验收：`src/editor/tests/clear_format.rs` 六条——整段剥掉 `**`、只动选中的那一个字（两边仍粗体，序列化成 `**a**b**c**`）、行内代码与标记文本的成对记号一起剥、纯链接那一段既不改字节也不报改动、跨两块各剥一次且一步撤销同时放回、菜单那一行与编辑器层入口同一条。

FP9a 键位显示层：菜单与工具栏读生效键位（已落地）。原先的快捷键列取 `default_shortcut_key`（只有默认键），用户在偏好页改过绑定后那一列仍写默认键——写的与按的不是同一颗。现在 `install_keybindings` 在绑完键位的同时写一份 `EffectiveShortcuts`（src/components/actions.rs:918，命令 → 生效主按键，取 `normalize_shortcut_config` 之后那份，所以撞车而被退回默认键的命令，表里也是默认键）；`effective_shortcut_key(command, cx)`（:946）读它，`default_shortcut_key` 因此删掉。`document_menu_shortcut` 多收一个 `cx`（src/editor/context_menu/document_menu.rs:575），三个消费方一起换：右键菜单行（src/editor/context_menu/render.rs:655）、选中工具栏那颗按钮的悬停说明与「段落」下拉（src/editor/selection_toolbar.rs:404、:515）。面板宽度要跟着这串字走，`DocumentMenuGeometry::measure` / `row_width` 改由调用方交一份 `shortcut_of`（渲染路径给真实键位，src/editor/context_menu/render.rs:100 与 src/editor/selection_toolbar.rs:259；document_menu.rs 的两条纯函数用例给固定桩 :830），否则用户改成 `⌃⌥⌘⇧K` 那一类长串时会被截字。验收：`components::actions::tests::effective_shortcut_table_follows_installed_bindings` 三条口径（改过的命令读改成的键、撞车被退回的读默认键、没改过的仍是默认键）；`editor::tests::document_context_menu::the_shortcut_column_shows_the_users_own_binding` 从真实右键事件起，装一份把加粗改成 `cmd-alt-b` 的绑定，断言那一列渲染出 `⌥⌘B` 且 `⌘B` 不再出现在屏幕上。两条用例的改前红值都是把这份表退回默认键得到的：`Some("cmd-b") != Some("alt-cmd-b")`、`"⌘B" != "⌥⌘B"`。

FP9b 键位表补齐：标记文本与清除格式（已落地）。两条命令的动作与入口早就都在，缺的只是键位表里那一项：`HighlightSelection` 从 FP3 起就有块层与编辑器层两条处理器（src/components/block/interactions/keys.rs:788、src/editor/format_ops.rs:150），只是从没绑过键；清除格式新加动作 `ClearFormatSelection` 与编辑器层处理器 `on_clear_format_capture`（src/editor/format_ops.rs:186，`clear_inline_format_on_selection` 改到内容才 `stop_propagation`），注册在 src/editor/render/paint.rs:821 那一串 `capture_action` 里。默认键 `cmd-shift-h`/`ctrl-shift-h` 与 `cmd-\\`/`ctrl-\\`（src/components/actions.rs:541、:548；两条都核过不与既有键位、也不与固定绑定 ⌘P/⌘⇧P/⌘⇧C/⌘1-9/缩放撞车）。偏好页那一列借命令自己在菜单上的文案（src/config/preferences/pages_shortcuts_window.rs:81、:82，与邻着的删除线、上标、下标同一口径）：同一个概念在菜单与偏好页各出现一次，用同一份字符串才不会「菜单叫这个、偏好页叫那个」，也就不必为六条命令各开一个 `preferences_shortcut_*` 键。「格式」那一档因此十行全有键位，菜单那一列与工具栏的悬停说明一起跟着显示（悬停那条读的是 FP9a 的生效键位表）。刻意不做：段落那一档不给默认键（标题的 ⌘1..⌘6 与本仓换标签的 ⌘1-9 撞车，列表、引用、代码块也没有通行键位可对齐，硬造一串反而要多记一件事），插入那一档除已有的图片、链接、表格之外同理；`document_menu_shortcut` 里这两档仍返回 None，用例把这条口径钉住（src/editor/context_menu/document_menu.rs:604）。验收：`editor::tests::inline_format::cmd_shift_h_marks_the_selection` 与 `cmd_backslash_clears_the_styles_in_the_selection` 两条从真实按键派发起到缓冲区字节；`rows_show_their_shortcut_column_when_a_binding_exists` 改成八种行内样式逐条渲染出键位、标记文本与清除格式两行按字面比出 `⌘⇧H` 与 `⌘\`、段落与插入的七行仍为空。

FP9c 命令面板补齐：「编辑」两条与「格式」十条（已落地）。`CommandMenu` 加 `Edit` 与 `Format` 两档（src/commands.rs:29），只进命令面板、不进系统菜单栏（菜单栏沿用既有那五档，`every_menu_bar_command_has_a_handler` 也按这五档把守）；十二条的 id 与键位表里同一条命令的 id 一致，新增守卫 `palette_ids_for_the_editable_commands_match_the_shortcut_table`（src/commands.rs:310）逐条核这一一致性——面板执行、偏好页改键、菜单显示三处认的是同一个名字，任一处另起就叫不拢。按回车执行当前那一行走 `run_selected_command` → `run_palette_command`（src/editor/command_palette.rs:111、:100），点一行与按回车是同一条收尾。

面板的输入框持有窗口焦点，而这一族的处理者挂在块那一层，实测收不到动作：当场派发 `BoldSelection` 时派发路径长 9（根 → 文档 → 面板输入框那一条），块不在其中；把窗口焦点当场指回那一块仍然不触发，因为 `Window::dispatch_action` 找的是上一帧的派发节点，而那一帧还带着面板；等一次重绘之后再派发同一个动作，块的处理者才触发并写出 `**alpha**`。`window.on_next_frame` 那一条路在本仓的测试里不触发（TestWindow 不走 `on_request_frame`），留一条测不到的路径不如把收口写清楚：「焦点被浮层借走」这一种由编辑器层接手，判定是 `Editor::block_focus_is_live`（src/editor/runtime_context.rs:333）。八种行内格式的捕获处理者按它决定交给块还是在编辑器层切块处理（src/editor/format_ops.rs:106），「粘贴为纯文本」加同口径的 `on_paste_as_plain_text_capture`（src/editor/clipboard_ops.rs:13，注册在 src/editor/render/paint.rs:811）——两处都在焦点不在块上时代为认出当前编辑目标，再调块里那唯一的实现（跨块选区、剪贴板图片那些分支一起跟着走），焦点在块上时原样往下传，⌘V 与 ⌘⇧V 那条路径一字未动。刻意不做：撤销、重做、剪切、拷贝、粘贴五条不进面板（面板原先就没有它们，这一笔只补与两套菜单同源的那十二条）；段落与插入两档同理不进（那两档的默认键还没有，面板里搜得到却按不了比搜不到更难看，等动作与处理器一并补）。验收：`src/editor/tests/palette_commands.rs` 四条——十二条逐条从面板执行比字节（改前红值：`bold_selection` 那条 `left: "alpha one\n\nbeta two\n"`，`paste_as_plain_text` 那条 `left: "选中文字\n\n别段\n"`）、打完查询词回车执行的是筛后那一条（标签从注册表现取，界面语言换了也不写死）、查无命中时回车既不动文档也不收起面板。

FP9d 文档（已落地）。`docs/architecture/editor-core.md` 新增 §8「四处入口一套实现：正文右键菜单、选中工具栏与命令面板」，记的是四入口汇到哪几个函数、可用判定那一份在哪、两套浮层的状态字段与开合路径、二级面板悬停的 120ms、几何与夹取规则、渲染层级（都挂窗口根而不是滚动区）、块级命令为什么必须在编辑器层收口（`Window::dispatch_action` 读上一帧的派发树）、一条命令的键位仪式那八处、以及已知边界（段落与插入两档无默认键也不进面板、上标下标与清除格式不在工具栏、图片与表格轴两个面板不走 `document_menu_origins`）。`overview.md` 的关键不变量加第 9 条（一个动作只有一处实现，新增命令四入口要一起有），`workspace-ui.md` §8 那行命令面板与 §9 那行浮层各补一句并指回 §8。

FP10 右键菜单补齐：「全选」与「拷贝为 HTML」（已落地）。Typora 的编辑那一档有这两行，本仓的动作、键位与处理者早就齐了（`SelectAll` 在键位表里、默认 `cmd-a`/`ctrl-a`，src/components/actions.rs:435；`CopyAsHtml` 有写死的 `cmd-shift-c`/`ctrl-shift-c`，:987），缺的只是菜单上那一行。`DocumentMenuCommand` 加 `SelectAll` 与 `CopyAsHtml` 两个变体，行名 `select-all`、`copy-as-html`（src/editor/context_menu/document_menu.rs:103、:104）；「全选」排在「粘贴为纯文本」之后（与剪切/拷贝/粘贴同组），「拷贝为 HTML」排在「拷贝为 Markdown」之后并单开一条分节线。两条都靠 `window.dispatch_action` 交回既有的那条链（src/editor/context_menu/document_menu.rs:420、:421），不写第二份实现：全选落 `Block::on_select_all` → `Editor::on_rendered_select_all_press` 那一条循环（一次选当前块、紧接着再来一次选整篇），拷贝为 HTML 落 `Editor::copy_as_html`。可用判定与「拷贝为 Markdown」同源（`!document.root_blocks().is_empty()`）。快捷键那一列：「全选」显示生效键位表里的 ⌘A，「拷贝为 HTML」留空——它那颗 ⌘⇧C 是写死的一份绑定、不在键位表内，等它补成表内条目时那一列一起显示。文案两行都借既有的键（`preferences_shortcut_select_all`、`menu_copy_as_html`），不新起 i18n 键。刻意不做：这两行不进命令面板——`SelectAll` 的处理者在块那一层（面板借走焦点时收不到，见 FP9c 那一段），`copy_as_html` 已经在导出的那一档。验收：`the_select_all_row_follows_the_same_cycle_as_the_key`（点一次选「alpha one」那九字节且不成跨块选区、再点一次出跨块选区）、`the_copy_as_html_row_puts_rendered_html_on_the_clipboard`（剪贴板里是带 `<strong>加粗</strong>` 的那份且文档一字未动）、主菜单行序的用例改按十三行把守、`rows_show_their_shortcut_column_when_a_binding_exists` 补 ⌘A 显示与 `CopyAsHtml` 留空两条。
第二期 FP14 把「全选」这一行从菜单上去掉了（键位与块层那条循环一字未动），守卫改从 ⌘A 那一路钉：`the_select_all_cycle_from_the_key_selects_the_block_then_the_document`（src/editor/tests/document_context_menu.rs:45），行序用例改按十二行把守；「拷贝为 HTML」跟着改口「复制为 HTML」。

FP7b 工具栏让位补齐（已落地）。FP7 那份「没有别的浮层」的判断漏了命令面板与快速打开两层（src/editor/selection_toolbar.rs:129 的注释早就写着这一条，代码里缺判断）：工具栏挂在窗口根、比画在正文区里的这两层晚进树，于是选中一段文字再按 ⇧⌘P，工具栏浮在面板的遮罩与列表之上。修法是让锚点不成立而不是在渲染里再收一次——锚点是工具栏开合的唯一来源，每帧现算，当场置 None 只压得住一帧。用例 `selection_toolbar::the_toolbar_yields_to_the_command_palette_and_quick_open` 钉三条：面板开着不出现、快速打开开着不出现、两层都收起后随还在的选区自己回来；改前红在「命令面板开着时工具栏还浮着，会压在面板的遮罩与列表之上」。`dismiss_contextual_overlays` 仍不收工具栏（它是现算的现场，不是需要谁去关的状态）。

FP11 选中工具栏补上「清除格式」（已落地）。§3 选中栏那一行点名的动作里只有这颗按钮在 FP8b 落地时被记成刻意不做（当时的理由是方格里放不下第二个形状），这一笔补上。按钮上写 `A×`：本仓没有图标字体，格子里的字就是它做的事，与既有的 `</>`、`[]()` 两颗同一个口径（`Editor::clear_format_button`，src/editor/selection_toolbar.rs:557）。派发汇到既有那一条：`SelectionToolbarCommand::ClearFormat`（src/editor/selection_toolbar.rs:56）→ `run_selection_toolbar_command`（:707）→ `Editor::clear_inline_format_on_selection`（src/editor/format_ops.rs:37），与右键菜单「格式」那一档最后一行（src/editor/context_menu/document_menu.rs:451）和 ⌘\ 同一条实现，不写第二份。悬停说明取菜单那一份文案加上 FP9a 的生效键位，`toolbar_size` 里的方按钮计数从 7.0 改 8.0（六个行内格式、链接、清除格式，「段落」那颗另按文字估，src/editor/selection_toolbar.rs:151），当时整条实测 320px 宽（面板 x 505.5..825.5）；FP12 把那颗换成图标、加上分节线之后是 279px。当时刻意不做「有没有样式可清」的置灰（理由是要到行内树里数一遍），这一条在 FP12 补上了：判定只读那份树，不重新解析；上标与下标仍不进工具栏，§3 那一行点的就是这七种动作，要放宽从那一行改起，不在这里自己加宽。验收：`selection_toolbar::clicking_clear_format_in_the_toolbar_strips_the_selected_style`（`alpha **one** beta` 选中 `one` 点一次剥成 `alpha one beta`、焦点仍在这一块的选区上、一步撤销复原）、`TOOLBAR_BUTTONS` 改按九颗把守。这条用例写的时候撞出一处坐标口径，与 §7 的 R7、R8 同族，一并记在这里：选区落进样式片段会让那一行当场显出记号（可见文本从 14 字节变 18 字节，块自己把 `selected_range` 换成记号那一份坐标），工具栏按选区摆放于是整体右移 20px，实测第一帧面板 x 505.5..825.5、第二帧 525.5..845.5，而命中测试读的是上一帧的节点——只重绘一帧就去点，点的是挪走前的位置，命令收不到，红在 `left: "alpha **one** beta\n" != right: "alpha one beta\n"`（去掉那次重绘就能复现）。用例因此按 §7 R7 那条口径写：交给块的选区从 `Block::display_text()` 现取（`find("one")` 算出那一段），重绘两帧让落点定下来之后一次点中。真人操作不会出现这一段：按下第一个字符时记号就已经显出、工具栏跟着走，抬手之后要过很多帧才点得到按钮。未在构建出的实机上手点过，以上判断走的是 gpui 测试里的逐帧边界与缓冲区断言。

FP12 两套浮层的宽度算法与工具栏观感（已落地，用户看图报修）。四件事。

一、面板截字的根因。`DocumentMenuGeometry::measure` 原先只把行自己的左右 `menu_item_padding_x` 算进宽度，面板那一份 `menu_panel_padding`（左右各 4）与 `dialog_border_width`（左右各 1）没算进去，于是面板内框比最宽那一行窄 10px；`menu_item` 的标签带 `.truncate()`，截了不报错，屏上就是「一级标题」只剩「一级标」。补这两份之后再加 3px 余量（`MENU_ROW_WIDTH_ALLOWANCE`，src/editor/context_menu/document_menu.rs:718）：拿系统真实度量量过，12 号字下英文最宽那一行 `Numbered List` 实测 93.6，而只补 padding 与边框的估算给到 94.0，只差 0.4px——换一份字体回落就红。全角字的系数从整 1em 提到 1.06em（src/editor/render.rs:326），理由同一处：实测比整 1em 宽一点。工具栏面板另有一处同族漏算：`toolbar_size` 也没算边框，最后一颗格子顶到边线上（src/editor/selection_toolbar.rs:151）。

二、守卫改成按真实字体度量核，不再只核估算。`document_menu::tests::panel_inner_box_fits_the_widest_row`（纯函数，中英文各核一次）钉「面板内框 ≥ 最宽那一行」；`selection_toolbar::the_paragraph_panel_fits_its_labels_measured_with_the_real_font`（src/editor/tests/selection_toolbar.rs:572）用 `window.text_system().shape_text` 量「段落」那一档十二行在中英文下的实际字宽比进内框，并核屏上那份面板的宽与几何算出来的一致；`dragging_a_selection_pops_the_toolbar_after_the_button_is_released` 补一条「每颗格子都在面板之内」。改前红值两条：`内框 94.0 vs 实测文字宽 93.6`（把余量调到 4px 即复现）、`toolbar-clear-format 顶到面板外了：右边 771.5 vs 面板右边 774.5`。

三、工具栏的观感。「段落」「链接」两格换成描出来的 svg（`icon/editor/paragraph.svg`、`icon/editor/link.svg`，16 的框、1.5 的描边，与 `icon/workspace/*` 同一套画法，注册在 src/main.rs:253），因为「段落」那颗没有通行的字母可写，而把中文词塞进一排字母里会读成一句话；三截之间各一条 1px 分节线（`Editor::toolbar_separator`，src/editor/selection_toolbar.rs:382）；字母字号 12 → 13.5、`</>` 收小到 11，让字母与图标在一排里一样抢眼；清除格式那颗从描出来的 `A×` 换回字体渲染的 `A` 加一枚 9.5 号、调淡的 `×`（src/editor/selection_toolbar.rs:557）——用户报修那颗与其他格子「颜色大小都对不上」，根子是描出来的图标与打出来的字母摆在一排，字重与高度怎么调都对不齐，于是让它跟 B、I、U、S 走同一份字体渲染。整条宽 279（九颗格子 +「段落」那颗 39 + 两条分节线 + 边框）。

四、「清除格式点下去没反应」这半条报修。这颗原先与八种行内样式同一条置灰（有选区且写得动），可只选中一串没样式的字时它确实什么都不会做，亮着就是骗人。新增 `Editor::clear_format_is_available`（src/editor/format_ops.rs:44）：在既有两条之上多问一句「选区里挂着样式吗」。问的是 `InlineTextTree::has_styles_in_range`（src/components/markdown/inline/tree.rs:899，`clear_styles_in_range` 的只读版，链接、脚注、公式与 HTML 行内样式都不算，与那条写回的口径一字不差），经块层 `Block::has_inline_styles_in_selection` / `has_inline_styles_in_range`（src/components/block/runtime/text_ops.rs:268、:276）做同一份屏幕坐标换算；跨块选区逐块问，切段与写回那条共用 `normalized_cross_block_selection`。菜单那一行（src/editor/context_menu/document_menu.rs:308）与工具栏那颗共用这一条，不会出现一处亮一处灰。灰着的那一颗没有悬停底色（点了不会有任何事发生），用户看着就成了「这颗坏了」——所以悬停说明改口：亮的时候写「清除格式 ⌘\\」，灰的时候写「清除格式 · 选区里没有可清除的格式」（新文案 `format_clear_unavailable`，五处位点齐），把「为什么没反应」当场说清楚。守卫：`clear_format::clear_format_availability_follows_the_selection`（裸字灰、带粗体亮、只有链接灰、跨块只要有一头挂着样式就亮）。

刻意不做：八种行内样式不做「已经带这个样式就置灰」——开关一种样式在没带样式的字上是有用功（加上），与清除格式那种「手上没东西可清」不是一回事；上标与下标仍不进工具栏（FP11 那条理由不变）；不给工具栏格子加图标底框或分组标题，分节线已经够把三截分开。

验收：`src/editor/tests/clear_format.rs` 七条（新增的口径由第六、第七两条一起钉：菜单那一行与编辑器层入口同一条、裸字选区什么都剥不到也不报改动）、`document_context_menu::rows_that_cannot_run_stay_in_place_but_greyed`（裸字选区下「清除格式」灰、其余九行亮、行序与行高不变）、上一条里的三条宽度守卫。全量 `cargo test --bin velora` 1521 通过、0 失败、6 项 ignored。

FP13 段内拖动不再要求那一段先有焦点（已落地，用户报修）。症状：去选另一段里的文字，按住拖动没有反应，必须先单击那一段把光标落下去，第二次才拖得动。根因在块层那一次按下：`Block::on_mouse_down`（src/components/block/interactions/keys.rs:331）原先只在「这一块已经聚焦」时置 `is_selecting`，未聚焦的分支只落光标再请求焦点；随后编辑器层的 `on_editor_mouse_move` 又对「锚点与落点在同一块」直接返回（src/editor/selection.rs:117，同块交给块自己处理），于是同一块内的拖动两边都不管，抬手之后选区停在 `1..1`。改法是把 `is_selecting` 从「聚焦与否」这两支里提出来，按下就起（src/components/block/interactions/keys.rs:384），未聚焦只多那一句请求焦点（:392-:393）；`select_to` 仍只在已聚焦的 shift+点那一条路上走（:387），未聚焦时照旧落光标。守卫 `selection_mouse::dragging_inside_a_paragraph_without_prior_focus_selects`（src/editor/tests/selection_mouse.rs:150）：先单击第一段并核 `active_entity_id` 落在它身上，再到第二段上按下—拖动—抬手，断言那一块的 `selected_range` 非空、编辑目标跟着换过来、选中工具栏也浮出。改前红值 `在没聚焦的段落里按下拖动该选出文字，实际选区是 1..1`。段落、代码块与表格格子的按下都挂在那一条处理者上（src/components/block/render/shell.rs:123），所以未聚焦的代码块与格子同样跟着修好；用例只核了段落那一支。刻意不做：跨段的 shift+点扩选——未聚焦块里那一份 `selected_range` 是焦点离开时留下的旧锚点，拿它当锚点会选出用户没打算要的一段，先按旧口径落光标；拖动从一个块内继续扩到相邻块仍然只由 `cross_block_drag` 那条既有路径负责，端点跨块时才成立。全量 `cargo test --bin velora` 1523 通过、0 失败、6 项 ignored。未在构建出的实机上手点过，验证走 gpui 测试里真实的按下—拖动—抬手事件序列与逐帧边界。

## 6. 边界与代价

- 本期只覆盖渲染态（所见即所得）。源码模式的右键沿用现在的原生编辑行为，只加最基础的剪切/拷贝/粘贴段——源码模式下选区和块的语义不一致，做段落转换会把用户写的字面文本改掉。
- 跨块选区上做行内格式是逐块处理，不做「一段被拆开的粗体」这种跨块语法（markdown 本身不支持）。
- 高亮 `==x==` 是本仓库新语法。代价：老文件里成对出现的字面 `==` 会被解析成高亮（`1 == 2 and 3 == 4` 里的 `2 and 3` 会变成标记文本），代码块与行内代码内不解析，没配对的单个 `==` 保持字面。套叠的先后按样式栈序写回，`==**x**==` 会规范成 `**==x==**`（与 `~~` 已有的口径一致）。用户手写的 `<mark>x</mark>` 仍按原生 HTML 显示，不吸收成高亮样式——序列化只会写 `==`，吸收了就等于改掉用户没碰过的字节。
- 不改 `context_menu_panel_width` 的默认值给表格轴菜单和图片菜单，避免这两个菜单跟着变宽。
- 不做子菜单的键盘导航（右键菜单目前整体不支持方向键，那是另一期）。
- 有序列表的序号由所在列表组重算（`sync_block_list`，src/editor/tree.rs:937），文件里字面写的那个起始号（`5. 甲` 这种从 5 起跳的写法）解析后不留副本。代价：把一组中间的项换出去，它后面那些项的序号会从头排；同族的写法（`.` 与 `)`、`-` 与 `+`）另有 `list_marker` 记着，不受影响。打字打断一组时模型本来就是这么算的，这一笔没有另立口径。
- 并进同一个列表组时，接缝那个空行按紧排列表的写法收掉（`collect_root_markdown_lines` 里「前后都是列表项就不补空行」那条既有规则）。所以「一段正文紧跟在列表下面换成列表项」会把两行并成紧排，不是只加记号；换出去的接缝同理补回空行。
- 引用这一档接、标注那一档不接：引用是一根块、文字在自己标题里，换进换出不动别人；标注（`> [!note]`）自己带头部与正文子块，换出去要安置子块（FP4b-4）。跨两行的引用只许换成正文或取消引用，换标题会被拒（`#` 后面那行会被读回成另一块）。带子块的列表项只许在列表一族内部换种类，换成正文或标题时菜单那一行置灰。
- 代码块只给「退回正文」这一条出路，退回时围栏行与语言号（`rust` 这类信息串）一起收掉——那份信息属于围栏行，正文里没有地方存它，硬要保留就得改写正文，与「只动被波及那几行」冲突（要保留语言就先在源码模式改，或等后续把信息串单独记账）。代码块上弹不出正文右键菜单（FP6 之前就有这条口径，本笔不放宽），所以从代码块退回去要走选中工具栏那一档或键位——退出去那一条已经有 ⌘↵（`exit_code_block` 的既有键位，本仓「离开这一块」的那颗键），FP9b 不给「转成代码块」这一行另造新键，理由与段落那一档相同。
- 链接这一档只接「选中了字」：空的 `[]()` 在行内树里存不住（写下去重新解析时那四个字符被当成空标签的链接丢掉，实测可见文本仍是原文），所以只有光标时菜单那一行与工具栏按钮都做不出东西，置灰处理——与格式那一档其余八行「要有选区才点得动」同一条口径。选中一段文字后粘贴网址另有 `Block::paste_url_as_link` 那条路（写字面 `[选中](地址)`），两者不重叠：一个补外壳等地址，一个手上已经有地址。

## 7. 风险

- R1：段落转换写前缀再 normalize，可能和「缓冲区是唯一事实源」的最小差异原则打架。做法是每次都开一个 `NonCoalescible` 撤销组，且断言改完的字节序列；FP4 若出现字节漂移（例如 CRLF 文件被洗成 LF），要先解决再往下走。
- R2：选中工具栏在滚动与缩放时的定位。已按「锚点每帧随选区重算」处理，选区整段不在视口里时由 `selection_is_on_screen`（:152）判掉；跨块时锚点要扫一遍可见块，成本是一次遍历加每块一次已缓存布局的取矩形，实测全量用例（含逐帧性能闸门）没有变化。
- R3：`toggle_inline_format` 依赖块自己的 `selected_range`。跨块选区时焦点块的 `selected_range` 只是选区尾部那一小块，直接用会只格式化最后一行——FP2 必须在 Editor 层按块切片。
- R4：菜单文案 5 个位点漏一个会在语言包导入校验（`i18n/manager.rs:205-222`）之外静默回退英文，靠 `config::preferences` 与 `i18n` 测试组兜住。
- R5（FP5a 量出来，已修）：`Block::enter_math_block("")` 原先写 `$$\n\n$$`，而区域扫描在空行处看到下一行像根块开头就断块，`$$` 自己也算根块开头，于是那对紧挨空行的记号被切成两块原始 markdown——打字 `$$` 加回车产生的空公式块，存盘重开就散了。两处一起收：打字那一路的空正文改写成紧挨着的两行 `$$\n$$`（src/components/block/runtime/text_ops.rs:107，与 FP5a 插入那一路同一个形状，两入口不再各写一种），`collect_display_math_region`（src/editor/document/parse.rs:575）在空行处先看一眼后面那行是不是以 `$$` 收尾——是就当成本块的结束行继续扫，不当成下一块的开头。验收：`editor::events::tests::blocks_enter::dollar_dollar_enter_creates_editable_math_block` 把落盘字节重新读一遍比根块种类（改前红在 `assertion left == right failed, left: "$$\n\n$$", right: "$$\n$$"`），`editor::document::tests::lists_tables::display_math_region_survives_a_blank_line_before_the_closing_marker` 三份写法（空正文、正文后空行、夹在两段之间）各比一次结构与字节（改前红在 `"$$\n\n$$" 读回 [RawMarkdown, RawMarkdown]`）。
- R6（本仓的格式化边界）：`rustfmt` 会跟着 `mod` 声明往下把整棵子目录重写。`src/editor/tests.rs` 一类只做声明的文件不能直接喂给 rustfmt（实测一次带出 28 个无关文件的改动），要格式化就只喂自己没有 `mod` 项的叶子文件，并且用 `--edition 2024`（本仓 edition 2024，2021 档遇到 let 链会直接报错退出）。
- R7（FP8a 量到，FP10 期间逐帧量过选区后结案：不是产品缺陷，是用例把选区写死）：同一个窗口里连着走两轮「选区 → 右键 → 点菜单行」，第二轮量到 `"https://example.test 后\n\n另一段\n"`，看着像落点漂了。逐步打印 `selected_range`（写 `0..14` → 焦点落回之后读回 `0..18` → 第二轮仍是 `0..14`）之后清楚了：粘贴每次都严格按当轮生效的那份选区落，一轮都没漂；漂的是选区本身——`"前 **加粗** 后"` 这一块的屏幕文本长度不是用例里那个数（R8 那条形状：整块只有一种样式时成对记号留在屏幕文本里），写 `0..14` 只盖住半句，剩的 " 后" 是应该留下的。第一轮之所以对，是因为同一次 `select` 里 `focus_block` 的焦点落回把写进去的 `0..14` 换成了整块。结论与口径：交握给用例的选区一律现取 `Block::visible_len()`，不写死字节数；钉住这一条的守卫是 `clipboard_text::two_menu_rounds_land_on_the_selection_in_effect`（两轮各选整块再各点一次「粘贴为纯文本」，两轮都只剩那个网址）。产品侧不改一行。
- R8（FP8b 量到的模型形状，不是这一笔引出的缺陷）：一整块只有一种行内样式时，那一族的成对记号留在屏幕文本里（实测 `**abc**` 那一块的 `display_text` 就是七个字符 `**abc**`；`before **bold** after` 那种「文字 + 记号 + 文字」的块则显示成 `before bold after`，记号不进可见文本）。菜单、工具栏与快捷键交下来的选区都是屏幕坐标，块内经 `current_to_clean_range` 换算，两种形状都走得通；写用例时选区坐标按实测的屏幕文本取，不能按「记号一定不进可见文本」想当然。

## 8. 第二期：右键菜单的观感（2026-10-06，用户看图报修）

报修三件事：菜单太长、没有图标不好看、同一种动作用了两个名字（「拷贝」与「复制」混着用）。要的形状点名 Windows 11 的右键菜单：剪切、复制、粘贴这一族做成纯图标，排在最上面一行。

现在的 13 行 + 4 条分隔线（src/editor/context_menu/document_menu.rs:195 那一份 `document_menu_rows`）改成「一条图标行 + 6 行」：

```
┌────────────────────────────────┐
│ ↶  ↷ │ ✂  ⧉  📋  📋A         │  撤销·重做 | 剪切·复制·粘贴·粘贴为纯文本
├────────────────────────────────┤
│ ▤  复制为 Markdown             │
│ 〈〉复制为 HTML                 │
├────────────────────────────────┤
│ A  格式                        ›│
│ ¶  段落                        ›│
│ ＋ 插入                        ›│
├────────────────────────────────┤
│ ⇄  切换源码模式                │
└────────────────────────────────┘
```

口径：

- 图标条六颗纯图标，内部按「撤销/重做」与「剪切/复制/粘贴/粘贴为纯文本」分两组，组间一条 1px 竖线。格子 26×26、图标 16，与选中工具栏同一套尺寸与配色（`dialog_secondary_button_text`、`dialog_secondary_button_hover`、按下 0.92 透明度），悬停说明写「标签 + 生效键位」，置灰的格子没有底色也不接点击——与工具栏那颗「清除格式」同一口径。
- 主面板的长行都带前置图标：复制为 Markdown、复制为 HTML、格式、段落、插入、切换源码模式。图标列宽 16 + 间距 8 要一起进 `DocumentMenuGeometry::measure` 与 `row_width`，漏算就又是截字（FP12 那条根因同族）。
- 三个二级面板不加图标，仍是「文字 + 键位」的行：那一档放的是可扫读的条目名（一级标题、有序列表……），图标只会把每行拉宽、把扫读节奏打乱。与选中工具栏那条分工一致——格子里放图标，面板里放文字。
- 「全选」这一行删掉：⌘A 那条循环本来就在（一次这一块、再下一次整篇，FP10 记的实现），菜单上多占一行没有额外信息。「删除」这一行正文菜单本来就没有（对着 `document_menu_rows` 逐行核过），带删除的是表格行列那一档与图片菜单，那是删结构，留着。
- 文案统一成「复制」：`context_menu_copy` 拷贝→复制、`context_menu_copy_as_markdown` 拷贝为 Markdown→复制为 Markdown。`menu_copy_as_html` 本来就叫「复制为 HTML」，两处此前对不上，正是那句「又是拷贝又是复制」。

拆分（按提交顺序）：

- FP14 文案与行序：两处改名 × 5 位点、去掉「全选」那一行与 `DocumentMenuCommand::SelectAll` 变体（动作、键位与块层那条实现一字不动），行序与几何用例跟着改。
- FP15 行前置图标：`menu_item` 加一个可选图标位（既有五处调用逐处补参数），六枚 svg 资产与注册，几何把图标列算进宽，真实字体度量守卫。
- FP16 顶部图标条：新增 `DocumentMenuRow::QuickActions` 一个变体负责渲染、测量与行序；撤销/重做/剪切/复制/粘贴/粘贴为纯文本六行从主面板消失、进图标条；面板高度、落点与二级面板对齐跟着这一条走；用例点名每颗图标按钮派发同一动作并收起菜单。

代价与刻意不做：撤销、重做、剪切、复制、粘贴、粘贴为纯文本六项不再有键位列（键位改在悬停说明里读生效键位表那一份），这是「一行顶六行」的代价；表格轴菜单、图片菜单、文件树与标签右键不在本期（它们没有这一族动作，图标另是一套）；不为图标条新增主题尺寸字段，先用 `src/components/menu.rs` 里的常量，与工具栏那份常量同形，等两处真要分别调参时再提到主题里。


落地记录（第二期）：

FP14 文案与行序（已落地）。两处中文值改口：`context_menu_copy` 拷贝→复制、`context_menu_copy_as_markdown` 拷贝为 Markdown→复制为 Markdown（src/i18n/strings_api.rs:302、:305）。改的是 `zh_cn()` 那一份的值，键名与 `keys.rs` 清单不动，`de_impl.rs:700` 的回退取的就是这份默认值，用户自己的语言包仍然覆盖；英文两处本来就是 Copy / Copy as Markdown，对不上的是中文这一侧。「全选」那一行连同 `DocumentMenuCommand::SelectAll` 变体、行名、label、shortcut、dispatch 四处分支一起从菜单上去掉，动作 `SelectAll`、键位 ⌘A 与块层那条循环一字未动；主菜单从十三行变十二行（src/editor/context_menu/document_menu.rs:234）。守卫从点菜单行那一路挪到按 ⌘A 那一路：`the_select_all_cycle_from_the_key_selects_the_block_then_the_document`（src/editor/tests/document_context_menu.rs:45），另加一条「菜单上不再挂着 ⌘A 那一列」。

FP15 行前置图标（已落地）。`menu_item` 多一个 `icon` 参数（src/components/menu.rs:22，第九位），图标是 16 的框、与文字之间用的就是行内那一份 `MENU_ROW_GAP`（flex 的间距对所有相邻子节点生效，不留第三个常量去漂移），置灰时图标与文字一起换成 `dialog_muted`。图标只在一处定义：`document_menu_command_icon` 与 `document_submenu_icon`（src/editor/context_menu/document_menu.rs:46、:78），渲染与 `row_width` 共读，不会出现「画了图标、按纯文字算宽」。新画 11 枚描出来的 svg（`assets/icon/editor/`：undo、redo、cut、copy、paste、paste-plain、copy-markdown、copy-html、format、insert、toggle-source，16 的框、1.4-1.5 的描边，与既有的 paragraph、link 同一套画法），注册在 src/main.rs:253 那一片。二级面板的行仍然不带图标，工具栏「段落」那一档的面板同理（src/editor/selection_toolbar.rs:676 传 `None`）。

- 真实度量守卫换了一份实现两处用：`real_label_widths` 提到 src/editor/tests/common.rs:79（原先内联在工具栏那条用例里），新增 `document_context_menu::the_main_panel_rows_draw_their_icons_inside_the_measured_box` 逐行核三件事——图标画得出且在面板之内、`图标列 + 标签实测宽 + 键位列 + 二级箭头` 装得下内框、屏上面板的宽高与 `DocumentMenuGeometry::measure` 一字不差。主面板为了这条守卫补了 `.debug_selector("editor-context-menu-panel")`（src/editor/context_menu/render.rs:146，与工具栏 `editor-selection-toolbar` 同一写法）。
- 新守卫当场撞出第二处漏算（与 FP12 同族）：`measure` 的宽加过边框、高没加，实测屏上 389 vs 算出 387，差的正是上下各 1px 的边框（src/editor/context_menu/document_menu.rs:789 补上 `dialog_border_width * 2.0`）。这条不是观感问题：`document_menu_origins` 按这份高夹紧下沿，少算 2px 就让菜单比 intended 低 2px。
- 图标的形状没在构建出的实机上看：用系统 QuickLook 把 13 枚 svg 各自栅格化成 128px 逐张看过（剪刀、带两行与带 T 的两份剪贴板、返回箭头、双向箭头、A、`</>`、Markdown 徽标都能读出来），落进菜单之后的观感以用户实机为准。
