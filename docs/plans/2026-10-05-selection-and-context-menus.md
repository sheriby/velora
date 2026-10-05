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

- 一行，横向排布，图标按钮 26×26，圆角 5，图标 14px，颜色用 `dialog_secondary_button_text`，悬停 `dialog_secondary_button_hover`，按下有 0.92 透明度反馈。
- 位置：选区外接框的上方 8px；上方放不下就翻到下方 8px；左右按视口宽度夹紧（沿用 `context_menus.rs:99-107` 的 8px 夹紧写法）。
- 出现时机：鼠标或键盘产生非空选区后出现；选区塌成光标、焦点离开文档、开始滚动、切换视图模式时立刻消失。不跟随选区拖动的中间过程（拖选中每次移动都会重排位置，视觉上会抖），只在选区确定后（鼠标抬起 / 键盘扩展一步）定位一次。
- 工具栏本身不吃键盘焦点（gpui 里按钮按下不影响块的焦点），保证按下去之后 ⌘Z、方向键仍然作用在文档上。
- 每个按钮有悬停说明，写明中文名和快捷键。

右键菜单：

- 单列面板宽 200（现有 132 只够放两个字，本期把正文右键单独放宽；表格轴菜单、图片菜单仍用现有宽度）。
- 分节用 1px 分隔线，顺序固定：编辑 → 格式 → 段落 → 插入 → 视图。段落与插入内部用二级子菜单，避免一个 30 行的长菜单。
- 每行右侧显示快捷键（灰 `dialog_muted`），没有快捷键的留空位对齐。
- 不可用的项（例如没有选区时的「拷贝」「清除格式」）置灰不隐藏，鼠标位置不变——菜单宽度变化会让连点两次右键跳位置。
- 右键时如果当前有选区，先保住选区（现在切到别的块会清空），菜单动作全部围绕这段选区。

## 5. 功能点拆分（按提交顺序）

每个功能点自己跑定向测试 + 全量 + clippy，写完就提交一笔，不攒。

FP1 菜单行渲染收口。新增一个通用行渲染：`(id, label, shortcut: Option<&str>, enabled, danger, on_click)`，支持分隔线与「右箭头 + 子菜单」；把 `render_axis_menu_item`、文件树菜单、标签菜单、插入子菜单里那 4 份重复行代码和 5 份分隔线代码收过来。验收：现有 3 个菜单测试仍通过；新增一条测试断言同一段代码渲染出的行高、内边距、快捷键文字位置一致。

FP2 动作层：选区上的一行内格式。给 `InlineFormat` 加 `Strikethrough / Superscript / Subscript`，接上 `InlineTextTree` 已有的 `toggle_style`（去掉 `toggle_strikethrough` 的 `dead_code`），并把 toggle 入口提到 `Editor`：`toggle_inline_format_on_selection(InlineFormat, cx)`（`src/editor/format_ops.rs`），跨块选区按块逐个处理，全程一个撤销组；块的 `toggle_inline_format_in_range` 收**可见文本**坐标，块内自己换算到树内坐标，Editor 层不必知道标记占位。上标/下标按本仓库既有写法落 `<sup>x</sup>` / `<sub>x</sub>`，与 Typora 的默认输出一致（Typora 也认 `^x^` / `~x~`，读入路径已有）。置灰要用的判定函数（选区能否做行内格式）随 FP6 一起进，这一笔没有消费者，进来就是警告。验收：单块、跨块、空选区三类用例。

FP3 高亮 `==x==`。新增 `StyleFlag::Highlight` + 分隔符解析 + 渲染颜色（用主题的强调色，浅色主题黄底、深色主题低饱和黄底），加 `InlineFormat::Highlight`。验收：解析往返测试（`==x==` 读进来 → 存出去不变形），toggle 用例，已有 markdown 测试无回归。

FP4 动作层：段落转换。新增 `Editor::apply_block_kind_to_selection(BlockKindTarget, cx)`，`BlockKindTarget = Heading(1..=6) | Paragraph | Blockquote | BulletList | OrderedList | TaskList | CodeBlock | Separator`。实现按前缀写回缓冲区（`# ` / `> ` / `- ` / `1. ` / `- [ ] `）并走 `normalize_after_title_edit`，代码块与分隔线复用 `enter_code_block` / `convert_to_separator`。同一段落重复点同一个目标要能取消（标题→正文）。验收：每种目标一条断言（缓冲区字节 + 根块类型 + 撤销一步回到原样）。

