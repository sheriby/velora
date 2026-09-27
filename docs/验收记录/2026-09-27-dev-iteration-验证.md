# dev 分支迭代验证记录（2026-09-27）

## 第二批补充（G1/D2/C5/E2/B3/A3 配置）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| G1 原子写入 | 2 个单测：替换成功无临时残留 / 失败保原文 | 通过 |
| D2 排序 | 类型排序测试 + workspace 全部用例 | 通过 |
| C5 大纲跟随 | workspace 全部用例；视觉待解锁屏复核 | 通过（待实机复核） |
| E2 命令面板 | menu 34 项全过（视图菜单索引断言更新） | 通过 |
| B3 HTML 粘贴 | 4 个转换器单测（标题/粗斜体/链接/列表/代码块/plain 回退） | 通过 |
| A3 默认窗口尺寸 | config 全部用例 | 通过 |


## 范围

roadmap（docs/plans/2026-09-27-dev-iteration-roadmap.md，62 项）中已完成 12 项：

A1 欢迎页、A2 窗口 frame 持久化、A4 会话恢复、A5 字号缩放、B2 搜索文档内
高亮、C1 frontmatter 保留、D1 文件树定位、D3 外部变更监听、D4 垃圾桶删除、
E1 快速切换器、E4 中键关标签、E5 ⌘1-9 切标签、G3 自动保存防抖可配置。

## 验证方式与结论

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| A1 欢迎页 | gpui 渲染烟雾测试 + 状态转换测试（welcome_page_renders/hides） | 通过 |
| A2 窗口 frame | config.toml 序列化断言（remember_bounds = true）+ 保存/读取回环 | 通过 |
| A4 会话恢复 | session.json 保存/读取/缺省回退测试 | 通过 |
| A5 缩放 | 全量测试通过；⌘=/⌘-/⌘0 绑定注册 | 通过 |
| B2 高亮 | 编译期映射正确性 + search 全部用例 | 通过（视觉效果待解锁屏截图复核） |
| C1 frontmatter | 两个 gpui 测试：保留为 opaque 块 / 非 frontmatter 的 --- 仍为分隔线 | 通过 |
| D1 树定位 | workspace 全部用例通过 | 通过 |
| D3 文件监听 | 编译 + workspace 用例；运行时行为需解锁后实测 | 待实机复核 |
| D4 垃圾桶 | 编译通过；workspace 用例通过 | 通过 |
| E1/E4/E5/E 快捷键 | tab 相关 93 项用例通过 | 通过 |

## 第三批补充（B4/B5/E1/E2/G1/D2 等）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| B4 环绕 | 3 个 gpui 测试（星号/括号/空选区） | 通过 |
| B5 URL 链接 | gpui 测试（选区+URL → 链接） | 通过 |
| E1 快速切换器 | 编译 + 模糊匹配单测 | 通过 |
| E2 命令面板 | menu 34 项（含视图菜单新索引断言） | 通过 |
| G1 原子写入 | 2 个单测 | 通过 |
| D2 排序 | 类型排序测试 | 通过 |
| C5 大纲跟随 | workspace 全部用例 | 通过（视觉待复核） |

## 第四批补充（C6/D5/D7/E8/G2）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| C6 大纲重命名 | gpui 测试：双击选中标题文本（0..9） | 通过 |
| E8 面包屑 | 渲染烟雾测试 + 路径状态断言（canonicalize 修正） | 通过 |
| D5 模板 | config 往返测试 | 通过 |
| D7 树 tooltip | 编译 + workspace 用例 | 通过 |
| G2 编码探测 | has_utf16_bom 单测（LE/BE/UTF-8） | 通过 |

## 第五批补充（D8/D5/D7/G2/E8/C6）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| C6 大纲双击重命名 | gpui 测试（标题选区 0..9） | 通过 |
| E8 面包屑 | 渲染烟雾测试 | 通过 |
| D5 新建模板 | config 往返测试 | 通过 |
| D7 树 tooltip | 编译 + workspace 用例 | 通过 |
| G2 UTF-16 探测 | has_utf16_bom 单测（LE/BE/UTF-8） | 通过 |
| D8 树过滤 | workspace 全部用例 | 通过 |

## 第六批补充（E6/D8）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| E6 光标历史 | workspace 全部用例；⌥⌘←/→ 绑定注册 | 通过 |
| D8 树过滤 | workspace 全部用例 | 通过 |

## 第七批补充（C3/C4/E6/G4）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| C3 wikilink | 解析单测 + 创建/打开 2 个 gpui 测试（canonicalize 断言） | 通过 |
| C4 标签 | tag_query 单测（CJK/数字/下划线/空格拒绝） | 通过 |
| E6 光标历史 | workspace 全部用例 | 通过 |
| G4 大文件基准 | 10 MiB 打开实测 17.5s（debug，159,683 块 ≈109µs/块）；断言 ≤400µs/块 防回归；3s 预算依赖 G8 惰性建块 | 通过（预算记录在案） |

## 第八批补充（G6 演练 / G7 快照）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| G6 崩溃恢复演练 | crash_recovery_drill 四阶段 gpui 测试 | 通过 |
| G7 渲染快照 | render_structure_snapshot 黄金快照（含列表组分隔空段、表格空 display_text 两处设计使然差异说明） | 通过 |

## 第九批补充（E3 拖拽排序 / F1 单文件导出验证）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| E3 标签拖拽 | tab 全部 93 项用例 | 通过 |
| F1 单文件 HTML | single_file_export_embeds_local_images_as_data_uris 测试 | 通过 |

## 第十批补充（F2 复制为 HTML）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| F2 复制为 HTML | 既有渲染测试覆盖 HTML 生成；⇧⌘C 绑定与导出菜单项注册；剪贴板写入走 ClipboardItem | 通过 |

