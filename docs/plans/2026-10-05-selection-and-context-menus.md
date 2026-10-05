# 选中菜单与右键菜单（对齐 Typora）

日期：2026-10-05
状态：计划已定，按功能点顺序实现

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

## 4. 界面与交互约定

选中菜单：

- 一行，横向排布，方形按钮 26×26，圆角与字号沿用主题的 `menu_item_radius`/`menu_text_size`，颜色用 `dialog_secondary_button_text`，悬停 `dialog_secondary_button_hover`，按下 0.92 透明度反馈（与标题栏按钮同一写法）。按钮上是字母形状而不是 SVG 图标：B 加粗、I 斜体、U 下划线、S 盖一条横线、`</>` 行内代码、A 带底色标记，本仓的 svg 清单（main.rs 的 `include_bytes!` 那一片）里没有格式类图标，为这一条临时造一套图形资产是另一件事；字母形状在中英文下都直接可读，不需要两份图形。
- 位置：选区外接框的上方 8px；上方放不下就翻到下方 8px；左右按视口宽度夹紧（沿用 `context_menus.rs:99-107` 的 8px 夹紧写法）。
- 出现时机：鼠标或键盘产生非空选区后出现；选区塌成光标、切到源码模式、右键菜单或任何浮层开着时不出现。拖动的中间过程不浮出（`cross_block_drag` 还开着就按住），只在抬手之后定位一次。滚动时不做特殊处理：选区还在，面板就跟着选区重算一次位置（比原地钉住准），整段滚出视口才收起。「焦点离开文档就收掉」这条没做：这个应用一篇一个 Editor 视图，选区在模型里留着、面板也就留着，窗口失焦时是否要藏起来涉及所有浮层的统一口径，不在这一笔。
- 工具栏本身不吃键盘焦点（gpui 里按钮按下不影响块的焦点），保证按下去之后 ⌘Z、方向键仍然作用在文档上。
- 每个按钮有悬停说明，写明中文名和快捷键。

右键菜单：

- 面板宽度按这一列里最宽的一行来定（`estimated_menu_label_width` 那套字符宽度估算，标题栏菜单本来就在用），下限取主题的 `context_menu_submenu_width`。定死 200 在中文下偏空、在英文下会把「Toggle Source View」这类长标签截掉；算出来的宽度同时给落点夹紧用，两处不会各自理解一遍面板尺寸。表格轴菜单、图片菜单仍用现有宽度。
- 分节用 1px 分隔线，顺序固定：编辑 → 格式 → 段落 → 插入 → 视图。段落与插入内部用二级子菜单，避免一个 30 行的长菜单。
- 每行右侧显示快捷键（灰 `dialog_muted`），没有快捷键的留空位对齐。
- 不可用的项（例如没有选区时的「拷贝」「清除格式」）置灰不隐藏，鼠标位置不变——菜单宽度变化会让连点两次右键跳位置。置灰只看这件事的前提在不在：剪切/拷贝看有没有一段选区，撤销/重做看历史，粘贴看剪贴板，格式与段落看这份文档能不能按区间写回。
- 右键时如果当前有选区，先保住选区（现在切到别的块会清空），菜单动作全部围绕这段选区。

## 5. 功能点拆分（按提交顺序）

每个功能点自己跑定向测试 + 全量 + clippy，写完就提交一笔，不攒。

FP1 菜单行渲染收口。新增一个通用行渲染：`(id, label, shortcut: Option<&str>, enabled, danger, on_click)`，支持分隔线与「右箭头 + 子菜单」；把 `render_axis_menu_item`、文件树菜单、标签菜单、插入子菜单里那 4 份重复行代码和 5 份分隔线代码收过来。验收：现有 3 个菜单测试仍通过；新增一条测试断言同一段代码渲染出的行高、内边距、快捷键文字位置一致。

