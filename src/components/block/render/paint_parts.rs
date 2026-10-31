use super::*;
use gpui_kit::component::{
    Sizable as _,
    button::{Button, ButtonVariants},
};

impl Focusable for Block {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// The render method builds the full element tree for a block:
/// - Common wrapper: key_context, track_focus, action handlers, mouse events.
/// - Kind-specific styling: headings get size/weight/border, list items get
///   a flex row with marker + content, everything else renders as plain text.
/// - The [`BlockTextElement`] handles text layout, selection, and cursor.
impl Render for Block {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A stray closing tag parses to an empty HTML document: the text stays
        // in the tree for source mode, rendered mode draws no row.
        if self.renders_nothing() {
            return div().w_full().into_any_element();
        }

        let focused = self.focus_handle.is_focused(window);
        if !focused || self.record.kind != BlockKind::MathBlock {
            self.math_edit_preview_source = None;
        }
        self.set_hide_markdown_markers(
            self.is_read_only() || crate::config::EditorSettings::hide_markdown_markers(cx),
        );
        let code_language_focused = self.code_language_focus_handle.is_focused(window);
        // 输入态（含 33ms 光标闪烁任务）还要求窗口是活动窗口：gpui 的 `is_focused`
        // 判不出窗口已不是 key window（macOS 红绿灯变灰时 window.focus 不清零），
        // 只看焦点时后台窗口持续闪烁、前台线程每拍全块重绘；公式编辑器的草稿
        // 光标（formula_editor.rs）同一条语义。
        let input_active = !self.is_read_only()
            && (focused || code_language_focused)
            && window.is_window_active();
        if self.sync_image_focus_state(focused && !self.is_read_only()) {
            cx.notify();
        }

        let showing_rendered_image = self.showing_rendered_image();
        // Inline math stays in the projected view while focused (its `$...$`
        // source shows as editable text), so links and other styling in the same
        // block keep their attributes instead of collapsing to raw Markdown, the
        // same way script spans already behave.
        self.sync_inline_projection_for_focus_notified(focused && !showing_rendered_image, cx);

        if input_active && self.cursor_blink_task.is_none() {
            self.start_cursor_blink(cx);
        } else if !input_active && self.cursor_blink_task.is_some() {
            self.cursor_blink_task = None;
        }
        if !input_active {
            self.reset_code_language_input_layout();
        }

        let block_id = ElementId::Name(format!("block-{}", self.record.id).into());
        let is_placeholder =
            focused && self.display_text().is_empty() && self.marked_range.is_none();

        // 文档块必须和编辑器外壳共用字号派生：块以前只套字号、漏掉界面缩放，
        // 导致缩放设置对正文完全没反应。派生收进 ThemeManager
        // 缓存成 Arc：块只借用，不再每块每帧克隆整份 Theme 自己派生。
        let theme = cx.global::<ThemeManager>().derived_theme(cx);
        let fonts = crate::config::EditorSettings::fonts(cx);
        let strings = cx.global::<I18nManager>().strings_arc();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let depth_padding = d.block_padding_x + d.nested_block_indent * self.render_depth as f32;