## 第十一批补充（C10 图片拖拽缩放 v1）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| C10 图片缩放 | 编译+全量用例；渲染态根级图片手柄拖拽实时调整宽度因子 | 通过（视觉待解锁复核；源码写回待 v2） |

## 第十二批补充（H1 设置界面批次一）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| H1 文件页三控件 | config 33 项测试（含新三项持久化断言） | 通过 |

## 第十三批补充（C7 标题行内折叠 chevron）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| C7 chevron UI | 新增 3 个 gpui 用例：`heading_fold_chevron_marks_only_foldable_headings`（含内容/空章节标题的 foldable 判定）、`heading_fold_chevron_toggle_hides_section_and_refocuses_heading`（事件折叠后章节隐藏 + 光标回退标题）、`heading_fold_chevron_renders_and_click_toggles_fold`（debug_bounds 取真实布局 + simulate_click 点击 chevron 端到端折叠） | 通过 |
| 全量回归 | `cargo test` 834 通过（831 基线 + 3 新增）；唯一失败仍为基线偶发项 | 通过（视觉待解锁复核） |

## 第十四批补充（D9 文件树扫描异步化）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| D9 后台扫描 | 扫描改在 background executor 执行，代数校验丢弃过期结果；新增 3 个 gpui 用例：`workspace_tree_scan_is_async_and_applies_result`（调用栈内不产出树、pump 后落地）、`workspace_tree_scan_discards_stale_root_results`（换根丢弃旧结果）、`reopening_workspace_root_rescans_after_tree_is_cleared`（同根重开必须重扫） | 通过 |
| 全量回归 | `cargo test` 838 通过 0 失败（831 基线 + 3 C7 + 3 D9 + 重开用例；基线偶发项本轮通过） | 通过（视觉待解锁复核） |

## 第十五批补充（B8 阅读时间 / B11 用例 / 偶发失败定位）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| B8 阅读时间 | `reading_time_is_rounded_up_at_300_words_per_minute`（0→隐藏、1→1、300→1、301→2、1500→5） | 通过 |
| B11 撤销跨模式 | 新增 `undo_after_view_mode_switch_keeps_text`：切换前后撤销/重做均不丢字 | 通过 |
| 偶发失败定位 | `autosave_does_not_overwrite_external_file_changes` 失败根因＝冲突标记寄存在 `workspace.file_error` 上、被文件树扫描清空；改为独立 `external_change_conflict` 状态后连跑 20 次 0 失败；新增 `external_autosave_conflict_survives_workspace_rescan` 并做旧实现正控（旧实现下必失败） | 通过 |
| 测试隔离 | 测试构建配置目录改指进程级临时目录（此前测试把偏好/会话/恢复快照写进真实用户目录，导致 `cargo run` 恢复出几十个窗口） | 通过 |
| 全量回归 | `cargo test` 841 通过 0 失败 | 通过 |

## 第十六批补充（A6/B10/C10 v2/B6 智能标点与全屏）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| A6 全屏切换 | 菜单项 + 快捷键（ctrl-cmd-f / f11）注册；`toggle_fullscreen_has_default_shortcuts` 断言默认键位；菜单结构测试随新项顺延索引（`build_menus_uses_*`） | 通过（标题栏沿用既有 is_fullscreen 逻辑） |
| B10 图片命名模板 | `pasted_image_hash_is_stable_and_content_sensitive` + `clipboard_image_name_uses_date_and_hash_template`（`YYYY-MM-DD-<8位哈希>.<ext>`，冲突仍追加序号） | 通过 |
| C10 v2 宽度写回 | 解析端 `parses_image_with_trailing_width_attribute` / `rejects_malformed_width_attribute`；渲染端 `image_width_attribute_seeds_resize_factor`、`resizing_image_writes_width_attribute_back_to_markdown`（40% 写回）、`resizing_image_back_to_full_width_removes_attribute`（100% 移除） | 通过 |
| B6 智能标点 | 纯函数单测（开合引号上下文、`--`→破折号）+ gpui 用例：开启后 `"`→`”`、`--`→`—`，关闭后保持直引号；`save/read` 往返断言 `smart_punctuation = true` 落盘；偏好文件页新增开关 | 通过 |
| 全量回归 | `cargo test` 854 通过 0 失败（含 `crash_recovery_drill_snapshot_restore_save` 基线偶发项本轮通过） | 通过 |

## 第十七批补充（B12/F2 增强/D6/H3/H4/E10）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| B12 长块护栏 | `long_paragraph_renders_as_plain_source`（20k+ 字节块渲染 `block-long-source` 元素且状态栏提示逻辑成立）+ `long_source_block_hint_tracks_single_long_line`（边界：等于阈值不触发） | 通过 |
| F2 增强 | gpui 剪贴板项新增 HTML flavor：`copy_as_html_writes_html_source_to_clipboard`（纯文本仍为 HTML 源码）+ `copy_as_html_item_carries_html_flavor`（`html()` 返回富文本载荷） | 通过（macOS 写 `NSPasteboardTypeHTML`） |
| D6 树剪贴板 | `tree_copy_then_paste_duplicates_file_into_selected_folder`（复制→选目标目录→粘贴生成 `alpha copy.md`）+ `tree_paste_writes_clipboard_image_with_date_hash_name`（剪贴板图片按 B10 模板落盘） | 通过 |
| H3 主题变量文档 | `docs/主题变量.md` 全量 token 表；`theme_token_documentation_covers_every_token` 双向校验（缺 token / 多余 token 都失败），并用假 token 做正控验证 | 通过 |
| H4 语言包外置 | 既有「导入→写入用户 languages 目录→启动加载」链路补齐目录直放用例 `loads_language_pack_dropped_into_user_directory`（含未覆盖字符串回退英文） | 通过 |
| E10 恢复合并 | 启动恢复时若会话已打开同一文件，把快照未保存内容并入该标签（快照 id 转移给标签），不再另开窗口；用例 `recovery_snapshot_merges_into_open_session_tab` 断言正文/脏标记/recovery_id 转移与磁盘旧内容不变 | 通过 |