FP2 动作层：选区上的一行内格式。给 `InlineFormat` 加 `Strikethrough / Superscript / Subscript`，接上 `InlineTextTree` 已有的 `toggle_style`（去掉 `toggle_strikethrough` 的 `dead_code`），并把 toggle 入口提到 `Editor`：`toggle_inline_format_on_selection(InlineFormat, cx)`（`src/editor/format_ops.rs`），跨块选区按块逐个处理，全程一个撤销组；块的 `toggle_inline_format_in_range` 收**可见文本**坐标，块内自己换算到树内坐标，Editor 层不必知道标记占位。上标/下标按本仓库既有写法落 `<sup>x</sup>` / `<sub>x</sub>`，与 Typora 的默认输出一致（Typora 也认 `^x^` / `~x~`，读入路径已有）。置灰要用的判定函数（选区能否做行内格式）随 FP6 一起进，这一笔没有消费者，进来就是警告。验收：单块、跨块、空选区三类用例。

FP3 高亮 `==x==`。新增 `StyleFlag::Highlight` + 分隔符解析 + 渲染颜色（用主题的强调色，浅色主题黄底、深色主题低饱和黄底），加 `InlineFormat::Highlight`。验收：解析往返测试（`==x==` 读进来 → 存出去不变形），toggle 用例，已有 markdown 测试无回归。

FP4a 动作层：段落转换的第一段——标题与正文。`Editor::apply_block_kind_to_selection(BlockKindTarget, cx)`（`src/editor/paragraph_ops.rs`），`BlockKindTarget = Heading(1..=6) | Paragraph`；对已经是这一级的标题再点一次等于取消。块的种类就地换（`Block::set_kind_in_place`，不发事件、不开撤销组），一次命令一个撤销组，字节按「前一块（仅当它的写法与文件一致）… 最后改到的那块」这一段区段写回，接缝空行由区段序列化按渲染态规则拼；引用与标注是容器、表/代码/公式是原子结构块，本段直接不动它们。验收：单块、跨块、取消、接缝、写回不碰邻居写法（Setext 夹具）五类用例。

FP4b-1 动作层：段落转换的第二段——列表（已落地）。目标补 `BulletList | NumberedList | TaskList`（`src/editor/paragraph_ops.rs`）。三条口径：记号跟着同族邻项抄（`+ ` 不被换成 `- `、`1)` 不被换成 `1.`，见 `list_marker_for_conversion`），没有同族邻项才用块自己记过的那份；任务项再点一次是去掉复选框退回无序项，不是取消整个列表；带子块的父项只在列表这一族内部换，换成标题或正文会拒（`block_kind_conversion`，序列化时子块的缩进层数跟着父块的种类走，换出这一族会把子块写成与父块平齐的行，块树还是父子、文件已经是两个根）。菜单的置灰与命令的实际行为读同一条判断（`block_kind_target_is_available` 调 `block_kind_conversion`），三个入口（快捷键、右键「段落」档、选中工具栏的下拉）共用这份行数据。验收：`src/editor/tests/paragraph_kind.rs` 里每种目标一条（缓冲区字节 + 根块种类 + 撤销一步回到原样），外加记号继承、序号接所在组、列表组中间换一项后重读字节得到同一结构、三段正文一次撤销、带子块的父项两种结果各一条；`document_context_menu.rs` 两条（点「任务列表」写回 `- [ ] `、在无序项上点「无序列表」取消记号）、`selection_toolbar.rs` 一条（工具栏下拉给出的行与右键菜单同源且写得动同样的字节）。

FP4b-2 动作层：段落转换的第三段——引用（已落地）。目标补 `Quote`（`src/editor/paragraph_ops.rs`）。解析器把引用建成一根根块、整段文字存在它自己的标题里（`> 甲\n> 乙` 是一块两行，`source_line_prefixes` 记每行让开几字节），所以换进换出动的就是这一块自己的字节，不需要动子块：`next_kind` 接 `Quote ↔ 正文`、`标题 / 列表 / 引用` 之间互换，再点一次「引用」等于取消引用。两条护栏：带子块的块仍只许在列表一族内部换（`block_kind_conversion`），跨两行的引用不许换成标题——`# 甲` 后面那行会被读回成另一块，块树一根、文件两根。落笔后 `written_line_ledger` 认不出引用这一族（只认正文 / 标题 / 列表项），那份记号宽度账清空，位置换算交回按文件量那一条，直到下次解析重新记。验收：`src/editor/tests/paragraph_kind.rs` 里正文 ↔ 引用（含再点一次取消）、跨两行的引用换正文不拆块、跨两行的引用换标题被拒且菜单那一行同步置灰、单行引用换标题不碰邻块、列表项换引用补那行空行、跨两块一次包成两块引用且写下的字节读回两块、一步撤销各一条；`document_context_menu.rs` 一条（点「引用」写回 `> `、再点一次取消、一步退回上一次）；`selection_toolbar.rs` 那条同源用例把 `quote` 行一并点名。

