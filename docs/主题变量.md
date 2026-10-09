# Velora 主题变量（token）参考

本文档是全量主题 token 表，供社区配色与自定义主题使用。主题 JSON 会随导出的 `themes/*.json` 保存，
字段名与下表一致；自定义主题只需给出要覆盖的字段，其余字段沿用默认主题。

顶层结构：`colors`（颜色）、`dimensions`（尺寸，单位 px，另有比例值）、`typography`（字体与字号）、`placeholders`（占位文案）。

> 一致性由测试 `theme_token_documentation_covers_every_token` 守护：主题结构体新增字段而本文档未同步时测试失败。

## colors（颜色，值形如 `#RRGGBBAA` 或主题 JSON 中的颜色对象）

| token | 类型 | 含义 |
|---|---|---|
| `editor_background` | 颜色 | 编辑器背景 |
| `source_mode_block_bg` | 颜色 | 源码模式块背景 |
| `comment_bg` | 颜色 | 注释块背景 |
| `text_default` | 颜色 | 正文文字 |
| `text_link` | 颜色 | 链接与正文粗体的强调色（标题保留自身文字色） |
| `text_placeholder` | 颜色 | 占位文字 |
| `text_h1` | 颜色 | 一级标题文字 |
| `text_h2` | 颜色 | 二级标题文字 |
| `text_h3` | 颜色 | 三级标题文字 |
| `text_h4` | 颜色 | 四级标题文字 |
| `text_h5` | 颜色 | 五级标题文字 |
| `text_h6` | 颜色 | 六级标题文字 |
| `border_h1` | 颜色 | 一级标题下边框 |
| `border_h2` | 颜色 | 二级标题下边框 |
| `text_quote` | 颜色 | 引用文字 |
| `border_quote` | 颜色 | 引用左边框 |
| `callout_note_bg` | 颜色 | 提示块 Note：背景 |
| `callout_note_border` | 颜色 | 提示块 Note：边框 |
| `callout_tip_bg` | 颜色 | 提示块 Tip：背景 |
| `callout_tip_border` | 颜色 | 提示块 Tip：边框 |
| `callout_important_bg` | 颜色 | 提示块 Important：背景 |
| `callout_important_border` | 颜色 | 提示块 Important：边框 |
| `callout_warning_bg` | 颜色 | 提示块 Warning：背景 |
| `search_highlight_bg` | 颜色 | 搜索命中高亮 |
| `search_active_highlight_bg` | 颜色 | 搜索当前活动命中高亮（比普通命中更深） |
| `callout_warning_border` | 颜色 | 提示块 Warning：边框 |
| `callout_caution_bg` | 颜色 | 提示块 Caution：背景 |
| `callout_caution_border` | 颜色 | 提示块 Caution：边框 |
| `footnote_bg` | 颜色 | 脚注背景 |
| `footnote_border` | 颜色 | 脚注边框 |
| `footnote_badge_bg` | 颜色 | 脚注角标背景 |
| `footnote_badge_text` | 颜色 | 脚注角标文字 |
| `footnote_backref` | 颜色 | 脚注返回箭头 |
| `task_checkbox_border` | 颜色 | 任务复选框：边框 |
| `task_checkbox_bg` | 颜色 | 任务复选框：背景 |
| `task_checkbox_checked_bg` | 颜色 | 任务复选框：选中背景 |
| `task_checkbox_check` | 颜色 | 任务复选框：勾选标记 |
| `separator_color` | 颜色 | 分隔线 |
| `code_bg` | 颜色 | 代码块背景 |
| `code_text` | 颜色 | 代码块正文 |
| `code_language_input_bg` | 颜色 | 语言输入框：背景 |
| `code_language_input_border` | 颜色 | 语言输入框：边框 |
| `code_language_input_text` | 颜色 | 语言输入框：文字 |
| `code_language_input_placeholder` | 颜色 | 语言输入框：占位文字 |
| `code_syntax_comment` | 颜色 | 代码高亮：注释 |
| `code_syntax_keyword` | 颜色 | 代码高亮：关键字 |
| `code_syntax_string` | 颜色 | 代码高亮：字符串 |
| `code_syntax_number` | 颜色 | 代码高亮：数字 |
| `code_syntax_type` | 颜色 | 代码高亮：类型 |
| `code_syntax_function` | 颜色 | 代码高亮：函数名 |
| `code_syntax_constant` | 颜色 | 代码高亮：常量 |
| `code_syntax_variable` | 颜色 | 代码高亮：变量 |
| `code_syntax_property` | 颜色 | 代码高亮：属性 |
| `code_syntax_operator` | 颜色 | 代码高亮：运算符 |
| `code_syntax_punctuation` | 颜色 | 代码高亮：标点 |
| `md_syntax_heading1` | 颜色 | Markdown 源码高亮：一级标题 |
| `md_syntax_heading2` | 颜色 | Markdown 源码高亮：二级标题 |
| `md_syntax_heading3` | 颜色 | Markdown 源码高亮：三级标题 |
| `md_syntax_heading4` | 颜色 | Markdown 源码高亮：四级标题 |
| `md_syntax_heading5` | 颜色 | Markdown 源码高亮：五级标题 |
| `md_syntax_heading6` | 颜色 | Markdown 源码高亮：六级标题 |
| `md_syntax_marker` | 颜色 | Markdown 源码高亮：结构记号（列表、引用、围栏、分隔线、`$$`） |
| `md_syntax_strong` | 颜色 | Markdown 源码高亮：加粗内容（配粗体字重） |
| `md_syntax_code` | 颜色 | Markdown 源码高亮：行内代码、公式正文与转义 |
| `md_syntax_link_text` | 颜色 | Markdown 源码高亮：链接文字 |
| `md_syntax_link_url` | 颜色 | Markdown 源码高亮：链接地址 |
| `md_syntax_label` | 颜色 | Markdown 源码高亮：标签、标注标签与脚注引用 |
| `table_border` | 颜色 | 表格边框 |
| `table_header_bg` | 颜色 | 表头背景 |
| `current_line_bg` | 颜色 | 当前行高亮背景 |
| `matching_bracket_bg` | 颜色 | 括号匹配高亮背景 |
| `table_cell_bg` | 颜色 | 单元格背景 |
| `table_cell_active_outline` | 颜色 | 表格：活动单元格描边 |
| `table_axis_preview_bg` | 颜色 | 表格：行列预览背景 |
| `table_axis_selected_bg` | 颜色 | 表格：行列选中背景 |
| `table_append_button_bg` | 颜色 | 表格：追加按钮背景 |
| `table_append_button_hover` | 颜色 | 表格：追加按钮悬停 |
| `table_append_button_text` | 颜色 | 表格：追加按钮文字 |
| `image_placeholder_bg` | 颜色 | 图片占位背景 |
| `image_placeholder_border` | 颜色 | 图片占位边框 |
| `image_placeholder_text` | 颜色 | 图片占位文字 |
| `image_caption_text` | 颜色 | 图注文字 |
| `scrollbar_thumb` | 颜色 | 滚动条滑块 |
| `cursor` | 颜色 | 光标 |
| `selection` | 颜色 | 选区 |
| `dialog_backdrop` | 颜色 | 对话框遮罩 |
| `dialog_surface` | 颜色 | 对话框面板 |
| `dialog_border` | 颜色 | 对话框边框 |
| `dialog_title` | 颜色 | 对话框标题 |
| `dialog_body` | 颜色 | 对话框正文 |
| `dialog_muted` | 颜色 | 对话框次要文字 |
| `dialog_primary_button_bg` | 颜色 | 对话框：primary_button_bg |
| `dialog_primary_button_hover` | 颜色 | 对话框：primary_button_hover |
| `dialog_primary_button_text` | 颜色 | 对话框：primary_button_text |
| `dialog_secondary_button_bg` | 颜色 | 对话框：secondary_button_bg |
| `dialog_secondary_button_hover` | 颜色 | 对话框：secondary_button_hover |
| `dialog_secondary_button_text` | 颜色 | 对话框：secondary_button_text |
| `dialog_danger_button_bg` | 颜色 | 对话框：danger_button_bg |
| `dialog_danger_button_hover` | 颜色 | 对话框：danger_button_hover |
| `dialog_danger_button_text` | 颜色 | 对话框：danger_button_text |
| `status_bar_background` | 颜色 | 状态栏背景 |
| `status_bar_text` | 颜色 | 状态栏文字 |
| `status_bar_text_dim` | 颜色 | 状态栏次要文字 |
| `status_bar_button_hover` | 颜色 | 状态栏按钮悬停 |