## 第十八批补充（H2 策略 / G5 启动计时）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| H2 外部变更策略 | `manual_external_change_policy_keeps_buffer_until_user_reload`：manual 时 `reload_externally_changed_document` 不替换正文，改回 auto 后重载生效 | 通过 |
| H2 删除策略 | `permanent_delete_removes_file_and_directory`：永久删除文件与目录；配置往返断言 `delete_policy = "permanent"` / `external_change_policy = "manual"` 落盘 | 通过 |
| G5 启动计时 | `VELORA_STARTUP_TIMING=1` 分阶段输出；改动前后同机对比（debug 构建，从进程进入到首个窗口）：**1420ms → 657ms**。阶段：app.run 入口 236ms（二进制装载+GPUI 初始化）、preferences 239ms、i18n/主题/设置 246ms、编辑器+快捷键 248ms、首个窗口 657ms；其中菜单构建从启动路径移出（延后一帧）、macOS 150ms 宽限期取消、会话标签仅同步打开活动标签 | 通过（解锁后可复测首帧可见时间） |
| 全量回归 | `cargo test` 880 通过 0 失败 | 通过 |

## 第十九批补充（F3 导出主题 / F4 打印 / F5 PNG 长图）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| F3 导出主题 | `export_theme_preference_round_trips_through_config_file`（`[export] theme` 落盘往返）+ `resolve_export_theme_reads_configured_preference` + 三个渲染用例：暗色偏好渲染暗背景 token、浅色偏好渲染浅背景 token、`current` 保持当前主题输出 | 通过 |
| F4 打印 | `print_temp_paths_are_unique_and_typed`（临时路径唯一且带类型后缀）、`print_open_command_uses_preview_on_macos` / `print_open_command_uses_platform_default_viewer`（平台命令构造）、`open_command_reports_spawn_failure` 与 `open_command_reports_non_zero_exit`（失败可诊断）；`render_pdf_from_print_html_uses_chromium_print_pipeline` 断言打印 HTML 复用 Chromium 打印管线（Chrome 缺失时报同款可行动错误）；菜单结构测试断言「打印…」分发 `PrintDocument` | 通过（真实打印需人工点一次菜单） |
| F5 PNG 长图 | 纯函数与管线用例：`long_image_params_capture_full_page_as_png`（PNG 格式 + 内容尺寸裁剪 + `capture_beyond_viewport`）、`device_metrics_size_rounds_fractional_content_up`（向上取整防末行裁切）、`long_image_html_uses_browser_layout`（走浏览器版式而非打印分页）；端到端 `long_image_height_covers_content_beyond_the_viewport`（200 段 vs 400 段：宽度恒为 2000px、高度超过首屏 4 倍且随段落数近线性增长，证明末段未被裁掉），**正控**：把裁剪高度写死为 1000px 后该用例必失败（实测报「长图高度 2000 未超出首屏」）；编辑器层 `export_png_writes_long_image_without_changing_editor_state`；菜单结构 + zh/en 菜单文案用例随新项更新 | 通过（人工目视：2000×2084 长图文字锐利、底部元素与留白完整；20 万像素级长文 2000×35110 正常输出） |
| 全量回归 | `cargo test` 887 通过 0 失败 1 忽略；`cargo build` 0 警告 | 通过 |
| 说明 | `window_size` 在新版无头模式下不决定页面视口（实测仍为 800px 宽），长图视口改由 CDP `Emulation.setDeviceMetricsOverride` 固定；内容高度取自 DOM（`body` 绘制高度），不用 `LayoutMetrics.css_content_size`（后者对短文档会返回视口高度，导致长图底部多出空白） | 已记录 |

## 第二十批补充（E9 快速切换器 IME）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| E9 输入法接线 | 快捷切换器的单行输入改走编辑器输入处理器（与搜索框共用 `OverlayInputKind` 路由，新增 `QuickOpen` 分支）；用例 `quick_open_accepts_ime_text_for_non_ascii_file_names`：`simulate_input("笔记")` 走 key_char→`replace_text_in_range` 路径，断言查询/字节选区/结果（只剩 `笔记.md`）；同用例补 ASCII 断言（`⌘A` + `alpha` 只插入一次，防止「手动插入 + 输入处理器」双写） | 通过 |
| 组合与编辑 | `quick_open_composition_commit_backspace_and_escape_edit_the_query`：组合期 `marked_range=0..4`（拼音 `biji`）→ 提交 `笔记` 覆盖组合串并刷新结果 → 退格按字素删除（`笔记`→`笔`，选区 3..3）→ escape 关闭面板 | 通过 |
| 正控 | 把输入处理器的 `canvas`+`handle_input` 临时删除后重跑：`quick_open_accepts_ime_text_for_non_ascii_file_names` 必失败（查询为空字符串），确认用例真的在测接线 | 通过 |
| 顺带修复 | escape 之前关不掉快捷切换器与命令面板：全局快捷键 `escape`→`DismissTransientUi` 先于浮层自身的 key_down 被分发并消费，现由 `on_dismiss_transient_ui` 统一调用 `close_quick_open`/`close_command_palette` | 通过（用例内 escape 断言） |
| 全量回归 | `cargo test` 889 通过 0 失败 1 ignored；`cargo build` 0 警告 | 通过 |