FP4b-3 动作层：段落转换的第四段——代码块（已落地）。目标补 `CodeBlock`（`src/editor/paragraph_ops.rs`）。正文 / 标题 / 列表 / 引用整块换进围栏，正文原样进围栏（`language` 给 `None`，那对围栏由 `safe_code_fence_with_info` 定长：正文里本来就有反引号围栏时外侧改用 `~~~`）；再点一次退回正文，围栏行与信息串一起收掉。两条口径：换出代码块只给「退回正文」这一条出路（`block_kind_conversion`），换成标题、列表或引用会把围栏内的多行原文按另一族的记号重写；`Block::set_kind_in_place` 的闸门从「raw 编辑模式一律不动」收窄到「只有 `SourceRaw` 不动」（src/components/block/runtime/text_ops.rs），代码块这一族因此换得出去，同时换出时把 `code_is_indented` 与 `source_fence_lines` 两本账清掉，免得下一次序列化还按缩进或围栏的形状写。验收：`src/editor/tests/paragraph_kind.rs` 里正文 ↔ 代码块（字节 + 块树 + 再点一次取消 + 一步撤销）、带语言写的围栏退回正文（信息串跟着围栏一起没掉，并把写下的字节重新读一遍比结构）、缩进写法的围栏退回正文（四格去掉、账上不留 `code_is_indented`）、代码块只给退回正文（标题 / 列表 / 引用三种目标既拒又置灰）、正文含围栏行时外侧改用波浪号围栏；`document_context_menu.rs` 一条（菜单点「代码块」补围栏；退回那一半按编辑器层入口验，因为代码块上弹不出这套菜单）；`selection_toolbar.rs` 一条（选中代码块里的文字，工具栏下拉那一行把围栏收掉）。

分割线不进「换种类」这一档：正文换成分割线要把那一行文字丢掉，破坏性动作不该藏在段落菜单里。它改由插入那一档给（FP5 的「插入分隔线」，`make_separator` 那条路径本来就不需要正文入参）。

FP4b-4 动作层：段落转换的第五段——标注（`> [!note]`）。标注自己带头部那颗变体记号（`callout_marker` 记用户写法）与正文子块，换进换出要把子块安置进根序列或收进容器，`source_separator_bytes` 那本空行账也要跟着重记；这一笔先把 `Callout` 留在 `next_kind` 的拒绝名单里（菜单那一行置灰）。验收：标注 ↔ 正文、标注换引用、多层嵌套标注各一条。

FP5a 动作层：插入类的第一段——五类块（已落地）。新增 `src/editor/insert_ops.rs`：`InsertBlockTarget = CodeBlock | MathBlock | Separator | Toc | FrontMatter`，入口 `Editor::insert_block_after_selection`（一条命令一个撤销组）与 `insert_block_target_is_available`（菜单置灰与命令实际行为同一条）。落点在光标所在根块之后（跨块选区落在整段选区之后），Front Matter 例外——解析器只认第一行那对 `---`，它必须顶到 0 位，接缝空行补在它后面。字节只在插入点那一处写：`接缝 + 新块的 markdown`，写完把插入点之后的根块区间整体挪位、给新块挂自己的区间（不含两边接缝），算不出来（锚点块没有源码区间，后台续建到一半）才交回整篇重投影。新块是代码块 / 公式块 / 分割线这类「光标走过去就出不去」的形状时，跟着 `ensure_trailing_paragraph_after_structural` 补一块空段落当退路。插完光标交给新块：代码块落在围栏里那一行、公式块落在开栏之后、Front Matter 落在两条 `---` 中间那一行、目录落在 `[toc]` 末尾、分割线停在自己那行。两处实际形状靠探针量出来：空代码块序列化是 ` ```\n\n``` ` 三行；空公式块不能写成 `$$\n\n$$`（解析器会把它切成两块原始 markdown），只有 `$$\n$$` 读回来还是一块。验收：`src/editor/tests/insert_blocks.rs` 九条——每类各一条（缓冲区字节 + 根块种类顺序 + 光标落点 + 一步撤销），五类各插一次把字节重新读一遍比块树，Front Matter 那份「只能在最前面、一篇只能有一份」（第二份既拒又置灰），跨块选区的落点，没碰过的那块 Setext 标题字节不动（钉住不是整篇重投影），以及右键菜单点「目录」与编辑器层入口同一条（六行的行名全点名）。菜单行：`插入` 那一档从一行变六行（表格 + 五类），行名 `insert-code-block`…`insert-front-matter`；文案 4 个键 × 5 处（`insert_math_block` 公式块 / Math Block、`insert_separator` 分隔线 / Thematic Break、`insert_toc` 目录 / Table of Contents、`insert_front_matter` Front Matter），代码块那一行复用段落档已有的 `paragraph_code_block`。

