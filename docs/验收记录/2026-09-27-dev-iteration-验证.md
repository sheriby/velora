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

## 已知事项

- 全量测试唯一失败项 `autosave_does_not_overwrite_external_file_changes`
  为基线（main 9254d49）即存在的偶发用例，与本批改动无关。
- 2026-09-27 23:5x 起机器自动锁屏，后续原生截图需解锁后补拍；
  已改为 gpui 测试验证渲染路径。
- 执行中发现 4 个新增项（D9/E9/E10/A7）已录入 roadmap 待办。