FP5 动作层：链接、图片、插入类。链接走应用内小输入框（不再是系统弹窗，遵循本仓库「不用系统原生弹窗」的规矩），确定后调 `paste_url_as_link`；图片选择器复用粘贴路径；目录插 `[toc]`、Front Matter 插 `---\n---`、公式块复用 `enter_math_block`。验收：链接包裹选区的字节断言；目录/Front Matter 落到缓冲区且能被解析。

FP6 右键菜单成形。把 `ContextMenuState::Insert` 扩成 `Document`，渲染编辑/格式/段落/插入/视图五段，段落与插入用二级子菜单；不可用项置灰；菜单宽度与夹紧按 §4。验收：菜单渲染出全部行 id；有选区与无选区两种置灰；点「粗体」「二级标题」确实改缓冲区且撤销一步复原。

FP7 选中菜单。新增 `Editor::selection_toolbar: Option<SelectionToolbarState>`，锚点用 `active_range_or_cursor_bounds` + 跨块选区外接框；出现/消失规则按 §4；`render/paint.rs` 的层级里放在右键菜单之下、正文之上。验收：拖出选区后 `debug_bounds` 能取到工具栏；塌成光标后消失；点按钮改字节且文档焦点没丢。

FP8 剪贴板补全。「粘贴为纯文本」（跳过 `html_paste` 转换）、「拷贝为 Markdown」（选区的 markdown 文本进剪贴板）、「清除格式」（剥掉选区里的 `** _ ` == ^ ~` 成对标记，保留反斜杠转义）各自动作 + 菜单行 + 快捷键位。验收：三条行为断言 + 清除格式对嵌套标记的用例。

FP9 文档与命令面板。`docs/architecture/` 里补选中菜单/右键菜单的入口、状态机与渲染层级；新命令进 `COMMANDS` 与 `SHORTCUT_DEFINITIONS`（有守卫测试要求两处对齐）。

## 6. 边界与代价

- 本期只覆盖渲染态（所见即所得）。源码模式的右键沿用现在的原生编辑行为，只加最基础的剪切/拷贝/粘贴段——源码模式下选区和块的语义不一致，做段落转换会把用户写的字面文本改掉。
- 跨块选区上做行内格式是逐块处理，不做「一段被拆开的粗体」这种跨块语法（markdown 本身不支持）。
- 高亮 `==x==` 是本仓库新语法。代价：老文件里成对出现的字面 `==` 会被解析成高亮（`1 == 2 and 3 == 4` 里的 `2 and 3` 会变成标记文本），代码块与行内代码内不解析，没配对的单个 `==` 保持字面。套叠的先后按样式栈序写回，`==**x**==` 会规范成 `**==x==**`（与 `~~` 已有的口径一致）。用户手写的 `<mark>x</mark>` 仍按原生 HTML 显示，不吸收成高亮样式——序列化只会写 `==`，吸收了就等于改掉用户没碰过的字节。
- 不改 `context_menu_panel_width` 的默认值给表格轴菜单和图片菜单，避免这两个菜单跟着变宽。
- 不做子菜单的键盘导航（右键菜单目前整体不支持方向键，那是另一期）。

## 7. 风险

- R1：段落转换写前缀再 normalize，可能和「缓冲区是唯一事实源」的最小差异原则打架。做法是每次都开一个 `NonCoalescible` 撤销组，且断言改完的字节序列；FP4 若出现字节漂移（例如 CRLF 文件被洗成 LF），要先解决再往下走。
- R2：选中工具栏在滚动与缩放时的定位。gpui 的绘制坐标随滚动变化，锚点每帧重算成本可控，但选区滚动出视口时要把工具栏藏掉。
- R3：`toggle_inline_format` 依赖块自己的 `selected_range`。跨块选区时焦点块的 `selected_range` 只是选区尾部那一小块，直接用会只格式化最后一行——FP2 必须在 Editor 层按块切片。
- R4：菜单文案 5 个位点漏一个会在语言包导入校验（`i18n/manager.rs:205-222`）之外静默回退英文，靠 `config::preferences` 与 `i18n` 测试组兜住。