FP5b-1 动作层：链接（已落地）。选中那段字外面包一层 `[文字]()`，地址留空由用户当场写：落笔后光标停在 `](` 之后。三个入口收在同一条实现上——`Editor::insert_link_on_selection`（src/editor/insert_ops.rs）由 ⌘K（新增 `ShortcutCommand::LinkSelection`，默认键 `cmd-k`/`ctrl-k`）、右键菜单「格式 → 链接」、选中工具栏那颗 `[]()` 按钮共同调用；菜单那一行的置灰读同一条 `link_insert_is_available`。单块交给 `Block::wrap_visible_range_in_link`（src/components/block/runtime/text_ops.rs:147），沿块自己那条 `Changed` 写回字节；跨块按可见块逐段包（切片口径与 `toggle_inline_format_on_selection` 一致），全程只开一个 `NonCoalescible` 撤销组，光标交给第一段。工具栏因此从七项变八项，那颗方形按钮上的字就是它写下的写法（本仓没有图标字体）。菜单行名 `link`，排在「格式」那一档末尾、与八种行内样式之间隔一条分隔线；文案一个新键 × 5 处（`insert_link` 链接 / Link），偏好页的快捷键表用同一个键。刻意不做应用内地址输入气泡：本仓的文本输入通道只有工作区那一条（`OverlayInputKind` 配 `EntityInputHandler`，src/editor/workspace/overlay_input.rs），为链接单开一档要把焦点归属、撤销分组与浮层渲染各走一遍，代价大于收益；外壳与落点先做对，地址直接敲就是最短路径。验收：`src/editor/tests/link_insert.rs` 七条——单块包住的字节断言 + 光标落点 + 焦点留在改过那一块 + 块树不拆 + 一步撤销；只有光标时既点不动也写不动，并断言菜单那一行的置灰与 `link_insert_is_available` 同源；跨块两段各包一层且一步同时退回；`- ` 记号与另一块字节不动；⌘K、菜单行、工具栏按钮三个入口写的字节一样。

FP5b-2 动作层：图片（已落地）。选文件那一步走原生文件选择器（消息框被 `app_source_never_uses_native_prompts` 禁掉，原生选择器在放行名单里），选完交回一条公共入口 `Editor::insert_image_at_caret`（src/editor/insert_ops.rs）——拖放那条路原本自己拼「焦点块 → 切开前后两段 → 交粘贴」，现在收在这里，两边同一形状。图片本身的落盘与路径写法沿用粘贴那条路径（`handle_paste_image_request` → `pasted_image_markdown`）：磁盘上的图片先收进文档旁边的 `assets`，写下的行是 `![文件名](./assets/文件名.png)`，光标所在段落被切成「前面 / 图片行 / 后面」三块，其余字节不动。三个入口：`插入 → 图片`（行名 `insert-image`，排在表格之后，置灰读 `image_insert_is_available`）、⌘⇧I（新增 `InsertImage` 动作与 `ShortcutCommand::InsertImage`，默认键 `cmd-shift-i`/`ctrl-shift-i`）、以及拖放。刻意不做：应用内的图片预览与「复制图片到文档旁边」的开关——那是偏好项，不在菜单这一档。验收：`src/editor/tests/insert_image.rs` 三条——插在段落中间产生的块序与缓冲区字节（含别处 `__下划线__` 写法不动、图片确实被复制进 `assets`、一步撤销把三块一起放回去）、写下去的字节重新读一遍还是同一套结构且那行带图片运行时、菜单那一行的行序与置灰口径同源于 `image_insert_is_available`。选择器那一步用例不点：gpui 的测试壳把 `prompt_for_paths` 写成 `unimplemented!()`（vendor/gpui/src/platform/test/platform.rs:334），一点就 panic。

