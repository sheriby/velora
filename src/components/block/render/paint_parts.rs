use super::*;

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
        let code_language_focused = self.code_language_focus_handle.is_focused(window);
        let input_active = focused || code_language_focused;
        if self.sync_image_focus_state(focused) {
            cx.notify();
        }

        let showing_rendered_image = self.showing_rendered_image();
        // Inline math stays in the projected view while focused (its `$...$`
        // source shows as editable text), so links and other styling in the same
        // block keep their attributes instead of collapsing to raw Markdown, the
        // same way script spans already behave.
        self.sync_inline_projection_for_focus(focused && !showing_rendered_image);

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

        let mut theme = cx.global::<ThemeManager>().current_arc().as_ref().clone();
        // 文档块必须和编辑器外壳共用字号派生：块以前只套字号、漏掉界面缩放，
        // 导致缩放设置对正文完全没反应（用户报修）。
        crate::config::EditorSettings::apply_scaled_typography(cx, &mut theme);
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
            // The header row is only styled distinctly (shaded background, medium
            // weight) when the show-table-headers preference is enabled.
            let style_as_header =
                is_header && crate::config::EditorSettings::show_table_headers(cx);
            let highlight = self.table_axis_highlight;
            let base_bg = if style_as_header {
                c.table_header_bg
            } else {
                c.table_cell_bg
            };
            let (bg, border_color) = table_cell_colors(base_bg, highlight, focused, c);
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
                .bg(bg)
                .text_size(px(t.text_size))
                .text_color(c.text_default)
                .line_height(relative(t.text_line_height));

            let cell_base = if style_as_header {
                cell_base.font_weight(FontWeight::MEDIUM)
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
                        false,
                        &theme,
                        &strings,
                        cx,
                    ))
                    .into_any_element();
            }

            if !focused
                && let Some(inline_images) = self.render_table_cell_inline_images(
                    &theme,
                    &strings,
                    if style_as_header {
                        FontWeight::MEDIUM
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
                    c.text_default,
                    t.text_size,
                    if style_as_header {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    },
                    cx,
                ))
                .into_any_element();
        }

        // Source-mode rendering: raw text with no formatting.
        if self.is_source_raw_mode()
            && (focused
                || !matches!(
                    self.kind(),
                    BlockKind::HtmlBlock | BlockKind::MathBlock | BlockKind::MermaidBlock
                ))
        {
            if focused && self.cursor_blink_task.is_none() {
                self.start_cursor_blink(cx);
            } else if !focused && self.cursor_blink_task.is_some() {
                self.cursor_blink_task = None;
            }
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
                .text_size(px(if self.kind().is_code_block() {
                    t.code_size
                } else {
                    t.text_size
                }))
                .text_color(c.text_default)
                .line_height(relative(t.text_line_height));

            let source_base = if self.kind().is_code_block() {
                source_base.font(font(fonts.code_family.clone()))
            } else {
                source_base
            };

            let source_base = if self.kind().is_code_block() {
                source_base.font(font(fonts.code_family.clone()))
            } else if self.kind() == BlockKind::Comment {
                source_base.bg(c.comment_bg).rounded_sm()
            } else if focused && !self.show_source_line_numbers() {
                // 行号视图的块是 512 行一切的分块：聚焦底色会把整屏铺满
                // （用户报修：markdown 源码界面一大片绿）。光标定位有当前行
                // 高亮兜底，这里不再上块底色。
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

        // 空行块不渲染内容：空的文本元素自带一行高，会把 min_h 撑开（用户报修：
        // 松列表里的空行比正文一行还高）。
        if self.collapses_to_blank_gap(focused, false) {
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
            let viewport_width = f32::from(window.viewport_size().width.max(px(1.0)));
            // Root images stay within a readable default width; wider artwork
            // is downscaled instead of filling the entire text column. The
            // session drag factor scales it further (roadmap C10).
            let max_width = px(
                (effective_image_width(self, viewport_width, d, cx).min(d.image_root_max_width))
                    * self.image_width_factor,
            );
            if let Some(runtime) = self.image_runtime() {
                return focused_base
                    .child(self.render_image_content(
                        runtime,
                        max_width.into(),
                        px(d.image_root_max_height),
                        px(d.image_root_placeholder_height),
                        true,
                        &theme,
                        &strings,
                        cx,
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
                    div()
                        .min_w(px(d.list_marker_width))
                        .text_color(c.text_link)
                        .child(SharedString::new(bulleted_list_marker(self.render_depth))),
                    if showing_rendered_image {
                        // 列表项里的图片按「所在列的可用宽度」封顶（relative(1.0)）：
                        // 按视口估算量不到 callout/嵌套的真实容器，图片与占位框会
                        // 冲出右边界（用户报修）。
                        let max_width = relative(1.0);
                        if let Some(runtime) = self.image_runtime() {
                            div().flex_grow().child(self.render_image_content(
                                runtime,
                                max_width.into(),
                                px(d.image_root_max_height),
                                px(d.image_root_placeholder_height),
                                false,
                                &theme,
                                &strings,
                                cx,
                            ))
                        } else {
                            div().min_w(px(0.0)).flex_grow().child(
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
                        div().min_w(px(0.0)).flex_grow().child(
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
                let marker_width = d.list_marker_width.max(d.task_checkbox_size);
                let first_line_height = t.text_size * t.text_line_height;
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
                        div()
                            .min_w(px(marker_width))
                            .h(px(first_line_height))
                            .flex()
                            .items_center()
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
                                div().flex_grow().child(self.render_image_content(
                                    runtime,
                                    max_width.into(),
                                    px(d.image_root_max_height),
                                    px(d.image_root_placeholder_height),
                                    false,
                                    &theme,
                                    &strings,
                                    cx,
                                ))
                            } else {
                                div().min_w(px(0.0)).flex_grow().child(
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
                            div().min_w(px(0.0)).flex_grow().child(
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
                    div()
                        .min_w(px(d.ordered_list_marker_width))
                        .child(SharedString::from(numbered_list_marker(
                            self.render_depth,
                            self.list_ordinal.unwrap_or(1),
                            self.record.list_marker.delimiter_or_default(),
                        ))),
                    if showing_rendered_image {
                        // 列表项里的图片按「所在列的可用宽度」封顶（relative(1.0)）：
                        // 按视口估算量不到 callout/嵌套的真实容器，图片与占位框会
                        // 冲出右边界（用户报修）。
                        let max_width = relative(1.0);
                        if let Some(runtime) = self.image_runtime() {
                            div().flex_grow().child(self.render_image_content(
                                runtime,
                                max_width.into(),
                                px(d.image_root_max_height),
                                px(d.image_root_placeholder_height),
                                false,
                                &theme,
                                &strings,
                                cx,
                            ))
                        } else {
                            div().min_w(px(0.0)).flex_grow().child(
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
                        div().min_w(px(0.0)).flex_grow().child(
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
                        .flex_grow()
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
                            .flex_grow()
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
                let show_language_input = focused || code_language_focused;
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
                    div()
                        .w_full()
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
                            div()
                                .id("code-copy-button")
                                .debug_selector(|| "code-copy-button".to_string())
                                .absolute()
                                .right(px(d.code_block_padding_x))
                                .top(px(6.0))
                                .occlude()
                                .h(px(input_height))
                                .w(px(input_height))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.0))
                                .cursor(CursorStyle::PointingHand)
                                .hover(|style| style.bg(c.dialog_secondary_button_hover))
                                .tooltip(move |_, cx| {
                                    cx.new(|_| HoverPreviewTooltip { label: copy_tooltip.clone() }).into()
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::on_code_copy_button),
                                )
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
                            div().flex_grow().min_w(px(0.0))
                        } else {
                            div()
                                .flex_grow()
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
                let append_extent = px(d.table_append_button_extent);
                let append_inset = px(d.table_append_button_inset);
                let activation_band = px(d.table_append_activation_band);
                let column_control_visible = self.table_append_column_hovered;
                let row_control_visible = self.table_append_row_hovered;
                // 悬停出现的追加按钮会让行容器收窄（下方的 `pr(right_gutter)`），
                // 列宽必须按收窄后的宽度算，否则被钉在内容宽度上的列会在悬停时换行。
                let right_gutter = if column_control_visible {
                    append_extent + append_inset
                } else {
                    px(0.0)
                };
                let bottom_gutter = if row_control_visible {
                    append_extent + append_inset
                } else {
                    px(0.0)
                };
                let table_width = (effective_table_width(self, viewport_width, d, cx)
                    - f32::from(right_gutter))
                    .max(1.0);
                let column_layout = self
                    .cached_table_column_layout(table_width, &theme, window, cx)
                    .unwrap_or_else(|| TableColumnLayout::equal(runtime.header.len()));
                let preview_marker = self.table_axis_preview;
                let selected_marker = self.table_axis_selection;
                let body_row_count = runtime.rows.len();
                let column_append_top = activation_band;
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
                            .left(-activation_band)
                            .w(activation_band)
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
                        let hover_block = weak_table_block.clone();
                        let select_block = weak_table_block.clone();
                        let menu_block = weak_table_block.clone();
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
                                    .top_0()
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
                                        .left(-activation_band)
                                        .w(activation_band)
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
                                    div()
                                        .flex_none()
                                        .flex_basis(relative(column_layout.fraction(column)))
                                        .w(relative(column_layout.fraction(column)))
                                        .h_full()
                                        .min_w(px(0.0))
                                        .child(cell)
                                }))
                        });

                {
                    let mut rows = Vec::with_capacity(1 + body_row_count);
                    rows.push(header_row.into_any_element());
                    rows.extend(body_rows.map(|row| row.into_any_element()));

                    let column_edge_band = div()
                        .id(ElementId::Name(
                            format!("table-append-column-edge-{}", self.record.id).into(),
                        ))
                        .absolute()
                        .top(column_append_top)
                        .bottom(bottom_gutter)
                        .right(right_gutter)
                        .w(activation_band)
                        .on_hover(cx.listener(Self::on_table_append_column_edge_hover));

                    let row_edge_band = div()
                        .id(ElementId::Name(
                            format!("table-append-row-edge-{}", self.record.id).into(),
                        ))
                        .absolute()
                        .left_0()
                        .right(right_gutter)
                        .bottom(bottom_gutter)
                        .h(activation_band)
                        .on_hover(cx.listener(Self::on_table_append_row_edge_hover));

                    let column_control = {
                        let base = div()
                            .id(ElementId::Name(
                                format!("table-append-column-zone-{}", self.record.id).into(),
                            ))
                            .absolute()
                            .top(column_append_top)
                            .bottom(bottom_gutter)
                            .right_0()
                            .w(right_gutter)
                            .on_hover(cx.listener(Self::on_table_append_column_zone_hover));

                        if column_control_visible {
                            base.child(
                                div()
                                    .id(ElementId::Name(
                                        format!("table-append-column-button-{}", self.record.id)
                                            .into(),
                                    ))
                                    .absolute()
                                    .top(append_inset)
                                    .bottom_0()
                                    .right_0()
                                    .w(append_extent)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(999.0))
                                    .bg(c.table_append_button_bg)
                                    .hover(|this| this.bg(c.table_append_button_hover))
                                    .cursor_pointer()
                                    .text_size(px(t.text_size))
                                    .text_color(c.table_append_button_text)
                                    .block_mouse_except_scroll()
                                    .on_hover(
                                        cx.listener(Self::on_table_append_column_button_hover),
                                    )
                                    .on_click(cx.listener(Self::on_append_table_column))
                                    .child("+"),
                            )
                        } else {
                            base
                        }
                    };

                    let row_control = {
                        let base = div()
                            .id(ElementId::Name(
                                format!("table-append-row-zone-{}", self.record.id).into(),
                            ))
                            .absolute()
                            .left_0()
                            .right(right_gutter)
                            .bottom_0()
                            .h(bottom_gutter)
                            .on_hover(cx.listener(Self::on_table_append_row_zone_hover));

                        if row_control_visible {
                            base.child(
                                div()
                                    .id(ElementId::Name(
                                        format!("table-append-row-button-{}", self.record.id)
                                            .into(),
                                    ))
                                    .absolute()
                                    .left(append_inset)
                                    .right(append_inset)
                                    .bottom_0()
                                    .h(append_extent)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(999.0))
                                    .bg(c.table_append_button_bg)
                                    .hover(|this| this.bg(c.table_append_button_hover))
                                    .cursor_pointer()
                                    .text_size(px(t.text_size))
                                    .text_color(c.table_append_button_text)
                                    .block_mouse_except_scroll()
                                    .on_hover(cx.listener(Self::on_table_append_row_button_hover))
                                    .on_click(cx.listener(Self::on_append_table_row))
                                    .child("+"),
                            )
                        } else {
                            base
                        }
                    };

                    div()
                        .id(block_id)
                        .w_full()
                        .relative()
                        .flex()
                        .flex_col()
                        .pr(right_gutter)
                        .pb(bottom_gutter)
                        .gap(px(0.0))
                        .child(
                            div().w_full().flex().flex_col()
                                .shadow_sm()
                                .bg(c.table_cell_bg)
                                .children(rows),
                        )
                        .child(column_edge_band)
                        .child(row_edge_band)
                        .child(column_control)
                        .child(row_control)
                        .into_any_element()
                }
            }
            BlockKind::HtmlBlock => {
                let html = self.record.html.as_ref().cloned().unwrap_or_else(|| {
                    crate::components::parse_html_document(
                        self.record
                            .raw_fallback
                            .as_deref()
                            .unwrap_or_else(|| self.display_text()),
                    )
                });
                focused_base
                    .text_size(px(t.text_size))
                    .text_color(c.text_default)
                    .line_height(relative(t.text_line_height))
                    .child(self.render_html_document(&html, &theme, cx))
                    .into_any_element()
            }
            BlockKind::MathBlock => {
                if !focused {
                    self.last_layout = None;
                    self.last_bounds = None;
                }
                let child = if focused {
                    BlockTextElement::new(cx.entity(), is_placeholder).into_any_element()
                } else {
                    self.render_math_content(&theme)
                };
                focused_base.w_full().child(child).into_any_element()
            }
            BlockKind::MermaidBlock => {
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
                // `[TOC]` 占位段落渲染为可点击目录（roadmap C2）。条目按层级
                // 缩进，点击跳转到对应标题行。
                let text_size = t.text_size;
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

        wrap_with_quote_guides(content, visible_quote_guides(self), &theme)
    }
}

