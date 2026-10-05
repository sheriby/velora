use super::*;

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.modal_key_interceptor.is_none() {
            // C13：模态的 Enter/Esc 在 keymap 绑定解析之前拦截（绑定先于
            // 元素监听，回车会被焦点块的 Newline 绑定消费）。拦截器全局
            // 注册、按窗口过滤；stop_propagation 阻断 action 派发。
            let editor = cx.entity().downgrade();
            let window_handle = window.window_handle();
            self.modal_key_interceptor = Some(cx.intercept_keystrokes(
                move |event: &gpui::KeystrokeEvent, window, cx| {
                    if window.window_handle() != window_handle {
                        return;
                    }
                    let keystroke = &event.keystroke;
                    // 三个浮层（模态/文件历史/[[ 补全）各自过滤按键；都未打开
                    // 时这里只有三次布尔检查，打字热路径无感。
                    let _ = editor.update(cx, |editor, cx| {
                        let consumed = editor.modal_handle_keystroke(keystroke, window, cx);
                        let consumed = consumed || editor.file_history_key_down(keystroke, cx);
                        if !consumed {
                            editor.wikilink_completion_key_down(keystroke, cx);
                        }
                    });
                },
            ));
        }

        self.window_handle = Some(window.window_handle());
        if self.system_appearance_subscription.is_none() {
            self.system_appearance_subscription =
                Some(cx.observe_window_appearance(window, |_editor, window, cx| {
                    let appearance = window.appearance();
                    cx.update_global::<ThemeManager, _>(|manager, _cx| {
                        manager.set_system_appearance(appearance)
                    });
                    cx.refresh_windows();
                }));
        }
        self.install_close_guard(cx, window);
        self.apply_pending_focus(window, cx);
        self.apply_pending_scroll_into_view(window, cx);
        self.apply_pending_workspace_search_focus(window, cx);
        self.refresh_selection_snapshot_if_changed(cx);
        self.sync_pending_save(window, cx);
        self.sync_pending_save_as(window, cx);
        self.sync_pending_workspace_tab_activation(window, cx);
        self.sync_window_edited_state(window);

        let viewport_bounds = self.scroll_handle.bounds();
        let viewport_size = viewport_bounds.size;
        self.sync_scroll_viewport(viewport_size, cx);
        self.sync_outline_follow_scroll(
            self.scroll_handle.bounds().top(),
            cx,
        );

        let mut theme = cx.global::<ThemeManager>().current_arc().as_ref().clone();
        let fonts = crate::config::EditorSettings::fonts(cx);
        let writing_width = crate::config::EditorSettings::writing_width(cx);
        // 字号设置 + 界面缩放（⌘+/⌘-/⌘0）由这一个派生负责，文档块共用。
        crate::config::EditorSettings::apply_scaled_typography(cx, &mut theme);
        let strings = cx.global::<I18nManager>().strings_arc();
        self.sync_window_title(window, &strings);

        let d = &theme.dimensions;
        // P4b：键命中时整帧复用行结构计划；未命中（行元数据/折叠/大纲/模式
        // 变化后的第一帧）才做一次全文档扫描。键用行元数据的版本而不是文档修订：
        // 打字只改块内文字时行元数据不动，计划就该照用。
        let rendered_mode = self.view_mode == crate::editor::ViewMode::Rendered;
        let plan_key = (
            self.document.row_meta_version(),
            self.fold_state_version,
            self.toc_state_version,
            rendered_mode,
            d.block_gap,
            self.document.visible_blocks().len(),
        );
        let cached_plan = self
            .rendered_row_plan
            .clone()
            .filter(|plan| {
                plan.row_meta_version == plan_key.0
                    && plan.fold_version == plan_key.1
                    && plan.toc_version == plan_key.2
                    && plan.rendered_mode == plan_key.3
                    && plan.block_gap == plan_key.4
                    && plan.visible_len == plan_key.5
            });
        let plan_rebuilt = cached_plan.is_none();
        let rendered_row_plan = match cached_plan {
            Some(plan) => plan,
            None => {
                self.row_plan_rebuilds.set(self.row_plan_rebuilds.get() + 1);
                let plan_started = std::time::Instant::now();
                let kept = self.apply_heading_fold_filter(cx);
                let plan = std::sync::Arc::new(self.build_rendered_row_plan(
                    &kept,
                    plan_key.0,
                    plan_key.1,
                    plan_key.2,
                    rendered_mode,
                    plan_key.4,
                    plan_key.5,
                    cx,
                ));
                self.rendered_row_plan = Some(plan.clone());
                self.row_plan_nanos.set(
                    self.row_plan_nanos.get()
                        + plan_started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                );
                plan
            }
        };
        let rows = &rendered_row_plan.rows;
        // The focused row is always kept mounted so its caret is not blurred; a
        // table cell maps to its containing table block's row. 跳转滚动进行中
        // （pending_scroll_active_block_into_view）时焦点通常已交还查询框，
        // 此刻必须把活动块（滚动目标）也钉在窗口里：否则目标滑出绘制窗口、
        // 边界被丢弃，精确居中失去坐标，估算爬行与之互相拉锯（用户报修：
        // 向上跳回开头的命中永远停在半路）。
        let focused_visible_index = self
            .focused_edit_target_entity_id(window, cx)
            .and_then(|id| {
                self.document.visible_index_for_entity_id(id).or_else(|| {
                    self.table_cell_binding(id).and_then(|binding| {
                        self.document
                            .visible_index_for_entity_id(binding.table_block.entity_id())
                    })
                })
            })
            .or_else(|| {
                if !self.pending_scroll_active_block_into_view {
                    return None;
                }
                self.active_entity_id
                    .and_then(|id| self.document.visible_index_for_entity_id(id))
            });
        let focus_mode_active = self.focus_mode
            && self.view_mode == crate::editor::ViewMode::Rendered
            && !self.code_tab_active()
            && self.cross_block_selection.is_none();
        let editor = cx.entity().downgrade();
        let has_menus = cx
            .get_menus()
            .map(|menus| !menus.is_empty())
            .unwrap_or(false);
        let titlebar_height = custom_titlebar_height(window, d);
        let menu_bar_height =
            in_window_menu_bar_height_for_target_os(std::env::consts::OS, has_menus, d);
        let scroll_trigger_padding = (d.block_min_height * 0.75).max(16.0);
        let max_scroll_y = f32::from(self.scroll_handle.max_offset().height.max(px(0.0)));
        let viewport_height = f32::from(viewport_bounds.size.height.max(px(1.0)));
        // Extra room below the last block so the lowest line can be scrolled up
        // to the viewport center instead of being pinned to the bottom edge.
        let scroll_beyond_bottom = viewport_height * 0.5;
        let viewport_width = f32::from(viewport_bounds.size.width.max(px(1.0)));
        let has_overflow = max_scroll_y > 0.5;

        // 行号视图（代码文件 / 切到源码的文档）：列宽吃满内容盒，不再居中留白。
        // 行号左边的空 = 滚动区左 padding（减半后 12）+ 块壳自身 block_padding_x
        // （12），合计 24——原来是 24+12+12=48，用户报修太空、整小一半。
        let source_view =
            self.code_document || self.view_mode == crate::editor::ViewMode::Source;
        let centered_width = if source_view {
            (viewport_width - d.editor_padding * 1.5).max(1.0)
        } else {
            Self::centered_column_width(viewport_width, &theme.dimensions)
                .min(writing_width.max_width(theme.dimensions.writing_max_width))
        };
        let current_scroll_y = (-f32::from(self.scroll_handle.offset().y)).clamp(0.0, max_scroll_y);
        let scrollbar_geometry =
            Self::scrollbar_geometry(viewport_height, max_scroll_y, current_scroll_y);
        let track_height = scrollbar_geometry.track_height;
        let thumb_height = scrollbar_geometry.thumb_height;
        let thumb_top = scrollbar_geometry.thumb_top;

        let show_custom_scrollbar = has_overflow
            && (self.scrollbar_drag.is_some()
                || self.scrollbar_hovered
                || Instant::now() <= self.scrollbar_visible_until);

        // Spacing metadata is read on demand instead of pre-collected into a
        // Vec<RenderedRowSpacingInfo> sized to all visible blocks. For long
        // documents this skips a ~tens-of-KB allocation per frame; per-block
        // entity.read_with is a cheap immutable lock + 7-field struct copy.
        // P7：行元数据（起始下标/行距/行首 id/footprint）全部来自计划，
        // 未变更帧零重算。
        let row_starts = &rendered_row_plan.visible_starts;
        let row_top_gaps = &rendered_row_plan.gaps;
        let row_first_ids = &rendered_row_plan.first_ids;
        // The focused row is always kept mounted so its caret is not blurred; a
        // table cell maps to its containing table block's row.
        let focus_row = focused_visible_index.map(|visible_index| {
            row_starts
                .partition_point(|&start| start <= visible_index)
                .saturating_sub(1)
        });

        // A row's first block keys its cached footprint.

        // On a structural edit the row indices no longer match last frame, so the
        // cache refresh below is skipped; its block-keyed entries still hold.
        // P4b：只有计划重建的帧才需要比较（其余帧 id 序列必然一致）。
        let structural_change = plan_rebuilt
            && (rows.len() != self.prev_visible_block_ids.len()
                || rows
                    .iter()
                    .zip(&self.prev_visible_block_ids)
                    .any(|(row, prev)| row.first_id != *prev));
        if structural_change {
            self.prev_visible_block_ids = rows.iter().map(|row| row.first_id).collect();
        }

        // A footprint only holds for the column it was measured at. The first
        // frame has no scroll bounds yet, so the column collapses to its 1px
        // floor and every block wraps a character per line; keeping those
        // measurements would leave the document permanently mis-sized.
        let width_changed = self.row_stride_width != Some(centered_width);
        if width_changed {
            self.row_stride_cache.clear();
            self.row_stride_width = Some(centered_width);
        }

        // The scroll container records every mounted child's layout bounds, so
        // adjacent tops differ by exactly one row's footprint whatever the row
        // holds. Caching those differences, not raw positions, keeps the window
        // stable while scrolling.
        if !structural_change && !width_changed {
            if let Some(prev) = self
                .prev_mounted_run
                .filter(|prev| self.mounted_run_is_addressable(*prev))
            {
                let prev_end = prev.row_end.min(row_first_ids.len());
                for row in prev.row_start..prev_end.saturating_sub(1) {
                    let child = prev.child_base + row - prev.row_start;
                    if let (Some(bounds), Some(next_bounds)) = (
                        self.scroll_handle.bounds_for_item(child),
                        self.scroll_handle.bounds_for_item(child + 1),
                    ) {
                        let stride = f32::from(next_bounds.top() - bounds.top());
                        if stride > 0.0 && stride.is_finite() {
                            self.row_stride_cache.insert(row_first_ids[row], stride);
                            if let Some(slot) = rendered_row_plan.strides.borrow_mut().get_mut(row)
                            {
                                *slot = stride;
                            }
                        }
                    }
                }
            }
        }

        // Unmeasured rows use the minimum block height: a lower bound, so the
        // window over-mounts rather than ever landing on a spacer.
        let estimate = d.block_min_height.max(1.0);
        let strides = rendered_row_plan.strides.borrow();

        // Bound the cache against block churn, only when it outgrows the live rows.
        if self.row_stride_cache.len() > row_first_ids.len().saturating_mul(2) {
            let live: std::collections::HashSet<EntityId> = row_first_ids.iter().copied().collect();
            self.row_stride_cache.retain(|id, _| live.contains(id));
        }

        let render_window = Self::rendered_window(
            &strides,
            current_scroll_y,
            viewport_height,
            RENDER_OVERDRAW_PX,
            focus_row,
            estimate,
        );

        // 冷启动续挂：行高仍被低估时一帧铺不满视口，立刻排下一帧继续补，
        // 而不是把整屏 spacer 留给读者、等到下一次输入才补上。
        if render_window.needs_fill && self.cold_fill_frames < COLD_FILL_MAX_FRAMES {
            self.cold_fill_frames += 1;
            self.schedule_followup_frame(cx);
        } else {
            self.cold_fill_frames = 0;
        }

        let island = render_window.focus_island;
        let island_before_run = island.is_some_and(|island| island.row < render_window.run_start);
        // A mounted row re-applies its own `mt`, which the preceding stride
        // already covered, so every spacer sheds the gap of the row it precedes.
        let spacer_before = |row: usize, height: f32| -> f32 {
            match row_top_gaps.get(row) {
                Some(gap) => (height - gap).max(0.0),
                None => height,
            }
        };
        let mut block_rows: Vec<AnyElement> =
            Vec::with_capacity(render_window.run_end - render_window.run_start + 4);
        let push_spacer = |rows: &mut Vec<AnyElement>, height: f32| {
            if height > 0.5 {
                rows.push(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .h(px(height))
                        .into_any_element(),
                );
            }
        };

        // P4b：行元素只在挂载时从计划构建（旧实现在扫描期为所有组行
        // 预构建元素再丢弃，是超大文档的每帧浪费）。
        let build_row_element = |row: usize| -> AnyElement {
            match &rows[row].body {
                RenderedRowBody::Ordinary { entity, .. } => {
                    let index = rows[row].visible_start;
                    let element = div()
                        .w(px(centered_width))
                        .max_w(relative(1.0))
                        .flex_shrink_0()
                        .mt(px(row_top_gaps[row]))
                        .opacity(focus_mode_row_opacity(
                            focus_mode_active,
                            focused_visible_index,
                            index,
                            index + 1,
                        ))
                        .child(entity.clone());
                    let element = if rendered_mode {
                        let row_editor = editor.clone();
                        let entity_id = entity.entity_id();
                        element.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                            let _ = row_editor.update(cx, |editor, cx| {
                                editor
                                    .on_block_context_menu_mouse_down(entity_id, event, window, cx);
                            });
                        })
                    } else {
                        element
                    };
                    element.into_any_element()
                }
                RenderedRowBody::Group {
                    callout_variant,
                    members,
                } => {
                    let index = rows[row].visible_start;
                    let group_end = index + members.len();
                    if let Some(variant) = callout_variant {
                        let mut group_children: Vec<AnyElement> = Vec::new();
                        let mut member_index = 0usize;
                        let mut previous_callout_row: Option<RenderedRowSpacingInfo> = None;
                        while member_index < members.len() {
                            let member = &members[member_index];
                            if let Some(footnote_anchor) = member.spacing.footnote_anchor {
                                let mut footnote_children: Vec<AnyElement> = Vec::new();
                                let mut previous_footnote_row: Option<RenderedRowSpacingInfo> =
                                    None;
                                let footnote_start = member_index;
                                while member_index < members.len()
                                    && members[member_index].spacing.footnote_anchor
                                        == Some(footnote_anchor)
                                {
                                    let inner = &members[member_index];
                                    let row = div()
                                        .w_full()
                                        .flex_shrink_0()
                                        .mt(px(footnote_row_top_gap(
                                            previous_footnote_row,
                                            d.block_gap,
                                        )))
                                        .child(inner.entity.clone());
                                    let row = if rendered_mode {
                                        let row_editor = editor.clone();
                                        let entity_id = inner.entity.entity_id();
                                        row.on_mouse_down(
                                            MouseButton::Right,
                                            move |event, window, cx| {
                                                let _ = row_editor.update(cx, |editor, cx| {
                                                    editor.on_block_context_menu_mouse_down(
                                                        entity_id, event, window, cx,
                                                    );
                                                });
                                            },
                                        )
                                    } else {
                                        row
                                    };
                                    footnote_children.push(row.into_any_element());
                                    previous_footnote_row = Some(inner.spacing);
                                    member_index += 1;
                                }

                                group_children.push(
                                    div()
                                        .w_full()
                                        .flex_shrink_0()
                                        .mt(px(callout_row_top_gap(
                                            previous_callout_row,
                                            members[footnote_start].spacing,
                                            d,
                                        )))
                                        .child(footnote_group_shell(
                                            footnote_children,
                                            &theme,
                                            d,
                                        ))
                                        .into_any_element(),
                                );
                                previous_callout_row = Some(members[member_index - 1].spacing);
                                continue;
                            }

                            let row = div()
                                .w_full()
                                .flex_shrink_0()
                                .mt(px(callout_row_top_gap(
                                    previous_callout_row,
                                    member.spacing,
                                    d,
                                )))
                                .child(member.entity.clone());
                            let row = if rendered_mode {
                                let row_editor = editor.clone();
                                let entity_id = member.entity.entity_id();
                                row.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                    let _ = row_editor.update(cx, |editor, cx| {
                                        editor.on_block_context_menu_mouse_down(
                                            entity_id, event, window, cx,
                                        );
                                    });
                                })
                            } else {
                                row
                            };
                            group_children.push(row.into_any_element());
                            previous_callout_row = Some(member.spacing);
                            member_index += 1;
                        }

                        let (accent, background) = callout_colors(*variant, &theme);
                        div()
                            .w(px(centered_width))
                            .max_w(relative(1.0))
                            .flex_shrink_0()
                            .mt(px(row_top_gaps[row]))
                            .flex()
                            .flex_col()
                            .gap(px(0.0))
                            .px(px(d.callout_padding_x))
                            .py(px(d.callout_padding_y))
                            .rounded(px(d.callout_radius))
                            .border_l(px(d.callout_border_width))
                            .border_color(accent)
                            .bg(background)
                            .opacity(focus_mode_row_opacity(
                                focus_mode_active,
                                focused_visible_index,
                                index,
                                group_end,
                            ))
                            .children(group_children)
                            .into_any_element()
                    } else {
                        let mut group_children: Vec<AnyElement> = Vec::new();
                        let mut previous_footnote_row: Option<RenderedRowSpacingInfo> = None;
                        for member in members {
                            let row = div()
                                .w_full()
                                .flex_shrink_0()
                                .mt(px(footnote_row_top_gap(
                                    previous_footnote_row,
                                    d.block_gap,
                                )))
                                .child(member.entity.clone());
                            let row = if rendered_mode {
                                let row_editor = editor.clone();
                                let entity_id = member.entity.entity_id();
                                row.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                    let _ = row_editor.update(cx, |editor, cx| {
                                        editor.on_block_context_menu_mouse_down(
                                            entity_id, event, window, cx,
                                        );
                                    });
                                })
                            } else {
                                row
                            };
                            group_children.push(row.into_any_element());
                            previous_footnote_row = Some(member.spacing);
                        }

                        div()
                            .w(px(centered_width))
                            .max_w(relative(1.0))
                            .flex_shrink_0()
                            .mt(px(row_top_gaps[row]))
                            .opacity(focus_mode_row_opacity(
                                focus_mode_active,
                                focused_visible_index,
                                index,
                                group_end,
                            ))
                            .child(footnote_group_shell(group_children, &theme, d))
                            .into_any_element()
                    }
                }
            }
        };
        let take_row = |rows: &mut Vec<AnyElement>, row: usize| {
            rows.push(build_row_element(row));
        };

        if let Some(island) = island.filter(|_| island_before_run) {
            push_spacer(&mut block_rows, spacer_before(island.row, island.lead_h));
            take_row(&mut block_rows, island.row);
        }
        push_spacer(
            &mut block_rows,
            spacer_before(render_window.run_start, render_window.top_h),
        );
        let run_child_base = block_rows.len();
        for row in render_window.run_start..render_window.run_end {
            take_row(&mut block_rows, row);
        }
        if let Some(island) = island.filter(|_| !island_before_run) {
            push_spacer(&mut block_rows, spacer_before(island.row, island.lead_h));
            take_row(&mut block_rows, island.row);
        }
        push_spacer(&mut block_rows, render_window.bottom_h);
        // Next frame reads the run's footprints back at these child indices, and
        // re-checks `child_count` before trusting them.
        self.prev_mounted_run = Some(MountedRun {
            row_start: render_window.run_start,
            row_end: render_window.run_end,
            child_base: run_child_base,
            child_count: block_rows.len(),
        });

        let scroll_content = div()
            .id("editor-scroll-inner")
            .flex()
            .flex_col()
            .flex_grow()
            .h_full()
            .items_center()
            .bg(theme.colors.editor_background)
            .overflow_y_scroll()
            .scrollbar_width(px(0.0))
            .track_scroll(&self.scroll_handle)
            .on_hover(cx.listener(Self::on_editor_hover))
            .capture_any_mouse_down(cx.listener(Self::on_editor_capture_mouse_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_editor_mouse_down))
            .on_mouse_move(cx.listener(Self::on_editor_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_editor_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_editor_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_editor_scroll_wheel))
            // 行号视图左 padding 减半：行号贴窗口左缘（用户报修：左边空隙大）。
            .pl(px(if source_view {
                d.editor_padding * 0.5
            } else {
                d.editor_padding
            }))
            .pr(px(d.editor_padding))
            .pt(px(if self.code_tab_active() {
                24.0
            } else if self.typewriter_mode && self.view_mode == crate::editor::ViewMode::Rendered {
                (viewport_height * 0.5).max(52.0)
            } else {
                52.0
            }))
            .pb(px(d.editor_padding
                + scroll_trigger_padding
                + scroll_beyond_bottom))
            .children(block_rows);
        let scroll_content = if self.view_mode == crate::editor::ViewMode::Rendered {
            scroll_content.on_mouse_down(
                MouseButton::Right,
                cx.listener(Self::on_editor_context_menu_mouse_down),
            )
        } else {
            scroll_content
        };

        let content_area = div()
            .id("editor-scroll")
            .w_full()
            .h_full()
            .flex_1()
            .min_w(px(0.0))
            .bg(theme.colors.editor_background)
            .relative()
            .child(scroll_content);

        let content_area = if show_custom_scrollbar {
            let scrollbar_editor = editor.clone();
            let track_origin_y = f32::from(viewport_bounds.origin.y);
            content_area.child(
                div()
                    .id("editor-scrollbar-thumb")
                    .absolute()
                    .occlude()
                    .top(px(thumb_top))
                    .right(px(d.scrollbar_right))
                    .w(px(d.scrollbar_width))
                    .h(px(thumb_height))
                    .rounded(px(999.0))
                    .bg(theme.colors.scrollbar_thumb)
                    .cursor_pointer()
                    .on_hover(cx.listener(Self::on_editor_hover))
                    .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                        let pointer_offset_y =
                            f32::from(event.position.y) - track_origin_y - thumb_top;
                        let _ = scrollbar_editor.update(cx, |editor, cx| {
                            cx.stop_propagation();
                            editor.start_scrollbar_drag(
                                pointer_offset_y,
                                track_height,
                                thumb_height,
                                max_scroll_y,
                                cx,
                            );
                        });
                    })
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |_thumb_bounds, _, window, _| {
                                window.on_mouse_event({
                                    let editor = editor.clone();
                                    move |_event: &MouseUpEvent, phase, _window, cx| {
                                        if !phase.bubble() {
                                            return;
                                        }
                                        let _ = editor.update(cx, |editor, cx| {
                                            editor.end_scrollbar_drag(cx);
                                        });
                                    }
                                });

                                window.on_mouse_event({
                                    let editor = editor.clone();
                                    move |event: &MouseMoveEvent, phase, _window, cx| {
                                        if !phase.bubble() || !event.dragging() {
                                            return;
                                        }

                                        let pointer_y_in_track =
                                            f32::from(event.position.y) - track_origin_y;
                                        let _ = editor.update(cx, |editor, cx| {
                                            editor.update_scrollbar_drag(pointer_y_in_track, cx);
                                        });
                                    }
                                });
                            },
                        )
                        .size_full(),
                    ),
            )
        } else {
            content_area
        };

        let content_area = content_area.into_any_element();
        let content_area = if self.quick_open.is_some() {
            self.render_quick_open_overlay(&theme, cx)
        } else if self.command_palette.is_some() {
            crate::editor::command_palette::render_command_palette_overlay(self, &theme, cx)
        } else {
            content_area
        };
        // A tab whose file the text editor can't preview replaces the whole
        // content area with a centered notice, VS Code style.
        let content_area = if self.show_welcome {
            self.render_welcome_page(&theme, &strings, cx)
        } else if let Some(path) = self.unsupported_preview_path.as_ref() {
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            div()
                .id("unsupported-preview")
                .w_full()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .text_color(theme.colors.dialog_muted)
                .child(
                    div()
                        .w(px(44.0))
                        .h(px(44.0))
                        .rounded(px(22.0))
                        .border_1()
                        .border_color(theme.colors.dialog_border)
                        .bg(theme.colors.dialog_secondary_button_bg)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(26.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.colors.dialog_muted)
                        .child("!"),
                )
                .child(
                    div()
                        .text_size(px(theme.typography.text_size))
                        .text_color(theme.colors.text_default)
                        .child(file_name),
                )
                .child(
                    div()
                        .text_size(px(theme.typography.text_size * 0.9))
                        .child(strings.workspace_preview_unavailable_message.clone()),
                )
                .children(self.unsupported_preview_detail.as_ref().map(|detail| {
                    div()
                        .px(px(12.0))
                        .text_size(px(theme.typography.text_size * 0.8))
                        .text_color(theme.colors.dialog_muted)
                        .text_align(TextAlign::Center)
                        .child(detail.clone())
                }))
                .into_any_element()
        } else {
            content_area
        };
        let document_tabs = self.render_document_tabs(&theme, cx);
        // Document tabs live inside the custom titlebar when it is visible;
        // without one (macOS fullscreen, server-side decorations) they fall
        // back to a standalone row above the editor column.
        let (titlebar_tabs, column_tabs) = if titlebar_height > 0.0 {
            (document_tabs, None)
        } else {
            (None, document_tabs)
        };
        let content_area = div()
            .id("editor-column")
            .w_full()
            .h_full()
            .flex_1()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .children(column_tabs.map(|tabs| {
                div()
                    .id("document-tabs-fallback")
                    .w_full()
                    .h(px(36.0))
                    .flex_shrink_0()
                    .flex()
                    .bg(theme.colors.dialog_surface)
                    .border_b(px(theme.dimensions.dialog_border_width))
                    .border_color(theme.colors.dialog_border)
                    .child(tabs)
                    .into_any_element()
            }))
            .child(content_area)
            .into_any_element();
        let content_area = if self.source_mode_fallback_required && !self.code_tab_active() {
            div()
                .id("source-mode-fallback-container")
                .w_full()
                .h_full()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("source-mode-fallback-notice")
                        .w_full()
                        .flex_shrink_0()
                        .px(px(16.0))
                        .py(px(9.0))
                        .border_b(px(1.0))
                        .border_color(theme.colors.callout_warning_border)
                        .bg(theme.colors.callout_warning_bg)
                        .text_size(px(theme.typography.text_size * 0.82))
                        .text_color(theme.colors.text_default)
                        .child(strings.source_mode_fallback_message.clone()),
                )
                .child(div().w_full().flex_1().min_h(px(0.0)).child(content_area))
                .into_any_element()
        } else {
            content_area
        };

        let body_font_family = if fonts.markdown_family == "theme" {
            &theme.typography.body_font_family
        } else {
            &fonts.markdown_family
        };
        let base = div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .relative()
            .bg(theme.colors.editor_background)
            .font(editor_text_font(body_font_family))
            .on_mouse_move(cx.listener(Self::on_workspace_resize_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(Self::on_workspace_resize_mouse_up),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(Self::on_workspace_resize_mouse_up),
            )
            .capture_action(cx.listener(Self::on_copy_capture))
            .capture_action(cx.listener(Self::on_cut_capture))
            .capture_action(cx.listener(Self::on_delete_capture))
            .capture_action(cx.listener(Self::on_delete_back_capture))
            .capture_action(cx.listener(Self::on_bold_capture))
            .capture_action(cx.listener(Self::on_italic_capture))
            .capture_action(cx.listener(Self::on_underline_capture))
            .capture_action(cx.listener(Self::on_strikethrough_capture))
            .capture_action(cx.listener(Self::on_highlight_capture))
            .capture_action(cx.listener(Self::on_code_capture))
            .capture_action(cx.listener(Self::on_superscript_capture))
            .capture_action(cx.listener(Self::on_subscript_capture))
            .capture_action(cx.listener(Self::on_heading1_capture))
            .capture_action(cx.listener(Self::on_heading2_capture))
            .capture_action(cx.listener(Self::on_heading3_capture))
            .capture_action(cx.listener(Self::on_heading4_capture))
            .capture_action(cx.listener(Self::on_heading5_capture))
            .capture_action(cx.listener(Self::on_heading6_capture))
            .capture_action(cx.listener(Self::on_paragraph_text_capture))
            .capture_key_down(cx.listener(Self::on_editor_key_down_capture))
            .can_drop(|dragged, _window, _cx| dragged.is::<ExternalPaths>())
            .on_drop::<ExternalPaths>(cx.listener(Self::on_external_paths_drop))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_save_document))
            .on_action(cx.listener(Self::on_file_history_action))
            .on_action(cx.listener(Self::on_save_document_as))
            .on_action(cx.listener(Self::on_export_html))
            .on_action(cx.listener(Self::on_export_pdf))
            .on_action(cx.listener(Self::on_export_png))
            .on_action(cx.listener(Self::on_quit_application))
            .on_action(cx.listener(Self::on_close_window))
            .on_action(cx.listener(Self::on_toggle_view_mode_action))
            .on_action(cx.listener(Self::on_find_in_document))
            .on_action(cx.listener(Self::on_find_next_match))
            .on_action(cx.listener(Self::on_find_previous_match))
            .on_action(cx.listener(Self::on_toggle_workspace_action))
            .on_action(cx.listener(Self::on_select_tab_index))
            .on_action(cx.listener(Self::on_quick_open_action))
            .on_action(cx.listener(Self::on_open_command_palette))
            .on_action(cx.listener(Self::on_cursor_history_back))
            .on_action(cx.listener(Self::on_cursor_history_forward))
            .on_action(cx.listener(Self::on_copy_as_html))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::on_page_up))
            .on_action(cx.listener(Self::on_page_down))
            .on_action(cx.listener(Self::on_jump_to_top))
            .on_action(cx.listener(Self::on_jump_to_bottom))
            .on_action(cx.listener(Self::on_dismiss_transient_ui))
            .on_action(cx.listener(Self::on_install_cli_tool))
            .on_action(cx.listener(Self::on_uninstall_cli_tool));
        // Fetch menus + collect labels once for both renderers; previously each
        // of render_in_window_menu_bar / render_in_window_menu_panel called
        // cx.get_menus() and walked menus.iter().map(|m| m.name.to_string())
        // independently — two redundant Vec<OwnedMenu> + two redundant
        // Vec<String>-of-N-allocations per frame.
        let menus = supports_in_window_menu()
            .then(|| cx.get_menus())
            .flatten()
            .filter(|m| !m.is_empty());
        let menu_labels: Vec<SharedString> = menus
            .as_ref()
            .map(|m| m.iter().map(|menu| menu.name.clone()).collect())
            .unwrap_or_default();
        // Windows：一级菜单入口是标题栏左侧的汉堡按钮，不再单占一行。
        let hamburger_menu = (supports_hamburger_menu() && menus.is_some())
            .then(|| self.render_hamburger_menu_button(&theme, cx));
        let base = if let Some(titlebar) = render_custom_titlebar(
            "editor-titlebar",
            format!("Velora - {}", self.workspace_breadcrumb()).into(),
            hamburger_menu,
            titlebar_tabs,
            &theme,
            window,
            cx,
            Self::on_titlebar_close,
        ) {
            base.child(titlebar)
        } else {
            base
        };
        let base = if supports_menu_bar_row() {
            if let Some(menu_bar) = self.render_in_window_menu_bar(
                &theme,
                cx,
                menus.as_deref(),
                &menu_labels,
                titlebar_height,
            ) {
                base.child(menu_bar)
            } else {
                base
            }
        } else {
            base
        };
        let workspace_width =
            self.current_workspace_panel_width(f32::from(window.viewport_size().width), cx);
        let workspace_panel =
            self.render_workspace_panel(&theme, &strings, workspace_width, window, cx);
        let mut main_content = div()
            .w_full()
            .flex_1()
            .min_h(px(0.0))
            .pt(px(titlebar_height + menu_bar_height))
            .flex()
            .min_w(px(0.0))
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                // 指针跑到浮层右边的正文里就收回（浮层宽度 = 窄条 + 面板）。
                if event.position.x > px(SIDEBAR_RAIL_WIDTH_PX + workspace_width) {
                    this.set_sidebar_peek(false, cx);
                }
            }));
        if self.workspace.is_open {
            // 展开：窄条与面板占位，正文被挤到右边。
            main_content = main_content.child(self.render_activity_rail(&theme, cx));
            if let Some(workspace_panel) = workspace_panel {
                main_content = main_content.child(workspace_panel);
            }
            main_content = main_content.child(content_area);
        } else {
            // 收起：整条侧边栏不占布局，正文占满整宽。指针贴到左边缘时整条侧边栏作为
            // 浮层滑出、盖在正文上（不挤压排版），移开带动画收回。顶边和展开时对齐
            // （标题栏 + 菜单栏之下），否则浮层会从窗口最顶上冒出来，盖住红绿灯和
            // 标签栏。
            let sidebar_top = px(titlebar_height + menu_bar_height);
            main_content = main_content.child(content_area);
            main_content = main_content.child(
                div()
                    .id("sidebar-auto-hide-edge")
                    .debug_selector(|| "sidebar-auto-hide-edge".to_string())
                    .absolute()
                    .left_0()
                    .top(sidebar_top)
                    .bottom_0()
                    .w(px(SIDEBAR_AUTO_HIDE_EDGE_PX))
                    .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                        // 进出贴边区都先递增 generation 作废挂着的停留定时器：
                        // 进入时换发新定时器，停留满才唤出；离开/再进入则让旧
                        // 定时器到点也不生效（防误触，见 SIDEBAR_PEEK_DWELL）。
                        this.sidebar_edge_dwell_generation =
                            this.sidebar_edge_dwell_generation.wrapping_add(1);
                        if *hovered {
                            let generation = this.sidebar_edge_dwell_generation;
                            let dwell = crate::editor::render::SIDEBAR_PEEK_DWELL;
                            cx.spawn(async move |editor, cx| {
                                cx.background_executor().timer(dwell).await;
                                _ = editor.update(cx, |editor, cx| {
                                    if editor.sidebar_edge_dwell_generation == generation {
                                        editor.set_sidebar_peek(true, cx);
                                    }
                                });
                            })
                            .detach();
                        }
                    })),
            );
            if let Some(workspace_panel) = workspace_panel {
                let overlay_width = px(SIDEBAR_RAIL_WIDTH_PX + workspace_width);
                let overlay = div()
                    .id("sidebar-auto-hide-overlay")
                    .debug_selector(|| "sidebar-auto-hide-overlay".to_string())
                    .absolute()
                    .left_0()
                    .top(sidebar_top)
                    .bottom_0()
                    // 显式宽度：绝对定位下不给宽度会按父级拉伸，鼠标移到正文时仍算
                    // 「在浮层内」，退出事件永远不触发。宽度 = 窄条 + 面板。
                    .w(overlay_width)
                    // 遮挡命中：浮层盖着正文，不挡住的话滚轮会同时命中浮层里的文件
                    // 树和后面的编辑器滚动区（用户报修：收起侧栏贴边唤出后，在浮层
                    // 里滚树把正文也带着滚了），点击也会穿透到正文。
                    .occlude()
                    .flex()
                    .border_r(px(1.0))
                    .border_color(theme.colors.dialog_border)
                    .child(self.render_activity_rail(&theme, cx))
                    .child(workspace_panel);
                // 唤出滑入 / 收回滑出都用负 left 把浮层整体推到左边界外：
                // `with_animation` 在元素每次挂载时从头播放（滑入/滑出是两个
                // 不同 id 的包装，切换状态即重播），动画结束后每帧按 delta=1
                // 收敛在终态。收回动画期间（sidebar_overlay_closing）浮层仍
                // 挂载，播完由 workspace.rs 的定时器卸载。
                let layer = if self.sidebar_peek {
                    overlay
                        .with_animation(
                            "sidebar-overlay-slide-in",
                            Animation::new(SIDEBAR_SLIDE_DURATION).with_easing(ease_out_quint()),
                            move |slide, delta| slide.left(overlay_width * (delta - 1.0)),
                        )
                        .into_any_element()
                } else {
                    debug_assert!(self.sidebar_overlay_closing, "面板此时只应随动画挂载");
                    overlay
                        .with_animation(
                            "sidebar-overlay-slide-out",
                            Animation::new(SIDEBAR_SLIDE_DURATION).with_easing(quadratic),
                            move |slide, delta| slide.left(-overlay_width * delta),
                        )
                        .into_any_element()
                };
                main_content = main_content.child(layer);
            }
        }
        let base = base.child(main_content);
        let base = if let Some(status_bar) = self.render_status_bar(&theme, &strings, window, cx) {
            base.child(status_bar)
        } else {
            base
        };
        let base = if let Some(hamburger_list) = menus.as_deref().and_then(|menus| {
            self.render_hamburger_menu_panel(&theme, cx, menus, titlebar_height)
        }) {
            base.child(hamburger_list)
        } else {
            base
        };
        let base = if let Some(open_index) = self.menu_bar_open {
            let dimensions = &theme.dimensions;
            let origin = if self.hamburger_menu_open && supports_hamburger_menu() {
                hamburger_menu_item_panel_origin(
                    open_index,
                    titlebar_height,
                    &menu_labels,
                    dimensions,
                )
            } else {
                MenuPanelOrigin {
                    panel_left: menu_panel_left(open_index, &menu_labels, dimensions),
                    panel_top: titlebar_height,
                }
            };
            if let Some(menu_panel) = self.render_in_window_menu_panel(
                &theme,
                cx,
                menus.as_deref(),
                origin,
                f32::from(window.viewport_size().height.max(px(1.0))),
            ) {
                base.child(menu_panel)
            } else {
                base
            }
        } else {
            base
        };
        let base = if let Some(toolbar) = self.render_selection_toolbar(&theme, window, cx) {
            base.child(toolbar)
        } else {
            base
        };
        let base = if let Some(context_menu) =
            self.render_context_menu_overlay(&theme, window.viewport_size(), cx)
        {
            base.child(context_menu)
        } else {
            base
        };
        let base =
            if let Some(menu) = self.render_workspace_context_menu_overlay(&theme, window, cx) {
                base.child(menu)
            } else {
                base
            };
        let base = if let Some(menu) = self.render_tab_context_menu_overlay(&theme, window, cx) {
            base.child(menu)
        } else {
            base
        };
        let base = if let Some(table_dialog) = self.render_table_insert_dialog_overlay(&theme, cx) {
            base.child(table_dialog)
        } else {
            base
        };
        let base = if self.wikilink_completion_is_open()
            && let Some(completion) = self.render_wikilink_completion_overlay(&theme, window, cx)
        {
            base.child(completion)
        } else {
            base
        };
        let base = if self.file_history_is_open()
            && let Some(overlay) = self.render_file_history_overlay(&theme, &strings, cx)
        {
            base.child(overlay)
        } else {
            base
        };
        if let Some(kind) = self.info_dialog {
            base.child(self.render_info_dialog_overlay(&theme, kind, cx))
        } else if self.modal_is_open() {
            match self.render_modal_overlay(&theme, cx) {
                Some(overlay) => base.child(overlay),
                None => base,
            }
        } else if self.show_drop_replace_dialog {
            base.child(self.render_drop_replace_overlay(&theme, cx))
        } else if self.show_unsaved_changes_dialog {
            base.child(self.render_unsaved_changes_overlay(&theme, cx))
        } else if self.pending_folder_choice.is_some() {
            base.child(self.render_folder_choice_overlay(&theme, cx).into_any_element())
        } else {
            base
        }
    }
}