FP6 右键菜单成形（已落地）。`ContextMenuState::Insert` 换成 `Document`（`open_submenu`/`hovered_submenu` 两个 `Option<DocumentSubmenu>` 取代原来那三个 bool），内容拆到 `src/editor/context_menu/document_menu.rs`：`document_menu_rows` 给编辑 → 格式 → 段落 → 插入 → 视图五段，格式八项与段落六加一项走二级面板，插入那一档目前只有表格（其余插入项在 FP5）。行点击走 `run_document_menu_command`：撤销/剪切/拷贝/粘贴/切换视图派发 gpui 动作，行内格式与段落转换调编辑器层入口（FP2、FP4a 那两条），也就是快捷键动作处理器所调的同一组函数。面板尺寸与落点在 `DocumentMenuGeometry::measure` / `document_menu_origins`：宽度按最宽一行估，右侧放不下时二级面板翻到主面板左侧，下沿越界时向上收，离窗口边缘留 6px。快捷键那一列取 `default_shortcut_key`（默认键位；用户自定的绑定在 FP9 与偏好页一并接）。`Insert`→`Document` 之后 `context_menu_panel_width` 这个主题字段不再有消费者，仍留着给表格轴与图片菜单那条路。验收：`src/editor/tests/document_context_menu.rs` 从真实右键事件起，断言全部行 id、两种置灰口径的行序行高不变、二级面板与父行对齐、点「加粗」与「二级标题」改字节且撤销一步复原、贴边右键整个菜单留在视口内、快捷键列按默认键位显示；`document_menu::tests` 两条纯函数用例盯落点夹紧与面板宽度。

FP7 选中菜单（已落地）。新增 `Editor::selection_toolbar: Option<SelectionToolbarState>`（src/editor/mod.rs:287）与 src/editor/selection_toolbar.rs：锚点是 `selection_toolbar_anchor`（:109）——单块用块的 `selected_range`，跨块逐块量再并起来，为此在 Block 上把 `active_range_or_cursor_bounds` 里那一段量区间的算术拆成 `visible_range_bounds(range)`（src/components/block/runtime/text_ops.rs:555）；出现/消失口径见 §4。面板 7 项：「段落」下拉（六档标题 + 正文，行数据与右键菜单同一份 `document_submenu_rows`）加六颗行内格式按钮；按钮派发走 `run_selection_toolbar_command`（:523），调 FP2 与 FP4a 那两条编辑器层入口。落点 `toolbar_origin`（:163）：优先选区上方 8px、放不下改下方、横向居中并按视口收回，离边 6px；「段落」列表贴工具栏下沿，下方放不下贴到上沿（`heading_menu_offsets` :189）。本帧面板边界记在 `panel_bounds`，正文那层的按下落在里面时不当成正文落点（src/editor/selection.rs:81），否则一次按下先把选区收成光标、面板自己先消失。悬停说明复用 `HoverPreviewTooltip`，文字是「名字 + 默认键位」。验收：`src/editor/tests/selection_toolbar.rs` 九条，含真实拖动后浮出、拖动过程中不浮出、塌成光标收起、点加粗改字节且焦点与选区都不丢、点档位列表转标题且撤销一步复原、矮视口里不越界、与右键菜单互斥、源码模式不出、以及落点与离屏判定的两条纯函数用例。