## 第二十一批补充（H5 命令注册表统一化）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| H5 注册表 | 新增 `src/commands.rs`：`CommandSpec{id, menu, separator_before, label(i18n), action}` + `commands()/commands_for()`；菜单的 App/File/Export/View/Help 分组与命令面板条目都由注册表生成（面板此前是 18 条手写子集） | 通过 |
| 结构一致 | `view_menu_matches_the_command_registry`：逐条对照视图菜单与注册表（文案、动作类型 `partial_eq`、分隔线位置、条目总数），多一条少一条都失败；**正控**：构建器临时跳过 `zoom_reset` 后该用例必失败 | 通过 |
| 无死命令 | `every_registered_command_has_a_handler`：对注册表每条命令断言 `cx.is_action_available`（菜单启用判定），把守「有菜单项/面板条目但没有处理者」。此用例在修复前实测抓出 5 条：复制为 HTML、命令面板、放大、缩小、重置缩放——前三条此前在命令面板里点了没反应（面板直接调 `dispatch_menu_action`，绕过视图级处理者） | 通过 |
| 死命令修复 | 命令面板改为 `window.dispatch_action`（与菜单项/快捷键同链路）；补齐 `OpenCommandPalette` 派发分支与 5 条全局监听（复制为 HTML、命令面板、缩放三件套）；缩放逻辑抽成 `Editor::zoom_by/zoom_reset` 供菜单与快捷键共用；视图菜单新增 放大/缩小/重置缩放（此前仅面板有且无效） | 通过 |
| i18n | 新增 `menu_open_command_palette`（视图菜单与面板共用）与 `command_zoom_in/out/reset`（原面板为写死英文 `⌘+ Zoom In`） | 通过 |
| 全量回归 | `cargo test` 894 通过 0 失败 1 ignored；`cargo build` 0 警告 | 通过 |

## 第二十二批补充（G8 渐进建块）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| G8 分块导入 | `from_markdown` 只同步建首块（`FIRST_CHUNK_ROOTS = 2000` 个根块），其余由 `cx.spawn` 任务按 `STEADY_CHUNK_ROOTS = 250` 续建；追加走新增的 `DocumentTree::append_roots` 增量快照（不重跑整树 DFS）。列表是唯一可能无限长的构造（无空行的 10 MiB 夹具整体被解析成一个大列表），`collect_list_blocks` 因此接受剩余根块预算并在顶层条目边界停下 | 通过 |
| 等价性（核心） | `progressive_import_matches_single_pass_import`：以预算 1/2/3/5/8 分块导入同一份含 frontmatter、懒惰续行、未闭合反引号、空行串、列表紧跟段落、表格、围栏、公式、结尾无换行的文档，与 `usize::MAX`（整篇一次建完）逐字节比较 `markdown_text`、可见块数、`raw_source_text`；**未建完期间**的文本重新导入后也必须与整篇导入一致（证明中途保存不丢内容） | 通过 |
| 续建与保存 | `streamed_document_saves_complete_text_to_disk`：预算 2 打开的文档在窗口内续建完成后，输入字符 + ⌘S 落盘内容 == `markdown_text`（含尾段）；`structural_edit_flushes_the_pending_import`：结构编辑（`insert_blocks_at`）先补建完剩余块（`pending_tail` 变 None），块数 = 整篇 + 1 且尾段内容仍在 | 通过 |
| 10 MiB 预算 | `large_document_opens_within_budget`（gitignored 夹具）：打开（首块）**1.23s**（此前一次性建块 17.5s）；判据为首块块数有界（≤4000）+ 打开耗时 ×5 仍小于整篇建块耗时 + 打开即存在挂起尾段（**正控**：把首块预算临时改成 `usize::MAX` 后「先只建首块」断言必失败）；续建完成后 159,683 块、文本与整篇导入逐字节一致，总成本 138.9 µs/块（≤ 400 µs 防回归） | 通过（第二十四批把绝对 3s 判据换成形状判据，避免并发跑测误报） |
| 拼接规则 | 未建完的尾段以原文行参与序列化（已建块正常序列化），连接处空行按「上一根块是列表项且尾段首行是列表标记 → 不加空行，否则加一行」计算，与整篇序列化的列表组规则同源；编号列表序号与列表组空行经 `SyncSeeds` / `PendingTail` 跨批延续 | 通过（开发中实撞：预算 5 时连接处多算 1 个空行 → 等价性用例失败，修好后通过） |
| 冲刷点 | 结构变更入口（`with_structure_mutation` / `insert_blocks_at` / `remove_block_by_id_raw`）与文档内搜索（`open_document_find`）先补建完剩余块；`replace_roots` 丢弃挂起尾段（替换整篇文档的语义） | 通过 |
| 已知取舍 | 续建期间滚动只能到已建块末尾；对未建区域做文字编辑会落在已建末尾（续建完成后与整篇导入的文本一致，不丢内容）；导出 / 自动恢复读的是拼接后的完整文本 | 已记录 |
| 全量回归 | `cargo test` 898 通过 0 失败 1 ignored；`cargo build` 0 警告 | 通过 |

## 第二十三批补充（A7 窗口标题同步，应用侧闭环）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| A7 首帧推送 | `from_markdown` 构造时置 `pending_window_title_refresh = true`：新窗口首帧即推送标题（此前只有打开文件 / 编辑 / 保存 / 恢复等事件才推，新建窗口的原生标题一直为空） | 通过 |
| A7 用例 | `window_title_tracks_file_and_edited_state`：打开即 `Velora - <文件名>` → 输入后 `<已编辑标记> Velora - <文件名>` → ⌘S 后回到干净标题；测试平台的窗口补实现 `get_title`（此前 `window_title()` 恒为空串，断言无从下手） | 通过（**正控**：去掉首帧推送后首条断言实测失败 `left: ""`） |
| 剩余人工项 | 锁屏 / 休眠期间「新建窗口何时显示」属系统合成行为（macOS 不向锁定中的会话合成新窗口），需解锁后目视确认一次；应用侧已保证解锁后的第一帧就带上正确标题与编辑标记 | 已记录 |
| 全量回归 | `cargo test` 898 通过 0 失败 1 ignored；`cargo build` 0 警告 | 通过 |

