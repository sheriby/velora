use super::*;

impl Block {
    pub(crate) fn on_html_details_toggle_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.html_details_open = !self.html_details_open;
        cx.stop_propagation();
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_image_content(
        &self,
        runtime: &ImageRuntime,
        max_width: Length,
        max_height: Pixels,
        placeholder_height: Pixels,
        resizable: bool,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let source = runtime.resolved_source.clone();
        let placeholder_theme = theme.clone();
        let loading_theme = theme.clone();
        let placeholder_strings = strings.clone();
        let loading_strings = strings.clone();
        let runtime_for_fallback = runtime.clone();
        let runtime_for_loading = runtime.clone();

        let image = match source {
            ImageResolvedSource::Local(path) => img(path),
            ImageResolvedSource::Remote(uri) => img(uri),
        }
        .max_w(max_width)
        .max_h(max_height)
        .object_fit(ObjectFit::Contain)
        .with_fallback(move || {
            render_image_placeholder(
                &runtime_for_fallback,
                max_width,
                placeholder_height,
                &placeholder_theme,
                &placeholder_strings,
            )
        })
        .with_loading(move || {
            render_loading_placeholder(
                &runtime_for_loading,
                max_width,
                placeholder_height,
                &loading_theme,
                &loading_strings,
            )
        });

        let image_host = if resizable {
            div()
                .relative()
                .child(image)
                .child(
                    div()
                        .id("image-resize-handle")
                        .absolute()
                        .right(px(-3.0))
                        .bottom(px(-3.0))
                        .size(px(14.0))
                        .rounded_full()
                        .border_1()
                        .border_color(c.dialog_border)
                        .bg(c.dialog_secondary_button_bg)
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .cursor(CursorStyle::ResizeLeftRight)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|block, event: &MouseDownEvent, _window, _cx| {
                                block.image_resize_drag = Some(crate::editor::ImageResizeDrag {
                                    start_x: f32::from(event.position.x),
                                    base_factor: block.image_width_factor,
                                });
                            }),
                        ),
                )
        } else {
            div().child(image)
        };

        let mut container = div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(d.image_caption_gap))
            .child(image_host);

        if let Some(title) = runtime
            .title
            .as_ref()
            .filter(|title| !title.trim().is_empty())
        {
            container = container.child(
                div()
                    .w_full()
                    .text_center()
                    .text_size(px(t.code_size))
                    .text_color(c.image_caption_text)
                    .child(SharedString::from(title.clone())),
            );
        }

        container.into_any_element()
    }

    pub(crate) fn render_math_content(&self, theme: &Theme) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let raw = self
            .record
            .raw_fallback
            .as_deref()
            .unwrap_or_else(|| self.display_text());

        let Some(source) = parse_display_math_source(raw) else {
            return div()
                .w_full()
                .text_size(px(t.text_size))
                .line_height(relative(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw.to_string()))
                .into_any_element();
        };

        match render_display_math_svg(&source, c.text_default, display_math_font_size(t.text_size))
        {
            Ok(rendered) => div()
                .w_full()
                .flex()
                .justify_center()
                .py(px(d.block_padding_y.max(6.0)))
                .child(
                    img(rendered.path)
                        .max_w(Length::Definite(relative(1.0)))
                        .max_h(px(d.image_root_max_height))
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element(),
            Err(err) => div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .rounded_sm()
                .bg(c.source_mode_block_bg)
                .px(px(d.block_padding_x))
                .py(px(d.block_padding_y))
                .text_size(px(t.text_size))
                .line_height(relative(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw.to_string()))
                .child(
                    div()
                        .text_size(px(t.code_size))
                        .text_color(c.dialog_muted)
                        .child(SharedString::from(format!("LaTeX render error: {err}"))),
                )
                .into_any_element(),
        }
    }

    pub(crate) fn render_mermaid_content(&self, theme: &Theme, window: &Window, cx: &App) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let raw = self
            .record
            .raw_fallback
            .as_deref()
            .unwrap_or_else(|| self.display_text());

        let Some(source) = parse_mermaid_fence_source(raw) else {
            return div()
                .w_full()
                .text_size(px(t.text_size))
                .line_height(relative(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw.to_string()))
                .into_any_element();
        };

        let viewport_width = f32::from(window.viewport_size().width.max(px(1.0)));
        let available_width = effective_image_width(self, viewport_width, d, cx);

        match render_mermaid_svg_for_display(&source, available_width, viewport_width) {
            Ok(rendered) => {
                let display_width = rendered.display_width.max(1.0);
                let display_height = rendered.display_height.max(1.0);
                let image_path = rendered.path.clone();
                let image = move || {
                    img(image_path.clone())
                        .w(px(display_width))
                        .h(px(display_height))
                };
                let content = if display_width <= available_width + 0.5 {
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(image())
                        .into_any_element()
                } else {
                    div()
                        .id(ElementId::Name(
                            format!("mermaid-scroll-{}", self.record.id).into(),
                        ))
                        .w_full()
                        .overflow_x_scroll()
                        .scrollbar_width(px(0.0))
                        .child(div().w(px(display_width)).child(image()))
                        .into_any_element()
                };

                div()
                    .w_full()
                    .py(px(d.block_padding_y.max(6.0)))
                    .child(content)
                    .into_any_element()
            }
            Err(err) => div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .rounded_sm()
                .bg(c.source_mode_block_bg)
                .px(px(d.block_padding_x))
                .py(px(d.block_padding_y))
                .text_size(px(t.text_size))
                .line_height(relative(t.text_line_height))
                .text_color(c.text_default)
                .child(SharedString::from(raw.to_string()))
                .child(
                    div()
                        .text_size(px(t.code_size))
                        .text_color(c.dialog_muted)
                        .child(SharedString::from(format!("Mermaid render error: {err}"))),
                )
                .into_any_element(),
        }
    }

    pub(crate) fn render_text_or_mixed_inline_visuals(
        &self,
        theme: &Theme,
        focused: bool,
        is_placeholder: bool,
        placeholder_text: Option<SharedString>,
        placeholder_color: Option<Hsla>,
        text_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 长块护栏（roadmap B12）：未聚焦时整块按源码文本渲染，跳过行内样式
        // 与混合可视元素的逐段布局；聚焦后仍走文本元素以保证可编辑。
        if !focused && !is_placeholder && self.exceeds_long_block_source_limit() {
            return div()
                .id("block-long-source")
                .debug_selector(|| "block-long-source".to_string())
                .w_full()
                .min_w(px(0.0))
                .text_size(px(font_size))
                .line_height(relative(theme.typography.text_line_height))
                .text_color(text_color)
                .child(SharedString::from(self.display_text().to_string()))
                .into_any_element();
        }

        // Mixed inline visuals are display-only. Once focused, the text element
        // takes over so caret movement, projection markers, and IME ranges stay
        // anchored to editable text rather than rendered SVG/script offsets.
        if focused || is_placeholder || !self.has_mixed_inline_visuals() {
            return match placeholder_text {
                Some(placeholder) => BlockTextElement::with_placeholder(
                    cx.entity(),
                    is_placeholder,
                    placeholder,
                    placeholder_color,
                )
                .into_any_element(),
                None => BlockTextElement::new(cx.entity(), is_placeholder).into_any_element(),
            };
        }

        self.render_mixed_inline_visual_runs(theme, text_color, font_size, font_weight, cx)
    }

    pub(crate) fn render_mixed_inline_visual_runs(
        &self,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_inline_tree_runs(
            &self.record.title,
            theme,
            base_color,
            font_size,
            font_weight,
            cx,
        )
    }

    pub(crate) fn render_inline_tree_runs(
        &self,
        tree: &crate::components::InlineTextTree,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(0.0))
            .text_size(px(font_size))
            .line_height(relative(theme.typography.text_line_height))
            .children(self.render_inline_tree_children(
                tree,
                theme,
                base_color,
                font_size,
                font_weight,
                cx,
            ))
            .into_any_element()
    }

    pub(crate) fn render_inline_tree_children(
        &self,
        tree: &crate::components::InlineTextTree,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let cache = tree.render_cache();
        let text = cache.visible_text();
        let mut children = Vec::new();
        let mut cursor = 0usize;

        for span in cache.spans() {
            if cursor < span.range.start {
                let fallback_span = crate::components::InlineSpan {
                    range: cursor..span.range.start,
                    style: crate::components::InlineStyle::default(),
                    html_style: None,
                    link: None,
                    footnote: None,
                    math: None,
                };
                children.extend(self.render_inline_text_word_segments(
                    &text[cursor..span.range.start],
                    &fallback_span,
                    theme,
                    base_color,
                    font_size,
                    font_weight,
                    cx,
                ));
            }

            let span_text = &text[span.range.clone()];
            if let Some(math) = span.math.as_ref() {
                children.push(
                    self.render_inline_math_segment(math, span, theme, base_color, font_size, cx),
                );
            } else {
                children.extend(self.render_inline_text_word_segments(
                    span_text,
                    span,
                    theme,
                    base_color,
                    font_size,
                    font_weight,
                    cx,
                ));
            }
            cursor = span.range.end;
        }

        if cursor < text.len() {
            let fallback_span = crate::components::InlineSpan {
                range: cursor..text.len(),
                style: crate::components::InlineStyle::default(),
                html_style: None,
                link: None,
                footnote: None,
                math: None,
            };
            children.extend(self.render_inline_text_word_segments(
                &text[cursor..],
                &fallback_span,
                theme,
                base_color,
                font_size,
                font_weight,
                cx,
            ));
        }

        children
    }

    /// Split a styled text run into wrap-friendly word segments. The mixed
    /// inline-visual layout is a `flex_wrap` row, so a long run rendered as one
    /// element wraps internally and claims the full row width, pushing the next
    /// item (inline math, a script, ...) onto its own line. Emitting one element
    /// per whitespace-delimited word lets the row break between words and keeps
    /// adjacent visuals on the same visual line. Inline code and background
    /// highlights stay a single element so their pill/background is continuous.
    /// `![alt](src)` fragments inside the run are promoted to inline image
    /// widgets, mirroring how table cells render embedded images.
    pub(crate) fn render_inline_text_word_segments(
        &self,
        text: &str,
        span: &crate::components::InlineSpan,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let has_background = span
            .html_style
            .is_some_and(|style| style.background_color.is_some());
        let mut segments = Vec::new();
        if promotes_inline_images(text, &span.style) {
            for segment in crate::components::markdown::image::parse_table_cell_inline_images(text)
            {
                match segment {
                    crate::components::markdown::image::TableCellInlineImageSegment::Text(
                        part,
                    ) => self.push_text_word_segments(
                        &part,
                        span,
                        theme,
                        base_color,
                        font_size,
                        font_weight,
                        has_background,
                        &mut segments,
                        cx,
                    ),
                    crate::components::markdown::image::TableCellInlineImageSegment::Image {
                        syntax,
                        ..
                    } => {
                        if let Some(runtime) = self.image_runtime_for_syntax(syntax.clone()) {
                            segments.push(self.render_inline_image_content(
                                &runtime,
                                theme,
                                &cx.global::<crate::i18n::I18nManager>().strings().clone(),
                            ));
                        } else {
                            self.push_text_word_segments(
                                &syntax.alt,
                                span,
                                theme,
                                base_color,
                                font_size,
                                font_weight,
                                has_background,
                                &mut segments,
                                cx,
                            );
                        }
                    }
                }
            }
            return segments;
        }
        self.push_text_word_segments(
            text,
            span,
            theme,
            base_color,
            font_size,
            font_weight,
            has_background,
            &mut segments,
            cx,
        );
        segments
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_text_word_segments(
        &self,
        text: &str,
        span: &crate::components::InlineSpan,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
        has_background: bool,
        segments: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) {
        if text.is_empty() {
            return;
        }
        for word in inline_word_chunks(text, span.style.code, has_background) {
            segments.push(self.render_inline_text_segment(
                word,
                span,
                theme,
                base_color,
                font_size,
                font_weight,
                cx,
            ));
        }
    }

    pub(crate) fn render_inline_text_segment(
        &self,
        text: &str,
        span: &crate::components::InlineSpan,
        theme: &Theme,
        base_color: Hsla,
        font_size: f32,
        font_weight: FontWeight,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if text.is_empty() {
            return div().into_any_element();
        }

        let mut color = if span.link.is_some() || span.footnote.is_some() {
            theme.colors.text_link
        } else if span.style.code {
            theme.colors.text_link
        } else {
            base_color
        };
        if let Some(style) = span.html_style
            && let Some(html_color) = style.color
        {
            color = html_css_color_to_hsla(html_color, color);
        }

        let script_offset = match span.style.script {
            InlineScript::Normal => 0.0,
            InlineScript::Superscript => -font_size * 0.28,
            InlineScript::Subscript => font_size * 0.22,
        };
        let display_font_size = inline_display_font_size(
            span,
            font_size,
            crate::config::EditorSettings::scaled_font_sizes(cx).1,
        );

        let mut element = div()
            .min_w(px(0.0))
            .text_size(px(display_font_size))
            .line_height(relative(theme.typography.text_line_height))
            .text_color(color)
            .font_weight(if span.style.bold {
                FontWeight::BOLD
            } else if span.style.code {
                if font_weight < FontWeight::MEDIUM { FontWeight::MEDIUM } else { font_weight }
            } else {
                font_weight
            })
            .child(SharedString::from(text.to_string()));

        if span.style.code {
            element = element.font(font(
                crate::config::EditorSettings::fonts(cx).code_family.clone(),
            ));
        }

        if script_offset != 0.0 {
            element = element.relative().top(px(script_offset));
        }

        // 悬停预览（roadmap C8/C9）：脚注显示脚注标签；链接显示目标与
        // 存在性。tooltip 闭包仅在悬停时执行，存在性检查零常驻开销。
        if let Some(footnote) = span.footnote.as_ref() {
            let footnote_id = footnote.id.clone();
            let strings = cx.global::<I18nManager>().strings().clone();
            let seg_id = ("hover-footnote", segment_hash(text, span.range.start));
            let tooltip_text =
                hover_preview_label(Some(&footnote_id), "", false, &strings);
            return element
                .id(seg_id)
                .tooltip(move |_, cx| {
                    cx.new(|_| HoverPreviewTooltip {
                        label: tooltip_text.clone().into(),
                    })
                    .into()
                })
                .into_any_element();
        } else if let Some(link) = span.link.as_ref() {
            let open_target = link.open_target.clone();
            let is_remote =
                open_target.starts_with("http://") || open_target.starts_with("https://");
            let strings = cx.global::<I18nManager>().strings().clone();
            let label =
                hover_preview_label(None, &open_target, is_remote, &strings);
            let seg_id = ("hover-link", segment_hash(text, span.range.start));
            return element
                .id(seg_id)
                .tooltip(move |_, cx| {
                    cx.new(|_| HoverPreviewTooltip { label: label.clone().into() }).into()
                })
                .into_any_element();
        }

        if span.style.underline || span.link.is_some() || span.footnote.is_some() {
            element = element.underline();
        }
        if span.style.code {
            element = element
                .rounded(px(theme.dimensions.code_bg_radius))
                .px(px(theme.dimensions.code_bg_pad_x))
                .py(px(theme.dimensions.code_bg_pad_y))
                .bg(theme.colors.code_bg);
        }
        // 标记文本 `==x==`：只铺底色，不加内边距，避免和同一行里的普通文字错位。
        if span.style.highlight {
            element = element.bg(theme.colors.comment_bg);
        }
        if let Some(style) = span.html_style
            && let Some(background) = style.background_color
        {
            element = element
                .rounded(px(3.0))
                .px(px(2.0))
                .bg(html_css_color_to_hsla(background, color));
        }

        // This run renders as plain (non-interactive) text, so a link inside a
        // mixed inline-visual block (alongside math or a script) would otherwise
        // have no way to be followed. Attach the open-link handlers directly to
        // the segment; they act only on Cmd/Ctrl+click so a plain click still
        // falls through and focuses the block for editing. The wrapper element
        // gates the hand cursor on that same modifier, matching the normal-text
        // path where links render through `BlockTextElement`.
        // `[[wikilink]]` 词渲染为链接样式，点击打开/创建工作区内同名文件
        // （roadmap C3）。
        if span.link.is_none()
            && !span.style.code
            && let Some(target) = wikilink_target(text)
        {
            let wikilink_color = theme.colors.text_link;
            let wikilink_element = element
                .text_color(wikilink_color)
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |block, _event, _window, _cx| {
                        block.wikilink_target = Some(target.clone());
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(
                        move |block, event: &gpui::MouseUpEvent, _window, cx| {
                            if event.click_count >= 1 {
                                let target = block.wikilink_target.take();
                                if let Some(target) = target {
                                    cx.emit(BlockEvent::RequestOpenWikilink { target });
                                }
                            }
                        },
                    ),
                )
                .into_any_element();
            return wikilink_element;
        }

        // `#tag` words render with link styling and open the workspace search
        // panel for that tag when clicked (roadmap C4).
        if span.link.is_none()
            && !span.style.code
            && let Some(query) = tag_query(text)
        {
            let tag_color = theme.colors.text_link;
            element = element.text_color(tag_color).cursor_pointer();
            return element
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |block, _event, _window, _cx| {
                        block.tag_query = Some(query.clone());
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(
                        move |block, event: &gpui::MouseUpEvent, _window, cx| {
                        if event.click_count >= 1 {
                            let query = block.tag_query.take();
                            if let Some(query) = query {
                                cx.emit(BlockEvent::RequestSearchTag { query });
                            }
                        }
                        },
                    ),
                )
                .into_any_element();
        }

        if let Some(link) = span.link.clone() {
            let element = element
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::on_rendered_link_mouse_down),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |block, event: &MouseUpEvent, _window, cx| {
                        if event.modifiers.secondary() {
                            block.open_rendered_link(&link, cx);
                        }
                    }),
                );
            return LinkFollowCursor {
                child: element.into_any_element(),
            }
            .into_any_element();
        }

        element.into_any_element()
    }
}