FP8a 动作层：剪贴板的两条变体（已落地）。「粘贴为纯文本」把块的粘贴主体抽成 `Block::paste_from_clipboard(plain_only, window, cx)`（src/components/block/interactions/keys.rs），⌘V 传 `false`、⌘⇧V 传 `true`；`plain_only` 下不做两件事——不读粘贴板的 HTML 味道（跳过 `maybe_markdown_from_clipboard`），也不把「选中文字后粘一个网址」改写成链接（跳过 `paste_url_as_link` 那一条）。剪贴板里只有图片没有文字时仍走图片那一条：那是唯一能落的东西，按键没反应更让人以为坏了。「拷贝为 Markdown」是 `Editor::copy_as_markdown`（src/editor/workspace/documents.rs），取 `selected_markdown_text`（无选区时整篇源码，与「拷贝为 HTML」同一条口径）写进剪贴板的纯文本味道；与「拷贝」的差别在内容来源——那一条拿渲染后的可见文本（`**加粗**` 只剩「加粗」），这一条拿文件里的那几个字节。键位两条：`cmd-shift-v`/`ctrl-shift-v` 与 `cmd-shift-m`/`ctrl-shift-m`（都已核对不与现有绑定冲突）；菜单行 `paste-as-plain-text`、`copy-as-markdown` 排在主菜单编辑那组「粘贴」之后，两行都靠派发同一个动作到达，与键盘同一条路径。文案两个新键 × 5 处（`context_menu_paste_as_plain_text` 粘贴为纯文本 / Paste as Plain Text、`context_menu_copy_as_markdown` 拷贝为 Markdown / Copy as Markdown）；偏好页那两行先借这两个键，独立的 `preferences_shortcut_*` 键在 FP9 一并整。验收：`src/editor/tests/clipboard_text.rs` 六条——「粘贴」在选中的文字上粘网址要包成链接（对照）、「粘贴为纯文本」同一场景落的还是那几个字且一步撤销复原、「拷贝为 Markdown」给的是带 `**` 的源码、无选区时给整篇、菜单两行各一条等价用例（其中拷贝那条同时钉住文档字节一字不动）。

FP8b 动作层：清除格式。剥掉选区里的行内成对标记（`**`、`__`、`*`、`_`、`` ` ``、`==`、`^`、`~`），保留反斜杠转义与块级记号；已有的原语是 `InlineTextTree::unwrap_styles_on_fragments`（src/components/markdown/inline/tree.rs:859）。验收：三种样式叠在一处时只剥选区覆盖到的那部分，嵌套标记剥完不残缺。

FP9 文档与命令面板。`docs/architecture/` 里补选中菜单/右键菜单的入口、状态机与渲染层级；新命令进 `COMMANDS` 与 `SHORTCUT_DEFINITIONS`（有守卫测试要求两处对齐）。

## 6. 边界与代价

- 本期只覆盖渲染态（所见即所得）。源码模式的右键沿用现在的原生编辑行为，只加最基础的剪切/拷贝/粘贴段——源码模式下选区和块的语义不一致，做段落转换会把用户写的字面文本改掉。
- 跨块选区上做行内格式是逐块处理，不做「一段被拆开的粗体」这种跨块语法（markdown 本身不支持）。
- 高亮 `==x==` 是本仓库新语法。代价：老文件里成对出现的字面 `==` 会被解析成高亮（`1 == 2 and 3 == 4` 里的 `2 and 3` 会变成标记文本），代码块与行内代码内不解析，没配对的单个 `==` 保持字面。套叠的先后按样式栈序写回，`==**x**==` 会规范成 `**==x==**`（与 `~~` 已有的口径一致）。用户手写的 `<mark>x</mark>` 仍按原生 HTML 显示，不吸收成高亮样式——序列化只会写 `==`，吸收了就等于改掉用户没碰过的字节。
- 不改 `context_menu_panel_width` 的默认值给表格轴菜单和图片菜单，避免这两个菜单跟着变宽。
- 不做子菜单的键盘导航（右键菜单目前整体不支持方向键，那是另一期）。
- 有序列表的序号由所在列表组重算（`sync_block_list`，src/editor/tree.rs:937），文件里字面写的那个起始号（`5. 甲` 这种从 5 起跳的写法）解析后不留副本。代价：把一组中间的项换出去，它后面那些项的序号会从头排；同族的写法（`.` 与 `)`、`-` 与 `+`）另有 `list_marker` 记着，不受影响。打字打断一组时模型本来就是这么算的，这一笔没有另立口径。
- 并进同一个列表组时，接缝那个空行按紧排列表的写法收掉（`collect_root_markdown_lines` 里「前后都是列表项就不补空行」那条既有规则）。所以「一段正文紧跟在列表下面换成列表项」会把两行并成紧排，不是只加记号；换出去的接缝同理补回空行。
- 引用这一档接、标注那一档不接：引用是一根块、文字在自己标题里，换进换出不动别人；标注（`> [!note]`）自己带头部与正文子块，换出去要安置子块（FP4b-4）。跨两行的引用只许换成正文或取消引用，换标题会被拒（`#` 后面那行会被读回成另一块）。带子块的列表项只许在列表一族内部换种类，换成正文或标题时菜单那一行置灰。
- 代码块只给「退回正文」这一条出路，退回时围栏行与语言号（`rust` 这类信息串）一起收掉——那份信息属于围栏行，正文里没有地方存它，硬要保留就得改写正文，与「只动被波及那几行」冲突（要保留语言就先在源码模式改，或等后续把信息串单独记账）。代码块上弹不出正文右键菜单（FP6 之前就有这条口径，本笔不放宽），所以从代码块退回去要走选中工具栏那一档或键位；FP9 给「代码块」补键位时这条路径才与 Typora 的 F3 对齐。
- 链接这一档只接「选中了字」：空的 `[]()` 在行内树里存不住（写下去重新解析时那四个字符被当成空标签的链接丢掉，实测可见文本仍是原文），所以只有光标时菜单那一行与工具栏按钮都做不出东西，置灰处理——与格式那一档其余八行「要有选区才点得动」同一条口径。选中一段文字后粘贴网址另有 `Block::paste_url_as_link` 那条路（写字面 `[选中](地址)`），两者不重叠：一个补外壳等地址，一个手上已经有地址。