## 第二十四批补充（用户报修：大于 8 KiB 的中文文件显示「无法使用文本编辑器预览该文件」）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| 问题复现 | 用户报修 `docs/plans/2026-09-24-velora-velotype-feasibility.md`（8.4 KiB，UTF-8）打开即显示占位「无法使用文本编辑器预览该文件」 | 已复现 |
| 根因 | `is_likely_text_file` 取前 8192 字节做 `str::from_utf8` 校验：中文按 3 字节/字，8192 字节边界大概率切在字符中间，**截断**导致校验失败 → 整篇判成二进制 → 只显示占位。仓库内 77 个 > 8 KiB 文件里 3 个被误判（含用户报的那个与本次交接文档、`src/components/block/runtime/mod.rs`），真正非法 UTF-8 的为 0 | 已定位 |
| 修复 | 截断错误（`Utf8Error::error_len() == None`，即窗口只切掉最后一个字符）视为文本前缀；含 NUL 或窗口内出现非截断的非法 UTF-8 仍判二进制 | 通过 |
| 用例 | `text_sniffing_accepts_a_prefix_cut_mid_character`：构造 8191 字节 + 三字节汉字（并在用例内断言前提「8192 字节确实切在字符中间」），另带两个反例（NUL 文件、窗口内非法 UTF-8 仍判二进制）；`opening_a_large_cjk_markdown_file_shows_the_editor`：经工作区打开该文件后不出现占位、正文含尾段 | 通过（**正控**：还原修复后两条同时失败，端到端用例实测报「长中文 md 文件应正常打开，不该显示『无法预览』占位」） |
| 顺带修正 | `large_document_opens_within_budget` 的 3s 判据在整套并发跑测时受 CPU 争用影响（同一次实测 3.96s 而单独跑为 1.23s），改为形状判据：首块块数有界（≤4000）+ 打开耗时乘以 5 仍小于整篇建块耗时；单机实测值改用 eprintln 记录（打开 1.39s / 整篇 25.1s / 157.5 µs·块） | 通过 |
| 全量回归 | `cargo test` 900 通过 0 失败 1 ignored；`cargo build` 0 警告 | 通过 |

## 第二十五批补充（用户报修：窗口位置与大小「根本没有实现」，且缺少对应设置）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| 问题复现 | 用户报修：调整窗口位置/大小后关闭，下一次 `cargo run` 又回到调整前；且设置里没有窗口位置/大小相关项 | 已复现 |
| 根因一（红灯关闭） | macOS 红灯关闭走平台关闭回调：`on_window_should_close` 判定「干净文档」后直接 `return true`，平台随即销毁窗口，**这条路径上没有任何落盘**；只有应用内关闭（⌘W/菜单）才写过 frame | 已定位 |
| 根因二（⌘Q 静默失效） | ⌘Q 由窗口内的处理者（`Editor::on_quit_application`）触发，此时该窗口正被借用；`request_quit_application` 内对同一窗口的 `window.update` 必然返回 Err（gpui `update_window_id` 对已在更新栈上的窗口取不到槽位），于是整段退出逻辑提前返回——既不退出、也不落盘 | 已定位 |
| 根因三（丢弃关闭） | 「放弃并关闭」路径直接 `window.remove_window()`，绕过了落盘 | 已定位 |
| 修复 | ① 窗口移除统一走 `Editor::close_editor_window`（先落盘再移除）；② `on_window_should_close` 在允许关闭前落盘（覆盖平台红灯关闭）；③ `request_quit_application` 改为 `cx.defer` 延后到本轮更新结束后执行（窗口已放回，逐窗口询问与落盘都正常）；④ 删除行「放弃并关闭」改走统一出口 | 通过 |
| 守卫用例 | `quitting_the_app_remembers_each_window_frame`：哨兵 frame 1111×777 → 建窗口 → 缩放到 1200×820 → 窗口内派发 ⌘Q → 断言落盘为 1200×820；`platform_close_remembers_the_window_frame`：`on_window_should_close` 允许关闭后断言落盘为当前尺寸；`every_editor_window_removal_remembers_the_frame`：源码扫描（`close.rs` 恰好 1 处 `window.remove_window()`、其余窗口模块 0 处），挡住「新写一条关闭路径忘了记 frame」 | 通过（**正控**：去掉 defer 后 ⌘Q 用例必失败；去掉 should_close 落盘后两条用例同时失败；去掉退出前落盘后用例仍通过，据此删掉了冗余的整体扫描并保留单点落盘） |
| 新增设置（用户要的「窗口位置与大小设置」） | 偏好设置「窗口」页新增 **打开位置**（`记住上次位置` 默认 / `居中打开`）与 **记住窗口位置与大小** 开关（从文件页移入），写入 config.toml `[window] open_position`；`居中打开` 时忽略记住的 frame，按「默认窗口尺寸」在主屏居中——默认尺寸设置由此真正生效 | 通过 |
| 设置用例 | `window_open_position_setting_controls_how_windows_open`：哨兵 frame (40,60,1000,700) + `记住上次位置` → 开窗即恢复该 frame；切 `居中打开` → 断言开窗为测试主屏（1920×1080）居中、尺寸=默认 1080×720（即 (420,180,1080,720)）；**正控**：临时忽略打开位置设置后必失败 | 通过 |
| config 往返 | `saves_and_reads_preferences` 覆盖 `open_position = "center"` 落盘与回读；`legacy_preferences_are_normalized_with_language` 断言老配置缺该键时按 `remember`（记住上次位置）处理并补写 | 通过 |
| 设置页用例 | `window_page_exposes_zoom_and_default_size_controls` 扩展为覆盖窗口页四个控件（缩放/默认尺寸/打开位置/记住开关）与未保存态 | 通过 |
| 测试隔离（顺带发现） | 窗口 frame 用例共用进程级配置目录时会互相污染：整套并发跑测时 `window_open_position_setting_controls_how_windows_open` 实测读到别的用例（关闭窗口时同样落盘 frame）写下的 (0,0,1920,1080)。新增 `crate::config::override_test_config_root`（线程局部覆盖 + Drop 守卫），三条 frame 用例各自独占临时目录 | 通过（修前整套必失败、修后整套通过） |
| 全量回归 | `cargo test` 904 通过 0 失败 1 ignored；`cargo build` 0 警告 | 通过 |