## dimensions（尺寸）

| token | 类型 | 含义 |
|---|---|---|
| `editor_padding` | 数值（px） | 编辑器四周内边距 |
| `writing_max_width` | 数值（px） | 正文最大宽度（仅写作列宽选「跟随主题」时生效） |
| `block_gap` | 数值（px） | 块间距 |
| `block_min_height` | 数值（px） | 块最小高度 |
| `block_padding_y` | 数值（px） | 块垂直内边距 |
| `block_padding_x` | 数值（px） | 块水平内边距 |
| `nested_block_indent` | 数值（px） | 嵌套缩进 |
| `list_marker_gap` | 数值（px） | 列表标记：标记与内容间距 |
| `list_marker_width` | 数值（px） | 列表标记：标记宽度 |
| `ordered_list_marker_width` | 数值（px） | 有序列表标记：宽度 |
| `task_checkbox_size` | 数值（px） | 任务复选框：尺寸 |
| `task_checkbox_radius` | 数值（px） | 任务复选框：圆角 |
| `task_checkbox_border_width` | 数值（px） | 任务复选框：边框宽度 |
| `task_checkbox_check_size` | 数值（px） | 任务复选框：勾选标记尺寸 |
| `h1_padding_bottom` | 数值（px） | 一级标题：下内边距 |
| `h1_margin_bottom` | 数值（px） | 一级标题：下外边距 |
| `cursor_width` | 数值（px） | 光标宽度 |
| `underline_thickness` | 数值（px） | 下划线粗细 |
| `h1_border_width` | 数值（px） | 一级标题：下边框宽度 |
| `quote_border_width` | 数值（px） | 引用：左边框宽度 |
| `quote_padding_left` | 数值（px） | 引用：左内边距 |
| `callout_padding_x` | 数值（px） | 提示块：水平内边距 |
| `callout_padding_y` | 数值（px） | 提示块：垂直内边距 |
| `callout_body_gap` | 数值（px） | 提示块：正文间距 |
| `callout_radius` | 数值（px） | 提示块：圆角 |
| `callout_border_width` | 数值（px） | 提示块：边框宽度 |
| `callout_header_gap` | 数值（px） | 提示块：标题间距 |
| `callout_header_margin_bottom` | 数值（px） | 提示块：标题下外边距 |
| `footnote_padding_x` | 数值（px） | 脚注：水平内边距 |
| `footnote_padding_y` | 数值（px） | 脚注：垂直内边距 |
| `footnote_radius` | 数值（px） | 脚注：圆角 |
| `footnote_badge_padding_x` | 数值（px） | 脚注：角标水平内边距 |
| `footnote_badge_padding_y` | 数值（px） | 脚注：角标垂直内边距 |
| `separator_thickness` | 数值（px） | 分隔线：线宽 |
| `separator_inset_x` | 数值（px） | 分隔线：水平内缩 |
| `separator_margin_y` | 数值（px） | 分隔线：垂直外边距 |
| `code_block_padding_y` | 数值（px） | 代码块：垂直内边距 |
| `code_block_padding_x` | 数值（px） | 代码块：水平内边距 |
| `code_bg_pad_x` | 数值（px） | 代码：背景水平内边距 |
| `code_bg_pad_y` | 数值（px） | 代码：背景垂直内边距 |
| `code_bg_radius` | 数值（px） | 代码：背景圆角 |
| `code_language_input_width` | 数值（px） | 语言输入框：宽度 |
| `code_language_input_height` | 数值（px） | 语言输入框：高度 |
| `code_language_input_padding_x` | 数值（px） | 语言输入框：水平内边距 |
| `code_language_input_padding_y` | 数值（px） | 语言输入框：垂直内边距 |
| `code_language_input_radius` | 数值（px） | 语言输入框：圆角 |
| `code_language_input_border_width` | 数值（px） | 语言输入框：边框宽度 |
| `code_language_input_gap` | 数值（px） | 语言输入框：与代码间距 |
| `table_cell_padding_x` | 数值（px） | 表格：单元格水平内边距 |
| `table_cell_padding_y` | 数值（px） | 表格：单元格垂直内边距 |
| `table_cell_min_height` | 数值（px） | 表格：单元格最小高度 |
| `table_append_button_extent` | 数值（px） | 表格：追加按钮尺寸 |
| `table_append_button_inset` | 数值（px） | 表格：追加按钮偏移 |
| `table_append_activation_band` | 数值（px） | 表格：追加激活带宽度 |
| `image_radius` | 数值（px） | 图片：圆角 |
| `image_root_max_height` | 数值（px） | 图片：最大高度，默认 0，按原图比例排版、不设固定上限；数学公式在值为 0 时仍限高 420px |
| `image_root_max_width` | 数值（px） | 图片：根级最大宽度，默认 0，以正文容器宽度的 95% 作为缩放 100% 的基准；始终受容器宽度约束 |
| `image_cell_max_height` | 数值（px） | 图片：单元格内最大高度 |
| `image_root_placeholder_height` | 数值（px） | 图片：根级占位高度 |
| `image_cell_placeholder_height` | 数值（px） | 图片：单元格占位高度 |
| `image_caption_gap` | 数值（px） | 图片：图注间距 |
| `scrollbar_width` | 数值（px） | 滚动条：宽度 |
| `scrollbar_right` | 数值（px） | 滚动条：距右偏移 |
| `centered_shrink_start` | 数值（px） | 居中版心：收缩起点 |
| `centered_shrink_end` | 数值（px） | 居中版心：收缩终点 |
| `centered_min_ratio` | 数值（px） | 居中版心：最小占比 |
| `dialog_width` | 数值（px） | 对话框：宽度 |
| `dialog_padding` | 数值（px） | 对话框：内边距 |
| `dialog_gap` | 数值（px） | 对话框：间距 |
| `dialog_radius` | 数值（px） | 对话框：圆角 |
| `dialog_border_width` | 数值（px） | 对话框：边框宽度 |
| `dialog_button_height` | 数值（px） | 对话框：按钮高度 |
| `dialog_button_gap` | 数值（px） | 对话框：按钮间距 |
| `dialog_button_padding_x` | 数值（px） | 对话框：按钮水平内边距 |
| `menu_bar_height` | 数值（px） | 菜单：菜单栏高度 |
| `menu_bar_padding_x` | 数值（px） | 菜单：菜单栏水平内边距 |
| `menu_bar_padding_y` | 数值（px） | 菜单：菜单栏垂直内边距 |
| `menu_bar_gap` | 数值（px） | 菜单：菜单栏项间距 |
| `menu_bar_button_width` | 数值（px） | 菜单：菜单栏按钮宽度 |
| `menu_bar_button_height` | 数值（px） | 菜单：菜单栏按钮高度 |
| `menu_bar_button_padding_x` | 数值（px） | 菜单：菜单栏按钮水平内边距 |
| `menu_bar_button_radius` | 数值（px） | 菜单：菜单栏按钮圆角 |
| `menu_text_size` | 数值（px） | 菜单：菜单字号 |
| `menu_panel_top` | 数值（px） | 菜单：面板顶部偏移 |
| `menu_panel_width` | 数值（px） | 菜单：面板宽度 |
| `menu_panel_padding` | 数值（px） | 菜单：面板内边距 |
| `menu_panel_gap` | 数值（px） | 菜单：面板项间距 |
| `menu_panel_radius` | 数值（px） | 菜单：面板圆角 |
| `menu_item_height` | 数值（px） | 菜单：菜单项高度 |
| `menu_item_padding_x` | 数值（px） | 菜单：菜单项水平内边距 |
| `menu_item_radius` | 数值（px） | 菜单：菜单项圆角 |
| `menu_separator_margin_x` | 数值（px） | 菜单：分隔线水平外边距 |
| `menu_separator_margin_y` | 数值（px） | 菜单：分隔线垂直外边距 |
| `menu_separator_height` | 数值（px） | 菜单：分隔线高度 |
| `context_menu_panel_width` | 数值（px） | 右键菜单：面板宽度 |
| `context_menu_submenu_width` | 数值（px） | 右键菜单：子菜单宽度 |
| `context_menu_submenu_gap` | 数值（px） | 右键菜单：子菜单偏移 |
| `context_menu_axis_panel_width` | 数值（px） | 右键菜单：行列面板宽度 |
| `table_insert_dialog_width` | 数值（px） | 表格：插入对话框宽度 |
| `table_insert_stepper_gap` | 数值（px） | 表格：insert_stepper_gap |
| `table_insert_stepper_button_size` | 数值（px） | 表格：insert_stepper_button_size |
| `table_insert_stepper_value_min_width` | 数值（px） | 表格：insert_stepper_value_min_width |
| `table_insert_stepper_value_padding_x` | 数值（px） | 表格：insert_stepper_value_padding_x |
| `table_insert_stepper_radius` | 数值（px） | 表格：insert_stepper_radius |
| `view_mode_toggle_left` | 数值（px） | 视图切换按钮：左偏移 |
| `view_mode_toggle_bottom` | 数值（px） | 视图切换按钮：底偏移 |
| `view_mode_toggle_padding_x` | 数值（px） | 视图切换按钮：水平内边距 |
| `view_mode_toggle_padding_y` | 数值（px） | 视图切换按钮：垂直内边距 |
| `view_mode_toggle_min_width` | 数值（px） | 视图切换按钮：最小宽度 |
| `view_mode_toggle_radius` | 数值（px） | 视图切换按钮：圆角 |
| `view_mode_toggle_border_width` | 数值（px） | 视图切换按钮：边框宽度 |
| `view_mode_toggle_text_size` | 数值（px） | 视图切换按钮：字号 |
| `status_bar_height` | 数值（px） | 状态栏：高度 |
| `status_bar_padding_x` | 数值（px） | 状态栏：水平内边距 |
| `status_bar_item_gap` | 数值（px） | 状态栏：项间距 |
| `status_bar_text_size` | 数值（px） | 状态栏：字号 |