## 7. 风险

- R1：段落转换写前缀再 normalize，可能和「缓冲区是唯一事实源」的最小差异原则打架。做法是每次都开一个 `NonCoalescible` 撤销组，且断言改完的字节序列；FP4 若出现字节漂移（例如 CRLF 文件被洗成 LF），要先解决再往下走。
- R2：选中工具栏在滚动与缩放时的定位。已按「锚点每帧随选区重算」处理，选区整段不在视口里时由 `selection_is_on_screen`（:152）判掉；跨块时锚点要扫一遍可见块，成本是一次遍历加每块一次已缓存布局的取矩形，实测全量用例（含逐帧性能闸门）没有变化。
- R3：`toggle_inline_format` 依赖块自己的 `selected_range`。跨块选区时焦点块的 `selected_range` 只是选区尾部那一小块，直接用会只格式化最后一行——FP2 必须在 Editor 层按块切片。
- R4：菜单文案 5 个位点漏一个会在语言包导入校验（`i18n/manager.rs:205-222`）之外静默回退英文，靠 `config::preferences` 与 `i18n` 测试组兜住。
- R5（FP5a 量出来的既有缺陷，本笔没动打字那条路）：`Block::enter_math_block("")` 写的是 `$$\n\n$$`，而解析器把这份读回成两块原始 markdown——打字 `$$` 加回车产生的空公式块，存盘重开就散了。插入这一档因此自己写成 `$$\n$$`（读回来还是一块）。要修的是打字那一路：把 `enter_math_block` 的空正文改成相邻两行记号，并补一条「写下去的字节读回同一结构」的断言；它属于块内打字路径，不该塞进菜单这一笔。
- R6（本仓的格式化边界）：`rustfmt` 会跟着 `mod` 声明往下把整棵子目录重写。`src/editor/tests.rs` 一类只做声明的文件不能直接喂给 rustfmt（实测一次带出 28 个无关文件的改动），要格式化就只喂自己没有 `mod` 项的叶子文件，并且用 `--edition 2024`（本仓 edition 2024，2021 档遇到 let 链会直接报错退出）。
- R7（FP8a 量到、本笔没有查成因）：同一个窗口里连着走两次「选区 → 右键 → 点菜单行」，第二次的粘贴落点与选区不一致——实测得到 `"https://example.test 后\n\n另一段\n"`，期望是 `"https://example.test\n\n另一段\n"`（选区按 `0..14` 摆）。拆成两个用例、各自开新窗口走一次，两条都对，且拷贝那条另外断言了文档字节一字不动。可疑处在菜单收起后的焦点与选区恢复、以及第二次右键的相互作用；用例里的选区是直接写 `block.selected_range`，它不参与 gpui 的拖动选区会话，也可能是这一层。要查就先加一条「第二次右键之后选区仍是 `0..14`」的断言，定位是脚手架还是产品。