## 第二十六批补充（用户报修：搜索结果里文件名本身点不了，只有下方一条空行能点）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| 问题复现 | 用户截图：工作区搜索（「所有文件」范围）列出 `assets/icon/velotype-*.png` 等，每行只有文件名与命中数；能点的只有文件名下方一条看不见的细条 | 已复现 |
| 根因 | 文件名命中（`WorkspaceSearchHit.line == None`、无预览）会渲染成一条 `py(4)` 且**没有任何子元素**的空行——它就是那条可点的细条；而真正显示文件名的「文件头」行完全没有点击处理。于是看起来只剩下面那小条能点 | 已定位 |
| 修复 | ① 文件头整行可点：加 `cursor_pointer`/悬停底色/选中底色与点击处理，点击打开该组第一条命中（文件名命中→打开文件；内容命中→跳到该处匹配）；② 文件名命中不再渲染独立空行，由文件头代表（`line.is_none()` 直接跳过） | 通过 |
| 用例 | `search_result_file_header_opens_the_file_and_has_no_empty_row`：造 png（文件名命中）+ md（文件名命中 + 内容命中），断言结果顺序与类型 → 断言文件名命中索引**没有**命中行元素而内容命中索引有 → 断言文件头点击区高度 > 16pt（原空行只有 8pt）→ 点文件头实际打开 png（不可预览占位路径）与 md（`file_path`） | 通过（**正控**：恢复空行后「不应再有空行」断言必失败；去掉文件头点击处理后「点文件名应打开该文件」断言必失败，实测 left: None） |
| 顺带 | 命中行与文件头都补 `debug_selector`（`workspace-search-hit-{i}` / `workspace-search-file-{i}`），后续 UI 断言可直接定位 | 通过 |
| 全量回归 | `cargo test` 905 通过 0 失败 1 ignored；`cargo build` 0 警告 | 通过 |

## 第二十七批补充（用户报修：点击搜索结果后侧栏闪一下——先空白再恢复）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| 问题复现 | 用户报修：点击搜索结果（文件名现在可点了，见第二十六批）后，搜索面板先变空白，随后结果又重新出现 | 已复现（状态层） |
| 根因 | `schedule_workspace_search` 一进入就把 `search_results.clear()` 掉再置 `search_pending`，而去抖窗口是 120ms，期间面板渲染 `…` 占位（看起来就是一片空白）；文件树刷新（watcher）等任何重新调度都会走这条路，点击打开文件后正好容易撞上 | 已定位 |
| 修复 | ① 重新搜索期间**保留上一次结果**，新结果落地后整体替换（仅当没有任何结果可显示时才用 `…` 占位）；② 查询被清空时立即清空结果，避免留下过期结果 | 通过 |
| 用例 | `re_search_keeps_previous_results_visible`：建工作区搜索（3 条命中）→ 调 `refresh_workspace_tree` 模拟 watcher 触发的重新调度 → 断言 pending 期间结果仍是 3 条、面板仍渲染出文件头与命中行（`debug_bounds`）、120ms 后结果一致 → 再清空查询，断言结果立即为空且不 pending | 通过（**正控**：恢复「进函数就清空」后状态断言实测失败 left:0/right:3；把 pending 判断改回「pending 就显示 …」后面板断言实测失败） |
| 顺带修门禁空心 | 排查正控时发现 `debug_bounds` 在 gpui 里**跨帧累积**（`Frame::clear()` 没清 `debug_bounds`），于是「元素还在不在」这类断言会读到早已消失元素的旧边界——正控一度因此假绿。本地补丁 `vendor/gpui/src/window.rs` 在逐帧 `clear()` 里补 `debug_bounds.clear()`，此后查询的是当前帧；原有 4 处 debug_bounds 用例（TOC 条目、折叠 chevron、代码复制按钮、长块护栏）全部复跑通过 | 通过 |
| 全量回归 | `cargo test` 906 通过 0 失败 1 ignored | 通过 |
| 补丁打包口径（事后修正） | 该批提交时只跑了 `cargo test`，漏跑 `cargo build`：`debug_bounds` 字段带 `#[cfg(any(test, feature = "test-support"))]`，测试构建（dev-dependency 打开了 test-support）能编过，而 `cargo run` / `cargo build` 的生产构建报 E0609。修复为 `clear()` 里补同样的 cfg 门（提交 `fix(build)`），并把「提交前 `cargo build` + `cargo test` 两条都跑」写进交接文档约定 | 已修正 |