## typography（字体与字号）

| token | 类型 | 含义 |
|---|---|---|
| `body_font_family` | 字符串 | 正文字体族 |
| `heading_font_family` | 字符串 | 标题字体族 |
| `text_size` | 数值（px） | 正文字号 |
| `text_line_height` | 数值（px） | 正文行高（倍数） |
| `text_letter_spacing` | 数值（em） | 正文字距；不拉伸代码内部字符 |
| `h1_size` | 数值（px） | 一级标题字号 |
| `h1_weight` | FontWeightDef | 一级标题字重 |
| `h2_size` | 数值（px） | 二级标题字号 |
| `h2_weight` | FontWeightDef | 二级标题字重 |
| `h3_size` | 数值（px） | 三级标题字号 |
| `h3_weight` | FontWeightDef | 三级标题字重 |
| `h4_size` | 数值（px） | 四级标题字号 |
| `h4_weight` | FontWeightDef | 四级标题字重 |
| `h5_size` | 数值（px） | 五级标题字号 |
| `h5_weight` | FontWeightDef | 五级标题字重 |
| `h6_size` | 数值（px） | 六级标题字号 |
| `h6_weight` | FontWeightDef | 六级标题字重 |
| `code_size` | 数值（px） | 代码字号 |
| `dialog_title_size` | 数值（px） | 对话框标题字号 |
| `dialog_title_weight` | FontWeightDef | 对话框标题字重 |
| `dialog_body_size` | 数值（px） | 对话框正文字号 |
| `dialog_body_weight` | FontWeightDef | 对话框正文字重 |
| `dialog_button_size` | 数值（px） | 对话框按钮字号 |
| `dialog_button_weight` | FontWeightDef | 对话框按钮字重 |

## placeholders（占位文案）

| token | 类型 | 含义 |
|---|---|---|
| `empty_editing` | 字符串 | 空文档占位提示文案 |