        if self.is_table_cell() {
            let is_header = self
                .table_cell_position()
                .map(|position| position.is_header())
                .unwrap_or(false);
            let style_as_header =
                is_header && crate::config::EditorSettings::show_table_headers(cx);
            let highlight = self.table_axis_highlight;
            let base_bg = if style_as_header {
                c.table_header_bg
            } else {
                c.table_cell_bg
            };
            let (bg, border_color) = table_cell_colors(base_bg, highlight, focused, c);
            let bg = if self.table_cell_hovered && highlight == TableAxisHighlight::None {
                bg.blend(Hsla { a: 0.04, ..c.text_link })
            } else {
                bg
            };
            let text_color = if style_as_header {
                c.text_link
            } else {
                c.text_default
            };
            let cell_base = self
                .render_shell(
                    block_id,
                    false,
                    focused,
                    if showing_rendered_image {
                        CursorStyle::PointingHand
                    } else {
                        CursorStyle::IBeam
                    },
                    0.0,
                    0.0,
                    d,
                    cx,
                )
                .w_full()
                .h_full()
                .min_h(px(d.table_cell_min_height))
                .px(px(d.table_cell_padding_x))
                .py(px(d.table_cell_padding_y))
                .border(px(1.0))
                .border_color(border_color)
                .rounded_tl(self.table_cell_corner_radii.top_left)
                .rounded_tr(self.table_cell_corner_radii.top_right)
                .rounded_bl(self.table_cell_corner_radii.bottom_left)
                .rounded_br(self.table_cell_corner_radii.bottom_right)
                .overflow_hidden()
                .on_hover(cx.listener(|block, hovered, _, cx| {
                    block.table_cell_hovered = *hovered;
                    cx.notify();
                }))
                .bg(bg)
                .text_size(px(t.text_size))
                .text_color(text_color)
                .line_height(relative(t.text_line_height));

            let cell_base = if style_as_header {
                cell_base.font_weight(FontWeight::BOLD)
            } else {
                cell_base
            };

            if showing_rendered_image && let Some(runtime) = self.image_runtime() {
                return cell_base
                    .child(self.render_image_content(
                        runtime,
                        Length::Definite(relative(1.0)),
                        px(d.image_cell_max_height),
                        px(d.image_cell_placeholder_height),
                        &theme,
                        &strings,
                    ))
                    .into_any_element();
            }

            if !focused
                && let Some(inline_images) = self.render_table_cell_inline_images(
                    &theme,
                    &strings,
                    if style_as_header {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    },
                    cx,
                )
            {
                return cell_base.child(inline_images).into_any_element();
            }

            return cell_base
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    text_color,
                    t.text_size,
                    if style_as_header {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    },
                    cx,
                ))
                .into_any_element();
        }

        // Source-mode rendering: raw text with no formatting.
        // 聚焦的数学块要放行到下面的 kind 分支：ƒx 按钮、公式编辑卡片与
        // SVG↔文本切换都挂在那里——这里早退会把它们全拦成死代码。
        if self.is_source_raw_mode()
            && (focused && !self.is_read_only() && self.kind() != BlockKind::MathBlock
                || (!focused || self.is_read_only())
                    && !matches!(
                        self.kind(),
                        BlockKind::HtmlBlock | BlockKind::MathBlock | BlockKind::MermaidBlock
                    ))
        {
            // 闪烁开关同上一处：焦点之外还要窗口是活动窗口；分支选择本身仍只看
            // 焦点——后台化不能改变这里渲染哪种元素树。
            let blink_active = focused && !self.is_read_only() && window.is_window_active();
            if blink_active && self.cursor_blink_task.is_none() {
                self.start_cursor_blink(cx);
            } else if !blink_active && self.cursor_blink_task.is_some() {
                self.cursor_blink_task = None;
            }
            // 行号视图（源码文档分块）与代码块一样用等宽字体和代码字号：
            // 行号跟着正文字体走，等宽才能上下对齐。
            let source_document = self.show_source_line_numbers();
            let source_base = self
                .render_shell(
                    block_id.clone(),
                    true,
                    focused,
                    CursorStyle::IBeam,
                    d.block_padding_x,
                    d.block_padding_x,
                    d,
                    cx,
                )
                .text_size(px(if self.kind().is_code_block() || source_document {
                    t.code_size
                } else {
                    t.text_size
                }))
                .text_color(c.text_default)
                .line_height(relative(t.text_line_height));

            let source_base = if self.kind().is_code_block() || source_document {
                source_base.font(font(fonts.code_family.clone()))
            } else {
                source_base
            };

            let source_base = if self.kind().is_code_block() {
                source_base
            } else if self.kind() == BlockKind::Comment {
                source_base.bg(c.comment_bg).rounded_sm()
            } else if focused && !source_document {
                // 行号视图的块是 512 行一切的分块：聚焦底色会把整屏铺满。
                // 光标定位有当前行高亮兜底，这里不再上块底色。
                source_base.bg(c.source_mode_block_bg).rounded_sm()
            } else {
                source_base
            };

            return source_base
                .child(
                    // 超长行折叠成单行后不做横向滚动：超出块宽的部分直接裁掉，
                    // 想看全文走行号展开。
                    div().min_w(px(0.0)).w_full().overflow_hidden()
                        .child(BlockTextElement::new(cx.entity(), is_placeholder)),
                )
                .into_any_element();
        }

        // 空行块不渲染内容：空的文本元素自带一行高，会把 min_h 撑开。
        if self.collapses_to_blank_gap(focused, false) {
            // 这一帧没有文本元素：上一帧量下的几何必须一起作废。留着它，
            // 点击视觉上属于下方块的区域也会被旧 bounds 认领进这个空块。
            self.last_layout = None;
            self.last_bounds = None;
            return self
                .render_shell(
                    block_id,
                    false,
                    focused,
                    CursorStyle::IBeam,
                    depth_padding,
                    d.block_padding_x,
                    d,
                    cx,
                )
                .into_any_element();
        }

        let focused_base = self.render_shell(
            block_id.clone(),
            false,
            focused,
            if showing_rendered_image {
                CursorStyle::PointingHand
            } else {
                CursorStyle::IBeam
            },
            if self.kind().is_separator() {
                depth_padding + d.separator_inset_x
            } else {
                depth_padding
            },
            if self.kind().is_separator() {
                d.block_padding_x + d.separator_inset_x
            } else {
                d.block_padding_x
            },
            d,
            cx,
        );
        let focused_base = if fonts.markdown_family == "theme"
            && matches!(self.kind(), BlockKind::Heading { .. })
        {
            focused_base.font(font(t.heading_font_family.clone()))
        } else {
            focused_base
        };

        if showing_rendered_image && self.kind() == BlockKind::Paragraph {
            let max_width: Length = if d.image_root_max_width > 0.0 {
                px(d.image_root_max_width).into()
            } else {
                relative(1.0).into()
            };
            if let Some(runtime) = self.image_runtime() {
                return focused_base
                    .child(self.render_image_content(
                        runtime,
                        max_width,
                        px(d.image_root_max_height),
                        px(d.image_root_placeholder_height),
                        &theme,
                        &strings,
                    ))
                    .into_any_element();
            }
        }

        let content = match self.kind() {
            BlockKind::Separator => focused_base
                .py(px(d.separator_margin_y))
                .child(
                    div()
                        .w_full()
                        .h(px(d.separator_thickness))
                        .bg(c.separator_color)
                        .rounded(px(999.0)),
                )
                .into_any_element(),
            BlockKind::Heading { level: 1 } => {
                let text = self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h1,
                    t.h1_size,
                    t.h1_weight.to_font_weight(),
                    cx,
                );
                self.heading_fold_row(
                    focused_base
                        .text_size(px(t.h1_size))
                        .font_weight(t.h1_weight.to_font_weight())
                        .text_color(c.text_h1)
                        .pb(px(d.h1_padding_bottom))
                        .mb(px(d.h1_margin_bottom))
                        .border_b(px(d.h1_border_width))
                        .border_color(c.border_h1),
                    text,
                    t.h1_size,
                    &theme,
                    cx,
                )
                .into_any_element()
            }
            BlockKind::Heading { level: 2 } => {
                let text = self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h2,
                    t.h2_size,
                    t.h2_weight.to_font_weight(),
                    cx,
                );
                self.heading_fold_row(
                    focused_base
                        .text_size(px(t.h2_size))
                        .font_weight(t.h2_weight.to_font_weight())
                        .text_color(c.text_h2)
                        .pb(px(d.h1_padding_bottom))
                        .mb(px(d.h1_margin_bottom))
                        .border_b(px(d.h1_border_width))
                        .border_color(c.border_h2),
                    text,
                    t.h2_size,
                    &theme,
                    cx,
                )
                .into_any_element()
            }
            BlockKind::Heading { level: 3 } => {
                let text = self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h3,
                    t.h3_size,
                    t.h3_weight.to_font_weight(),
                    cx,
                );
                self.heading_fold_row(
                    focused_base
                        .text_size(px(t.h3_size))
                        .font_weight(t.h3_weight.to_font_weight())
                        .text_color(c.text_h3),
                    text,
                    t.h3_size,
                    &theme,
                    cx,
                )
                .into_any_element()
            }
            BlockKind::Heading { level: 4 } => {
                let text = self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h4,
                    t.h4_size,
                    t.h4_weight.to_font_weight(),
                    cx,
                );
                self.heading_fold_row(
                    focused_base
                        .text_size(px(t.h4_size))
                        .font_weight(t.h4_weight.to_font_weight())
                        .text_color(c.text_h4),
                    text,
                    t.h4_size,
                    &theme,
                    cx,
                )
                .into_any_element()
            }
            BlockKind::Heading { level: 5 } => {
                let text = self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h5,
                    t.h5_size,
                    t.h5_weight.to_font_weight(),
                    cx,
                );
                self.heading_fold_row(
                    focused_base
                        .text_size(px(t.h5_size))
                        .font_weight(t.h5_weight.to_font_weight())
                        .text_color(c.text_h5),
                    text,
                    t.h5_size,
                    &theme,
                    cx,
                )
                .into_any_element()
            }
            BlockKind::Heading { level: 6 } => {
                let text = self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_h6,
                    t.h6_size,
                    t.h6_weight.to_font_weight(),
                    cx,
                );
                self.heading_fold_row(
                    focused_base
                        .text_size(px(t.h6_size))
                        .font_weight(t.h6_weight.to_font_weight())
                        .text_color(c.text_h6),
                    text,
                    t.h6_size,
                    &theme,
                    cx,
                )
                .into_any_element()
            }
            BlockKind::BulletedListItem => focused_base
                .text_size(px(t.text_size))
                .text_color(c.text_default)
                .line_height(relative(t.text_line_height))
                .w_full()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(d.list_marker_gap))
                .children([
                    self.list_marker_column(&theme)
                        .child(
                            div()
                                .size(px(t.text_size * if self.render_depth >= 2 { 0.30 } else { 0.36 }))
                                .flex_shrink_0()
                                .when(self.render_depth <= 1, |this| this.rounded_full())
                                .when(self.render_depth != 1, |this| this.bg(c.text_link))
                                .when(self.render_depth == 1, |this| this.border(px(1.25)).border_color(c.text_link)),
                        ),
                    if showing_rendered_image {
                        // 列表项里的图片按「所在列的可用宽度」封顶（relative(1.0)）：
                        // 按视口估算量不到 callout/嵌套的真实容器，图片与占位框会
                        // 冲出右边界。
                        let max_width = relative(1.0);
                        if let Some(runtime) = self.image_runtime() {
                            div().flex_grow(1.0).child(self.render_image_content(
                                runtime,
                                max_width.into(),
                                px(d.image_root_max_height),
                                px(d.image_root_placeholder_height),
                                &theme,
                                &strings,
                            ))
                        } else {
                            div().min_w(px(0.0)).flex_grow(1.0).child(
                                self.render_text_or_mixed_inline_visuals(
                                    &theme,
                                    focused,
                                    is_placeholder,
                                    None,
                                    None,
                                    c.text_default,
                                    t.text_size,
                                    FontWeight::NORMAL,
                                    cx,
                                ),
                            )
                        }
                    } else {
                        div().min_w(px(0.0)).flex_grow(1.0).child(
                            self.render_text_or_mixed_inline_visuals(
                                &theme,
                                focused,
                                is_placeholder,
                                None,
                                None,
                                c.text_default,
                                t.text_size,
                                FontWeight::NORMAL,
                                cx,
                            ),
                        )
                    },
                ])
                .into_any_element(),
            BlockKind::TaskListItem { checked } => {
                focused_base
                    .text_size(px(t.text_size))
                    .text_color(c.text_default)
                    .line_height(relative(t.text_line_height))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(d.list_marker_gap))
                    .children([
                        self.list_marker_column(&theme)
                            .child(
                                div()
                                    .size(px(d.task_checkbox_size))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(d.task_checkbox_radius))
                                    .border(px(d.task_checkbox_border_width))
                                    .border_color(c.task_checkbox_border)
                                    .bg(if checked {
                                        c.task_checkbox_checked_bg
                                    } else {
                                        c.task_checkbox_bg
                                    })
                                    .text_size(px(d.task_checkbox_check_size))
                                    .text_color(c.task_checkbox_check)
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(Self::on_task_checkbox_mouse_down),
                                    )
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(Self::on_task_checkbox_mouse_up),
                                    )
                                    .child(if checked {
                                        SharedString::new(TASK_CHECKMARK)
                                    } else {
                                        SharedString::new("")
                                    }),
                            ),
                        if showing_rendered_image {
                            let max_width = relative(1.0);
                            if let Some(runtime) = self.image_runtime() {
                                div().flex_grow(1.0).child(self.render_image_content(
                                    runtime,
                                    max_width.into(),
                                    px(d.image_root_max_height),
                                    px(d.image_root_placeholder_height),
                                    &theme,
                                    &strings,
                                ))
                            } else {
                                div().min_w(px(0.0)).flex_grow(1.0).child(
                                    self.render_text_or_mixed_inline_visuals(
                                        &theme,
                                        focused,
                                        is_placeholder,
                                        None,
                                        None,
                                        c.text_default,
                                        t.text_size,
                                        FontWeight::NORMAL,
                                        cx,
                                    ),
                                )
                            }
                        } else {
                            div().min_w(px(0.0)).flex_grow(1.0).child(
                                self.render_text_or_mixed_inline_visuals(
                                    &theme,
                                    focused,
                                    is_placeholder,
                                    None,
                                    None,
                                    c.text_default,
                                    t.text_size,
                                    FontWeight::NORMAL,
                                    cx,
                                ),
                            )
                        },
                    ])
                    .into_any_element()
            }
            BlockKind::NumberedListItem => focused_base
                .text_size(px(t.text_size))
                .text_color(c.text_default)
                .line_height(relative(t.text_line_height))
                .w_full()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(d.list_marker_gap))
                .children([
                    self.list_marker_column(&theme)
                        .justify_end()
                        .text_color(c.text_link)
                        .child(div().flex_shrink_0().whitespace_nowrap()
                            .font_weight(FontWeight::MEDIUM)
                            .child(SharedString::from(numbered_list_marker(
                                self.render_depth,
                                self.list_ordinal.unwrap_or(1),
                                self.record.list_marker.delimiter_or_default(),
                            )))),
                    if showing_rendered_image {
                        // 列表项里的图片按「所在列的可用宽度」封顶（relative(1.0)）：
                        // 按视口估算量不到 callout/嵌套的真实容器，图片与占位框会
                        // 冲出右边界。
                        let max_width = relative(1.0);
                        if let Some(runtime) = self.image_runtime() {
                            div().flex_grow(1.0).child(self.render_image_content(
                                runtime,
                                max_width.into(),
                                px(d.image_root_max_height),
                                px(d.image_root_placeholder_height),
                                &theme,
                                &strings,
                            ))
                        } else {
                            div().min_w(px(0.0)).flex_grow(1.0).child(
                                self.render_text_or_mixed_inline_visuals(
                                    &theme,
                                    focused,
                                    is_placeholder,
                                    None,
                                    None,
                                    c.text_default,
                                    t.text_size,
                                    FontWeight::NORMAL,
                                    cx,
                                ),
                            )
                        }
                    } else {
                        div().min_w(px(0.0)).flex_grow(1.0).child(
                            self.render_text_or_mixed_inline_visuals(
                                &theme,
                                focused,
                                is_placeholder,
                                None,
                                None,
                                c.text_default,
                                t.text_size,
                                FontWeight::NORMAL,
                                cx,
                            ),
                        )
                    },
                ])
                .into_any_element(),
            BlockKind::Quote
                if !focused
                    && self.display_text().is_empty()
                    && !self.children.is_empty()
                    && self.marked_range.is_none() =>
            {
                // 空标题只承担容器结构，子块前不应再多出一行引用空白。
                self.last_layout = None;
                self.last_bounds = None;
                div().w_full().h(px(0.0)).into_any_element()
            }
            BlockKind::Quote => focused_base
                .text_size(px(t.text_size))
                .text_color(c.text_quote)
                .line_height(relative(t.text_line_height))
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_quote,
                    t.text_size,
                    FontWeight::NORMAL,
                    cx,
                ))
                .into_any_element(),
            BlockKind::Callout(variant) => {
                let (accent, _) = callout_accent_and_background(variant, &theme);
                let title_is_empty = self.record.title.visible_text().is_empty();
                let show_static_default_label = title_is_empty && !focused;
                let header_label = SharedString::from(variant.label());
                let header_text = if show_static_default_label {
                    div()
                        .text_size(px(t.text_size))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(accent)
                        .child(header_label.clone())
                        .into_any_element()
                } else {
                    div()
                        .min_w(px(0.0))
                        .flex_grow(1.0)
                        .text_size(px(t.text_size))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(accent)
                        .child(self.render_text_or_mixed_inline_visuals(
                            &theme,
                            focused,
                            is_placeholder,
                            Some(header_label),
                            Some(accent),
                            accent,
                            t.text_size,
                            FontWeight::SEMIBOLD,
                            cx,
                        ))
                        .into_any_element()
                };

                focused_base
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(d.callout_header_gap))
                    .child(
                        div()
                            .text_size(px(t.text_size))
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent)
                            .child(variant.icon()),
                    )
                    .child(header_text)
                    .into_any_element()
            }
            BlockKind::FootnoteDefinition => {
                let ordinal = self.footnote_definition_ordinal();
                let badge = ordinal
                    .map(|ordinal| ordinal.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let badge_text_size = px((t.code_size - 1.0).max(10.0));
                let header = focused_base
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(d.list_marker_gap))
                    .text_size(px(t.code_size))
                    .text_color(c.text_quote)
                    .child(
                        div()
                            .px(px(d.footnote_badge_padding_x))
                            .py(px(d.footnote_badge_padding_y))
                            .rounded(px(999.0))
                            .bg(c.footnote_badge_bg)
                            .text_size(badge_text_size)
                            .text_color(c.footnote_badge_text)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(badge)),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .flex_grow(1.0)
                            .text_color(c.text_quote)
                            .child(self.render_text_or_mixed_inline_visuals(
                                &theme,
                                focused,
                                is_placeholder,
                                None,
                                None,
                                c.text_quote,
                                t.code_size,
                                FontWeight::NORMAL,
                                cx,
                            )),
                    );

                if self.footnote_definition_has_backref() {
                    header
                        .child(
                            div()
                                .text_color(c.footnote_backref)
                                .hover(|this| this.text_color(c.text_link))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_footnote_backref_mouse_down),
                                )
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(Self::on_footnote_backref_mouse_up),
                                )
                                .child("\u{21A9}"),
                        )
                        .into_any_element()
                } else {
                    header.into_any_element()
                }
            }
            BlockKind::CodeBlock { .. } => {
                let show_language_input = !self.is_read_only() && (focused || code_language_focused);
                let language_placeholder =
                    SharedString::from(strings.code_language_placeholder.clone());
                let code_panel = focused_base
                    .bg(c.code_bg)
                    .font(font(fonts.code_family.clone()))
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(c.table_border)
                    .shadow_sm()
                    .pl(px(d.code_block_padding_x))
                    .pr(px(d.code_block_padding_x))
                    .pt(px(48.0))
                    .pb(px(d.code_block_padding_y))
                    .text_size(px(t.code_size))
                    .text_color(c.code_text)
                    .line_height(relative(1.5))
                    .child(
                        // 同上：折叠单行裁切显示，不横向滚动。
                        div().min_w(px(0.0)).w_full().overflow_hidden()
                            .child(BlockTextElement::new(cx.entity(), is_placeholder)),
                    );

                {
                    let input_height = d.code_language_input_height
                        + d.code_language_input_padding_y * 2.0
                        + d.code_language_input_border_width * 2.0;
                    let code_copied = self
                        .code_copied_at
                        .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(1200));
                    let copy_icon = if code_copied {
                        "icon/workspace/check.svg"
                    } else {
                        "icon/workspace/copy.svg"
                    };
                    let copy_tooltip: SharedString = strings.code_copy_button.clone().into();
                    let code_host = div()
                        .w(relative(VISUAL_BLOCK_WIDTH_RATIO))
                        .min_w(px(0.0))
                        .relative()
                        .child(code_panel)
                        .child(
                            div()
                                .debug_selector(|| "code-block-header".to_string())
                                .absolute()
                                .left(px(d.code_block_padding_x))
                                .right(px(d.code_block_padding_x))
                                .top(px(6.0))
                                .h(px(input_height))
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .children([0xff5f57ff, 0xffbd2eff, 0x28c840ff].map(|color| {
                                    div().size(px(9.0)).rounded_full().bg(rgba(color))
                                })),
                        )
                        .child(
                            div().absolute().top(px(40.0))
                                .left(px(d.code_block_padding_x)).right(px(d.code_block_padding_x))
                                .h(px(1.0)).bg(c.table_border),
                        )
                        .child(
                            Button::new("code-copy-button")
                                .debug_selector(|| "code-copy-button".to_string())
                                .ghost()
                                .small()
                                .compact()
                                .absolute()
                                .right(px(d.code_block_padding_x))
                                .top(px(6.0))
                                .occlude()
                                .h(px(input_height))
                                .w(px(input_height))
                                .rounded(px(4.0))
                                .accessibility_label(copy_tooltip.clone())
                                .tooltip(copy_tooltip)
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(cx.listener(Self::on_code_copy_button))
                                .child(svg().path(copy_icon).size(px(14.0))
                                    .text_color(if code_copied { c.text_link } else { c.text_placeholder })),
                        )
                        .when(show_language_input, |panel| panel.child(
                            div()
                                .absolute()
                                .left(px(d.code_block_padding_x + 48.0))
                                .top(px(6.0))
                                .occlude()
                                .key_context(BLOCK_EDITOR_CONTEXT)
                                .track_focus(&self.code_language_focus_handle)
                                .on_action(cx.listener(Self::on_code_language_newline))
                                .on_action(cx.listener(Self::on_code_language_dismiss))
                                .on_action(cx.listener(Self::on_code_language_delete_back))
                                .on_action(cx.listener(Self::on_code_language_delete))
                                .on_action(cx.listener(Self::on_code_language_focus_content))
                                .on_action(cx.listener(Self::on_code_language_focus_next))
                                .on_action(cx.listener(Self::on_code_language_move_left))
                                .on_action(cx.listener(Self::on_code_language_move_right))
                                .on_action(cx.listener(Self::on_code_language_home))
                                .on_action(cx.listener(Self::on_code_language_end))
                                .on_action(cx.listener(Self::on_code_language_select_left))
                                .on_action(cx.listener(Self::on_code_language_select_right))
                                .on_action(cx.listener(Self::on_code_language_select_all))
                                .on_action(cx.listener(Self::on_code_language_copy))
                                .on_action(cx.listener(Self::on_code_language_cut))
                                .on_action(cx.listener(Self::on_code_language_paste))
                                .on_action(cx.listener(Self::on_code_language_indent))
                                .on_action(cx.listener(Self::on_code_language_outdent))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_code_language_mouse_down),
                                )
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(Self::on_code_language_mouse_up),
                                )
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(Self::on_code_language_mouse_up_out),
                                )
                                .on_mouse_move(cx.listener(Self::on_code_language_mouse_move))
                                .w(px(d.code_language_input_width))
                                .px(px(d.code_language_input_padding_x))
                                .py(px(d.code_language_input_padding_y))
                                .rounded(px(d.code_language_input_radius))
                                .border(px(d.code_language_input_border_width))
                                .border_color(c.code_language_input_border)
                                .bg(c.code_language_input_bg)
                                .text_size(px((t.code_size - 1.0).max(10.0)))
                                .text_color(c.code_language_input_text)
                                .cursor(CursorStyle::IBeam)
                                .child(CodeLanguageInputElement::new(
                                    cx.entity(),
                                    language_placeholder,
                                )),
                        ))
                        .into_any_element();
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .pl(px(depth_padding))
                        .pr(px(d.block_padding_x))
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(code_host)
                        .into_any_element()
                }
            }
            // FrontMatter 属性面板：底色跟主题走（forest 下即 code_bg 浅绿），
            // 不套代码块的任何装饰；key 列对齐并用主题 accent 高亮，值一律普通
            // 文本（列表值逗号连接），`---` 围栏隐藏。编辑态同底色内改 YAML
            // 原文（等宽）。行解析与 HTML 导出共用 [`frontmatter::parse_front_matter_rows`]。
            BlockKind::FrontMatter => {
                let raw = self
                    .record
                    .raw_fallback
                    .clone()
                    .unwrap_or_else(|| self.record.title.visible_text().to_string());
                let rows = crate::components::markdown::frontmatter::parse_front_matter_rows(
                    &crate::components::markdown::frontmatter::front_matter_body(&raw),
                );
                let card = focused_base
                    .w_full()
                    .bg(c.code_bg)
                    .rounded(px(10.0))
                    .px(px(14.0))
                    .py(px(10.0));
                if focused || rows.is_empty() {
                    card
                        .font(font(fonts.code_family.clone()))
                        .text_size(px(t.code_size))
                        .text_color(c.code_text)
                        .line_height(relative(1.5))
                        .child(
                            div().min_w(px(0.0)).w_full().overflow_hidden()
                                .child(BlockTextElement::new(cx.entity(), is_placeholder)),
                        )
                        .into_any_element()
                } else {
                    let text_size = px(t.text_size);
                    let mut rows_div = div().w_full().flex().flex_col().gap(px(8.0));
                    for (key, values) in &rows {
                        // 值一律普通正文：列表值逗号连接、自然折行，不做任何胶囊装饰。
                        let value_div = if values.is_empty() {
                            div().flex_grow(1.0).min_w(px(0.0))
                        } else {
                            div()
                                .flex_grow(1.0)
                                .min_w(px(0.0))
                                .text_size(text_size)
                                .text_color(c.text_default)
                                .child(SharedString::from(values.join(", ")))
                        };
                        rows_div = rows_div.child(
                            div()
                                .w_full()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .w(px(110.0))
                                        .flex_shrink_0()
                                        .text_size(text_size)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(c.text_link)
                                        .child(SharedString::from(key.clone())),
                                )
                                .child(value_div),
                        );
                    }
                    card.child(rows_div).into_any_element()
                }
            }
            BlockKind::Table => {
                let Some(runtime) = self.table_runtime.clone() else {
                    return focused_base
                        .text_size(px(t.text_size))
                        .text_color(c.text_default)
                        .line_height(relative(t.text_line_height))
                        .child(self.render_text_or_mixed_inline_visuals(
                            &theme,
                            focused,
                            is_placeholder,
                            None,
                            None,
                            c.text_default,
                            t.text_size,
                            FontWeight::NORMAL,
                            cx,
                        ))
                        .into_any_element();
                };

                let viewport_width = f32::from(window.viewport_size().width.max(px(1.0)));
                let activation_band = px(d.table_append_activation_band);
                let container_width = self
                    .table_container_width
                    .map(f32::from)
                    .unwrap_or_else(|| {
                        self.table_container_width_for_viewport(viewport_width, d, cx)
                    });
                let table_width = container_width.max(1.0);
                let column_layout = self
                    .cached_table_column_layout(table_width, &theme, window, cx)
                    .unwrap_or_else(|| TableColumnLayout::equal(runtime.header.len()));
                let preview_marker = self.table_axis_preview;
                let selected_marker = self.table_axis_selection;
                let body_row_count = runtime.rows.len();
                let column_count = runtime.header.len();
                let weak_table_block = cx.entity().downgrade();

                let header_cells = runtime.header;
                let header_hover_block = weak_table_block.clone();
                let header_select_block = weak_table_block.clone();
                let header_menu_block = weak_table_block.clone();
                // The header is visual row 0; its handle uses a more opaque
                // version of the body-row color to signal its distinct role.
                let header_marker = crate::components::TableAxisMarker {
                    kind: TableAxisKind::Row,
                    index: 0,
                };
                let header_band_bg = if selected_marker == Some(header_marker) {
                    header_axis_emphasis(c.table_axis_selected_bg)
                } else if preview_marker == Some(header_marker) {
                    header_axis_emphasis(c.table_axis_preview_bg)
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                };
                let header_row = div()
                    .relative()
                    .w_full()
                    .flex()
                    .gap(px(0.0))
                    .debug_selector(|| "table-header-row".into())
                    .child(
                        // Left-edge band mirrors the body rows so the header row
                        // can be hovered, selected, and right-clicked just like
                        // them, with the Header Row toggle added to its menu.
                        div()
                            .id(ElementId::Name(
                                format!("table-header-axis-band-{}", self.record.id).into(),
                            ))
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .w(px(6.0))
                            .debug_selector(|| "table-header-axis-band".into())
                            .rounded(px(6.0))
                            .bg(header_band_bg)
                            .cursor_pointer()
                            .on_hover(move |hovered, _window, cx| {
                                let _ = header_hover_block.update(cx, |_block, cx| {
                                    cx.emit(BlockEvent::RequestTableAxisPreview {
                                        kind: TableAxisKind::Row,
                                        index: 0,
                                        hovered: *hovered,
                                    });
                                });
                            })
                            .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                let _ = header_select_block.update(cx, |_block, cx| {
                                    cx.stop_propagation();
                                    cx.emit(BlockEvent::RequestSelectTableAxis {
                                        kind: TableAxisKind::Row,
                                        index: 0,
                                    });
                                });
                            })
                            .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                let _ = header_menu_block.update(cx, |_block, cx| {
                                    cx.stop_propagation();
                                    cx.emit(BlockEvent::RequestOpenTableAxisMenu {
                                        kind: TableAxisKind::Row,
                                        index: 0,
                                        position: event.position,
                                    });
                                });
                            })
                            .block_mouse_except_scroll(),
                    )
                    .children(header_cells.into_iter().enumerate().map(|(column, cell)| {
                        let corners = table_cell_corner_radii(
                            0, column, body_row_count + 1, column_count,
                        );
                        cell.update(cx, |block, cx| {
                            if block.table_cell_corner_radii != corners {
                                block.table_cell_corner_radii = corners;
                                cx.notify();
                            }
                        });
                        let hover_block = weak_table_block.clone();
                        let select_block = weak_table_block.clone();
                        let menu_block = weak_table_block.clone();
                        let cell_menu_block = weak_table_block.clone();
                        let marker = crate::components::TableAxisMarker {
                            kind: TableAxisKind::Column,
                            index: column,
                        };
                        let indicator = if selected_marker == Some(marker) {
                            Hsla { a: 0.8, ..c.table_cell_active_outline }
                        } else if preview_marker == Some(marker) {
                            Hsla { a: 0.4, ..c.table_cell_active_outline }
                        } else {
                            hsla(0.0, 0.0, 0.0, 0.0)
                        };
                        div()
                            .relative()
                            .flex_none()
                            .flex_basis(relative(column_layout.fraction(column)))
                            .w(relative(column_layout.fraction(column)))
                            .h_full()
                            .min_w(px(0.0))
                            .when(!self.is_read_only(), |this| {
                                let cell_menu_block = cell_menu_block.clone();
                                this.on_mouse_down(
                                    MouseButton::Right,
                                    move |event, _window, cx| {
                                        let _ = cell_menu_block.update(cx, |_block, cx| {
                                            cx.stop_propagation();
                                            cx.emit(BlockEvent::RequestOpenTableCellMenu {
                                                row: 0,
                                                column,
                                                position: event.position,
                                            });
                                        });
                                    },
                                )
                            })
                            .child(cell)
                            .child(
                                div()
                                    .id(ElementId::Name(
                                        format!(
                                            "table-column-axis-activation-{}-{}",
                                            self.record.id, column
                                        )
                                        .into(),
                                    ))
                                    .absolute()
                                    .top(-activation_band)
                                    .left_0()
                                    .right_0()
                                    .h(activation_band)
                                    .child(div().debug_selector(move || format!("table-column-indicator-{column}"))
                                        .absolute().top_0().left_0().right_0().h(px(2.0)).bg(indicator))
                                    .cursor_pointer()
                                    .on_hover(move |hovered, _window, cx| {
                                        let _ = hover_block.update(cx, |_block, cx| {
                                            cx.emit(BlockEvent::RequestTableAxisPreview {
                                                kind: TableAxisKind::Column,
                                                index: column,
                                                hovered: *hovered,
                                            });
                                        });
                                    })
                                    .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
                                        let _ = select_block.update(cx, |_block, cx| {
                                            cx.stop_propagation();
                                            cx.emit(BlockEvent::RequestSelectTableAxis {
                                                kind: TableAxisKind::Column,
                                                index: column,
                                            });
                                        });
                                    })
                                    .on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                                        let _ = menu_block.update(cx, |_block, cx| {
                                            cx.stop_propagation();
                                            cx.emit(BlockEvent::RequestOpenTableAxisMenu {
                                                kind: TableAxisKind::Column,
                                                index: column,
                                                position: event.position,
                                            });
                                        });
                                    })
                                    .block_mouse_except_scroll(),
                            )
                    }));

                let body_rows =
                    runtime
                        .rows
                        .into_iter()
                        .enumerate()
                        .map(|(body_row_index, row)| {
                            let hover_block = weak_table_block.clone();
                            let select_block = weak_table_block.clone();
                            let menu_block = weak_table_block.clone();
                            let cell_menu_block = weak_table_block.clone();
                            // Row selections are addressed by visual index, where
                            // the header is `0` and body rows follow at `1..`.
                            let visual_row = body_row_index + 1;
                            let marker = crate::components::TableAxisMarker {
                                kind: TableAxisKind::Row,
                                index: visual_row,
                            };
                            let band_bg = if selected_marker == Some(marker) {
                                c.table_axis_selected_bg
                            } else if preview_marker == Some(marker) {
                                c.table_axis_preview_bg
                            } else {
                                hsla(0.0, 0.0, 0.0, 0.0)
                            };
                            div()
                                .relative()
                                .w_full()
                                .flex()
                                .gap(px(0.0))
                                .debug_selector(|| "table-body-row".into())
                                .child(
                                    div()
                                        .id(ElementId::Name(
                                            format!(
                                                "table-row-axis-band-{}-{}",
                                                self.record.id, body_row_index
                                            )
                                            .into(),
                                        ))
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .left_0()
                                        .w(px(6.0))
                                        .debug_selector(|| "table-body-axis-band".into())
                                        .rounded(px(6.0))
                                        .bg(band_bg)
                                        .cursor_pointer()
                                        .on_hover(move |hovered, _window, cx| {
                                            let _ = hover_block.update(cx, |_block, cx| {
                                                cx.emit(BlockEvent::RequestTableAxisPreview {
                                                    kind: TableAxisKind::Row,
                                                    index: visual_row,
                                                    hovered: *hovered,
                                                });
                                            });
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            move |_event, _window, cx| {
                                                let _ = select_block.update(cx, |_block, cx| {
                                                    cx.stop_propagation();
                                                    cx.emit(BlockEvent::RequestSelectTableAxis {
                                                        kind: TableAxisKind::Row,
                                                        index: visual_row,
                                                    });
                                                });
                                            },
                                        )
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            move |event, _window, cx| {
                                                let _ = menu_block.update(cx, |_block, cx| {
                                                    cx.stop_propagation();
                                                    cx.emit(BlockEvent::RequestOpenTableAxisMenu {
                                                        kind: TableAxisKind::Row,
                                                        index: visual_row,
                                                        position: event.position,
                                                    });
                                                });
                                            },
                                        )
                                        .block_mouse_except_scroll(),
                                )
                                .children(row.into_iter().enumerate().map(|(column, cell)| {
                                    let corners = table_cell_corner_radii(
                                        visual_row, column, body_row_count + 1, column_count,
                                    );
                                    cell.update(cx, |block, cx| {
                                        if block.table_cell_corner_radii != corners {
                                            block.table_cell_corner_radii = corners;
                                            cx.notify();
                                        }
                                    });
                                    div()
                                        .flex_none()
                                        .flex_basis(relative(column_layout.fraction(column)))
                                        .w(relative(column_layout.fraction(column)))
                                        .h_full()
                                        .min_w(px(0.0))
                                        .when(!self.is_read_only(), |this| {
                                            let cell_menu_block = cell_menu_block.clone();
                                            this.on_mouse_down(
                                                MouseButton::Right,
                                                move |event, _window, cx| {
                                                    let _ =
                                                        cell_menu_block.update(cx, |_block, cx| {
                                                            cx.stop_propagation();
                                                            cx.emit(
                                                                BlockEvent::RequestOpenTableCellMenu {
                                                                    row: visual_row,
                                                                    column,
                                                                    position: event.position,
                                                                },
                                                            );
                                                        });
                                                },
                                            )
                                        })
                                        .child(cell)
                                }))
                        });

                {
                    let mut rows = Vec::with_capacity(1 + body_row_count);
                    rows.push(header_row.into_any_element());
                    rows.extend(body_rows.map(|row| row.into_any_element()));

                    let measure_block = cx.entity().downgrade();
                    // 顶部留出列轴激活条自己的窄条：条子贴在这里，不再盖住
                    // 表头文字上沿的选择热区。
                    let table_host = div()
                        .w(relative(VISUAL_BLOCK_WIDTH_RATIO))
                        .min_w(px(0.0))
                        .relative()
                        .flex()
                        .flex_col()
                        .gap(px(0.0))
                        .pt(activation_band)
                        .child(
                            div().w_full().flex().flex_col()
                                .shadow_sm()
                                .rounded(px(TABLE_CORNER_RADIUS))
                                .bg(c.table_cell_bg)
                                .children(rows),
                        )
                        .child(
                            canvas(move |bounds, _, cx| {
                                let width = bounds.size.width;
                                if let Err(error) = measure_block.update(cx, |block, cx| {
                                    block.table_host_bounds = Some(bounds);
                                    if width > px(0.0) && block.table_container_width != Some(width) {
                                        block.table_container_width = Some(width);
                                        cx.notify();
                                    }
                                }) {
                                    eprintln!("更新表格容器宽度失败：{error}");
                                }
                            }, |_, _, _, _| {})
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0(),
                        );
                    // 表格宿主不走 render_shell：那层 track_focus 会在按下时
                    // 抢走格子的焦点（自动聚焦外壳，输入就落空）。这里只补
                    // 一个只处理「表格宿主体外」的按下监听：点块自己的
                    // padding/缩进区时把焦点送进表格第一格。
                    div()
                        .id(block_id)
                        .debug_selector(|| "table-block-shell".to_string())
                        .w_full()
                        .min_w(px(0.0))
                        .pl(px(depth_padding))
                        .pr(px(d.block_padding_x))
                        .flex()
                        .flex_col()
                        .items_center()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::on_table_padding_mouse_down),
                        )
                        .child(table_host)
                        .into_any_element()
                }
            }
            BlockKind::HtmlBlock => {
                // 解析好的文档只按引用渲染：每帧深克隆整棵树（光标闪烁、悬停、
                // 选区变化都会重绘）要付一次 O(节点数) 的分配。只有还没有解析
                // 结果时才现场解析一份本地文档。
                let parsed;
                let html = match self.record.html.as_ref() {
                    Some(document) => document,
                    None => {
                        parsed = crate::components::parse_html_document(
                            self.record
                                .raw_fallback
                                .as_deref()
                                .unwrap_or_else(|| self.display_text()),
                        );
                        &parsed
                    }
                };
                focused_base
                    .text_size(px(t.text_size))
                    .text_color(c.text_default)
                    .line_height(relative(t.text_line_height))
                    .child(self.render_html_document(html, &theme, cx))
                    .into_any_element()
            }
            BlockKind::MathBlock => {
                let focused = focused && !self.is_read_only();
                if !focused {
                    self.last_layout = None;
                    self.last_bounds = None;
                }
                let child = if focused {
                    BlockTextElement::new(cx.entity(), is_placeholder).into_any_element()
                } else {
                    self.render_math_content(&theme, false, cx)
                };
                if !focused {
                    focused_base.w_full().child(child).into_any_element()
                } else {
                    // 聚焦编辑：整个块（含 $$ 定界行）铺公式卡片底色——底色只盖
                    // 公式行时，$$ 行像掉在块外。编辑器入口两个：
                    // 卡片右上角的 ƒx 小标记（与首行 `$$` 同一水平带），双击也可。
                    let card_padding_x = d.block_padding_x.max(8.0);
                    let card_padding_y = d.block_padding_y.max(6.0);
                    let fx_height = px(20.0);
                    // 块内文本的行高口径与 BlockTextElement 一致（数学块没有逐段
                    // 字号覆盖），标记要跟首行居中对齐就得用同一个值。
                    let first_line_height = f32::from(window.line_height()).max(f32::from(fx_height));
                    let fx_top = card_padding_y + (first_line_height - f32::from(fx_height)) / 2.0;
                    let fx_label: SharedString = cx
                        .global::<crate::i18n::I18nManager>()
                        .strings()
                        .insert_formula
                        .clone()
                        .into();
                    let fx_button = Button::new(ElementId::Name(
                        format!("math-fx-button-{}", self.record.id).into(),
                    ))
                    .debug_selector(|| "math-fx-button".to_string())
                    .ghost()
                    .small()
                    .compact()
                    .absolute()
                    .h(fx_height)
                    .px(px(6.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(c.dialog_border)
                    .text_size(px(t.text_size * 0.68))
                    .text_color(c.dialog_muted)
                    .accessibility_label(fx_label.clone())
                    .tooltip(fx_label)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|_block, _event: &ClickEvent, _window, cx| {
                        cx.emit(BlockEvent::RequestFormulaEditor);
                    }))
                    .child("ƒx");
                    focused_base
                        .w_full()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id("math-block-live-preview")
                                .debug_selector(|| "math-block-live-preview".to_string())
                                .w_full()
                                // 预览区贴着公式本身收缩：固定预留三行时，短公式与源码之间
                                // 会空出一条比公式还高的缝。超高公式仍在上限内滚动。
                                .max_h(px((t.text_size * t.text_line_height * 4.0).max(160.0)))
                                .flex_shrink_0()
                                .overflow_y_scroll()
                                .mb(px(card_padding_y))
                                .cursor(CursorStyle::Arrow)
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(self.render_math_content(&theme, true, cx)),
                        )
                        .child(
                            div()
                                .w_full()
                                .debug_selector(|| "math-block-card".to_string())
                                .relative()
                                .rounded(px(8.0))
                                .bg(c.code_bg)
                                .px(px(card_padding_x))
                                .py(px(card_padding_y))
                                .child(child)
                                .child(fx_button.top(px(fx_top)).right(px(card_padding_x))),
                        )
                        .into_any_element()
                }
            }
            BlockKind::MermaidBlock => {
                let focused = focused && !self.is_read_only();
                if !focused {
                    self.last_layout = None;
                    self.last_bounds = None;
                }
                let child = if focused {
                    BlockTextElement::new(cx.entity(), is_placeholder).into_any_element()
                } else {
                    self.render_mermaid_content(&theme, window, cx)
                };
                focused_base.w_full().child(child).into_any_element()
            }
            BlockKind::Paragraph if !self.toc_entries.is_empty() => {
                // 聚焦的目录块回到普通段落文本：块里得有文本元素，光标、输入
                // 与 IME 才有落脚点；失焦时才画成可点击的目录行（roadmap C2）。
                let text_size = t.text_size;
                if focused {
                    focused_base
                        .text_size(px(t.text_size))
                        .text_color(c.text_default)
                        .line_height(relative(t.text_line_height))
                        .child(self.render_text_or_mixed_inline_visuals(
                            &theme,
                            focused,
                            is_placeholder,
                            None,
                            None,
                            c.text_default,
                            t.text_size,
                            FontWeight::NORMAL,
                            cx,
                        ))
                        .into_any_element()
                } else {
                // `[TOC]` 占位段落渲染为可点击目录。条目按层级缩进，点击跳转到
                // 对应标题行。
                let rows: Vec<AnyElement> = self
                    .toc_entries
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let line = entry.line;
                        let indent = (entry.level.saturating_sub(1) as f32) * 14.0;
                        div()
                            .id(("toc-entry", index))
                            .debug_selector(move || format!("toc-entry-{index}"))
                            .pl(px(indent))
                            .cursor(CursorStyle::PointingHand)
                            .text_color(c.text_link)
                            .hover(|style| style.text_color(c.text_default))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |_block, _event, _window, cx| {
                                    cx.stop_propagation();
                                    cx.emit(BlockEvent::RequestJumpToHeadingLine { line });
                                }),
                            )
                            .child(SharedString::from(entry.title.clone()))
                            .into_any_element()
                    })
                    .collect();
                focused_base
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .text_size(px(text_size))
                    .line_height(relative(t.text_line_height))
                    .children(rows)
                    .into_any_element()
                }
            }
            BlockKind::Paragraph
            | BlockKind::Comment
            | BlockKind::RawMarkdown
            | BlockKind::Heading { .. } => focused_base
                .text_size(px(t.text_size))
                .text_color(c.text_default)
                .line_height(relative(t.text_line_height))
                .child(self.render_text_or_mixed_inline_visuals(
                    &theme,
                    focused,
                    is_placeholder,
                    None,
                    None,
                    c.text_default,
                    t.text_size,
                    FontWeight::NORMAL,
                    cx,
                ))
                .into_any_element(),
        };

        wrap_with_quote_guides(content, visible_quote_guides(self), self.record.id, &theme)
    }
}