## 第二十八批补充（用户报修：点击含行内代码的行会跳字号；行内代码应跟「代码字号」设置）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| 问题复现 | 用户报修两条：① 未聚焦时行内代码显示为 0.85× 正文，点击进去编辑那一刻字号跳回正文；② 既然已有「代码块字体大小」设置，行内代码应直接用该设置 | 已复现 |
| 根因 | 显示态与编辑态走两套机制：含行内代码的块走「混合分段」flex 路径（每段可有独立字号），而聚焦后的可编辑文本是一个 `TextRun` 列表 + 整行单一字号——gpui 的 `TextRun` 没有逐段字号，两套字号不可能一致，所以 0.85× 只能停在显示态、一进编辑就跳。也正因如此，行内代码当年被迫留在混合分段路径里（`has_mixed_inline_visuals` 带 `style.code`） | 已定位 |
| 本地补丁（逐段字号） | `gpui::TextRun` 新增 `font_size: Option<Pixels>`（`None` 沿用元素字号 ⇒ 既有渲染逐字节不变）：`FontRun` 带字号并进缓存键（`Pixels` 只有 `PartialEq`，按 `to_bits` 手写 `Eq`/`Hash`，与 `RenderGlyphParams` 同法）；mac `layout_line` 按段 `clone_with_font_size` 并按段字号算 ascent/descent，CoreText 切出的字形段按段首字形 UTF16 下标回查字号写进 `ShapedRun`；`text_system/line.rs` 的字形/emoji 绘制与背景、下划线基准都取该段字号；windows DirectWrite 同步（按段 `SetFontSize`、`glyphrun.fontEmSize` 回写）；linux 仍是 cosmic-text 单字号（代码内已注明）；测试占位平台 `NoopTextSystem` 按字号缩放步进并切段，让断言跑得动 | 通过 |
| 应用侧修复 | ① 行内代码退出混合分段路径（`src/components/markdown/inline.rs`），聚焦前后同一个文本元素、同一套字号；② 编辑态 `build_text_runs` 给代码段带 `font_size = 代码字号`（`src/components/block/element.rs`）；③ 混合块（数学/上下标/行内图片同块）里代码段的显示字号也取「代码字号」（`inline_display_font_size` 新增入参）；④ 表格列宽按代码字号量宽（`src/components/markdown/table.rs`），避免含行内代码的列按正文字号估宽 | 通过 |
| 用例 | ① `mac_platform_text_system_applies_per_run_font_size`：真实 CoreText（走 test-support 门控的 `gpui::shape_line_with_platform_text_system` 入口）——同一 Menlo，16px 正文段与 13px 代码段，代码段步进 < 正文段步进 90%，且 `ShapedRun` 带 `Some(13)` 供绘制取用；② `inline_code_runs_use_the_code_font_size`：编辑态 `build_text_runs` 的代码段带 `Some(13)`、两侧正文段不带覆盖，整形结果按字号切段且代码段更窄；③ `inline_code_uses_the_code_font_size_while_scripts_shrink`：混合块显示字号（代码=代码字号、上标=72%、正文=正文）；④ `vendored_text_system_keeps_the_per_run_font_size_patch`：源码守卫，守住 mac/windows 的逐段字号补丁不被 vendored 副本更新悄悄覆盖 | 通过 |
| 正控 | ① 去掉 `TextRun → FontRun` 的字号传递（`vendor/gpui/src/text_system.rs`）⇒ 用例②「整形结果应保留代码段字号」失败；② mac 排版层改回整行字号（`clone_with_font_size(font_size)`）⇒ 用例①失败；③ mac 不回写 `ShapedRun` 字号（`font_size: None`）⇒ 用例①失败；④ 显示态 helper 把代码段退回正文字号 ⇒ 用例③失败（实测 left: 16.0 / right: 13.0）；全部复原后复跑通过 | 通过 |
| 门禁环境说明 | `cargo test -p gpui` 与在 `vendor/gpui` 内跑 gpui 自带测试都跑不通（gpui 非 workspace 成员；其独立测试构建还受本地 `ClipboardItem.html` 补丁影响），所以真实平台断言放在 velora 用例里，通过 test-support 门控入口调用真实 mac 文本系统 | 已记录 |
| 全量回归 | `cargo build` 0 警告；`cargo test` 911 通过 0 失败 1 ignored | 通过 |
| 待人工目视 | 锁屏解除后确认：含行内代码段落点击前后字号不再变化；把「代码字号」调到明显小于/接近正文字号（如 12 与 16）时行内代码随之变化；围栏代码块与导出 HTML 不受影响 | 待复核 |

## 第二十九批补充（用户报修：行内代码内部不该再解析任何 Markdown）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| 问题复现 | 用户截图：roadmap 表格单元格里 `` `![alt](path){width=NN%}` `` 的**代码内容**被渲染成「无法加载图片 alt」占位框 | 已复现 |
| 根因 | 解析层是对的（代码段内容原样保留、序列化可往返），坏在**渲染期二次扫描**：表格单元格按 `serialize_markdown()` 调 `parse_table_cell_inline_images` 提升行内图片，段落「混合分段」路径也对每段文本重扫 `![`，两者都不认得反引号，于是把代码里的图片语法当真图片处理 | 已定位 |
| 修复 | ① `parse_table_cell_inline_images` 先跳过完整反引号代码段（`inline_code_span_end` 按 CommonMark「等长反引号串成对」扫描，未闭合的反引号按字面文本继续）；② 段落路径的判定抽成 `promotes_inline_images(text, style)`，`style.code` 一律不提升；③ `has_mixed_inline_visuals` 的 `![` 判据同样排除代码段（纯代码里的 `![` 不再把整块拖进混合分段路径） | 通过 |
| 用例 | ① `table_cell_inline_images_ignore_inline_code_content`：整格只有代码里的图片语法 ⇒ 单条文本段；代码段外的 `![b](y.png)` ⇒ 仍提升为图片段；② `inline_code_does_not_force_the_mixed_visual_path` 扩到代码段内部：`![alt](p.png) [a](b) $c$ ^d^` 全部原样保留（visible_text 与 serialize_markdown 逐字符断言）；③ `inline_code_content_never_becomes_an_image_widget`：`promotes_inline_images` 对代码段为假、对正文段为真 | 通过 |
| 正控 | 去掉 image.rs 的代码段跳过 ⇒ 用例①失败；把 `promotes_inline_images` 改回只看 `![` ⇒ 用例③失败；全部复原后复跑通过 | 通过 |
| 全量回归 | `cargo build` 0 警告；`cargo test` 913 通过 0 失败 1 ignored | 通过 |

## 第三十批补充（用户报修：块级公式字号太大；图片占位框冲出容器）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| 问题复现 | ① `$$ \int_0^1 x^2 dx = \frac{1}{3} $$` 渲染出的字号明显大于正文与行内公式；② callout 列表项里 `![image](missing.png)` 的「无法加载图片」占位框被拉成横穿整屏的空心长条，冲出 callout 右边界 | 均已复现 |
| 根因（公式） | `DISPLAY_MATH_SCALE = 1.25` 而 `INLINE_MATH_SCALE = 1.12`：块级公式额外放大 12%。KaTeX/Typora 的 display 模式只改变极限位置，不放大字号 | 已定位 |
| 修复（公式） | `DISPLAY_MATH_SCALE = INLINE_MATH_SCALE`（1.12），行内与块级同字号 | 通过 |
| 根因（图片） | 列表项图片宽度按**视口**估算成 definite 像素（`effective_list_item_image_width`：centered 列宽 − padding − 缩进 − 记号，callout 内缩只在 `callout_depth > 0` 时扣），量不到真实容器；占位框再按 `.w(该宽度)` 画死，于是冲出 callout | 已定位 |
| 修复（图片） | ① 三处列表项图片路径（无序/有序/任务）的宽度上限改为 `relative(1.0)`——按所在列封顶，交 taffy 按实际容器算；② 失败/加载占位框由 `.w(width)` 改为 `.min_w(0)+.max_w(width)`，贴着文字收紧，不再拉成空心长条；③ 删掉不再使用的 `effective_list_item_image_width` | 通过 |
| 用例 | ① `display_math_font_size_matches_inline_math`：块级与行内字号相等（原断言 25.0 已按新口径改）；② `broken_image_placeholder_stays_compact_inside_the_column`：1400pt 宽窗口渲染「callout + 列表 + 读不出来的图片」，用 `debug_bounds("image-placeholder")` 断言占位框宽度 ≤400pt 且右边不超出视口 | 通过 |
| 正控 | 把占位框改回 `.w(width)` 并按旧的视口估算 definite 宽度 ⇒ 实测宽度 900pt、原点左移到 784，用例失败（阈值 400）；恢复后 233pt 通过 | 通过 |
| 全量回归 | `cargo build` 0 警告；`cargo test` 914 通过 0 失败 1 ignored | 通过 |
| 待人工目视 | 解锁后确认：块级公式与行内公式观感同字号、不再压过正文；callout 列表里的坏图/远端加载中的占位框是小号贴字框，真实宽图按列宽缩放不冲出边界 | 待复核 |

## 第三十一批补充（用户复核：块级公式「大小基本没变」——真正的根因是 SVG 单位）

| 项目 | 验证方式 | 结论 |
|------|----------|------|
| 用户反馈 | 上一批把 `DISPLAY_MATH_SCALE` 从 1.25 调到 1.12 后，用户实测「还是这么大」 | 成立：12% 的差异肉眼几乎看不出，说明真正放大不在字号系数 |
| 真正根因 | ratex 输出的 SVG 根标签是 `width="114.75pt" height="56.91pt"`（**pt**），而 gpui 的 SVG 通道（usvg）与浏览器都按 96/72 把 pt 换成 px ⇒ 所有公式比请求字号**大 1/3**。行内公式被 `.max_h(math_size × 1.65)` 的上限顺手压住，所以看起来正常；块级公式没有这个上限，直接按内在尺寸画，于是明显偏大 | 已定位（实测 `\frac{1}{3}` @16px → 43.32 单位，积分式 @17.92px → 56.91 单位） |
| 修复 | ① `normalize_svg_size_units_to_px`：只改写根标签的尺寸单位为 `px`（坐标空间本身就是「1 单位 = 该字号下 1px」），行内/块级/导出 HTML 三条路径共用同一份 SVG，字号设置由此真正等于所见字号；② `latex_cache_key` 加缓存格式版本盐（`ratex-svg-px-v2`），避免旧的 pt 缓存文件被继续复用（行内公式字号未变，键不变 ⇒ 不加盐就仍然偏大） | 通过 |
| 用例 | `svg_root_uses_pixel_size_units`：根标签必须含 `px"` 且不残留 `pt"`；`\frac{1}{3}` @16px 高度落在 1.5~3em（含留白）；字号翻倍高度翻倍（线性） | 通过 |
| 正控 | 去掉单位归一化（直接返回 ratex 原文）⇒ 「根标签不应残留 pt 单位」断言失败；恢复后通过 | 通过 |
| 净效果 | 块级公式实际尺寸 = 旧的 1.12/(1.25×1.333) ≈ **0.67 倍**（小 33%）；行内公式同样按字号收敛（不再靠 max_h 压），三者同一字号法则 | 通过 |
| 全量回归 | `cargo build` 0 警告；`cargo test` 915 通过 0 失败 1 ignored | 通过 |
| 待人工目视 | 重启 `cargo run` 后确认：块级积分式明显小于改前、与正文比例接近 Typora/KaTeX；行内公式与行高协调；导出 HTML 里的公式同尺寸 | 待复核 |

## 已知事项

- 全量测试唯一失败项 `autosave_does_not_overwrite_external_file_changes`
  为基线（main 9254d49）即存在的偶发用例，与本批改动无关。
- 2026-09-27 23:5x 起机器自动锁屏，后续原生截图需解锁后补拍；
  已改为 gpui 测试验证渲染路径。
- 执行中发现 4 个新增项：D9 / E9 / E10 / G8 已完成（第十四 / 二十 / 二十二批）；A7 应用侧已闭环（第二十三批），仅剩「锁屏期间新建窗口的显示时序」这一平台行为需解锁后人工确认一次。
- 第二十五批只覆盖逻辑与配置层（gpui 用例 + config 往返）；macOS 上「拖动窗口 → 红灯关闭 → 重开」的观感需解锁后人工复核一次。
- 窗口 frame 的落盘时机（关窗/退出）不覆盖强制杀进程（`kill -9`）——强制退出前最后的位置调整不会保存。
