use super::*;

impl BlockTextElement {
    pub fn new(input: Entity<Block>, is_placeholder: bool) -> Self {
        Self {
            input,
            is_placeholder,
            placeholder_text: None,
            placeholder_color: None,
        }
    }

    pub fn with_placeholder(
        input: Entity<Block>,
        is_placeholder: bool,
        placeholder_text: SharedString,
        placeholder_color: Option<Hsla>,
    ) -> Self {
        Self {
            input,
            is_placeholder,
            placeholder_text: Some(placeholder_text),
            placeholder_color,
        }
    }
}

/// Prepared text layout and paint geometry for one `BlockTextElement` frame.
pub struct PrepaintState {
    lines: std::sync::Arc<Vec<WrappedLine>>,
    source_line_numbers: Vec<ShapedLine>,
    source_line_number_gutter_width: Pixels,
    /// 超长行的行号槽折叠指示符（▸ 折叠 / ▾ 展开），有超长行才有。
    long_line_chevron_collapsed: Option<ShapedLine>,
    long_line_chevron_expanded: Option<ShapedLine>,
    /// (源行下标, 是否展开)：paint 时按行顶画指示符。
    long_line_markers: Vec<(usize, bool)>,
    cursor: Option<PaintQuad>,
    /// 当前行高亮（光标所在视觉行全宽，最底层）。
    current_line: Vec<PaintQuad>,
    /// 括号匹配高亮（成对两个字符的小底色）。
    bracket_highlights: Vec<PaintQuad>,
    selection: Vec<PaintQuad>,
    code_backgrounds: Vec<PaintQuad>,
    search_highlights: Vec<PaintQuad>,
    line_height: Pixels,
    hitbox: Hitbox,
}

impl IntoElement for BlockTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BlockTextElement {
    type RequestLayoutState = Rc<RefCell<Option<std::sync::Arc<Vec<WrappedLine>>>>>;
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let theme = cx.global::<ThemeManager>().current_arc();
        let input = self.input.read(cx);
        let shared_text = input.shared_display_text();
        let is_placeholder = self.is_placeholder;
        let show_inline_code_backgrounds = !input.is_source_raw_mode();
        let show_source_line_numbers = input.show_source_line_numbers();
        let source_line_count = source_line_count(shared_text.as_ref());
        let wrap_prose = !is_placeholder
            && !input.is_source_raw_mode()
            && (input.kind() == BlockKind::Paragraph || input.kind().is_list_item());
        let code_ranges: Vec<_> = input.inline_spans().iter().filter(|span| span.style.code)
            .map(|span| span.range.clone()).collect();
        let space_prose = !is_placeholder && !input.is_source_raw_mode() && !input.kind().is_code_block();
        let style = window.text_style();

        let (display_text, text_color): (SharedString, Hsla) = if is_placeholder {
            (
                self.placeholder_text
                    .clone()
                    .unwrap_or_else(|| theme.placeholders.empty_editing.clone().into()),
                self.placeholder_color
                    .unwrap_or(theme.colors.text_placeholder),
            )
        } else {
            (shared_text, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size: None,
        };

        let runs: Vec<TextRun> = if !is_placeholder {
            // 代码块与带高亮语言的源码分块（markdown 源码）走高亮管线；
            // 其余按 markdown 行内样式上色。
            if input.kind().is_code_block() || input.code_highlight_result().is_some() {
                build_code_text_runs(
                    input,
                    &display_text,
                    &run,
                    px(theme.dimensions.underline_thickness),
                    &theme.colors,
                )
            } else {
                let fonts = crate::config::EditorSettings::scaled_fonts(cx);
                build_text_runs(
                    input,
                    &display_text,
                    &run,
                    px(theme.dimensions.underline_thickness),
                    theme.colors.text_link,
                    theme.colors.text_link,
                    show_inline_code_backgrounds,
                    &fonts.code_family,
                    px(fonts.code_size as f32),
                )
            }
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let letter_spacing = font_size * theme.typography.text_letter_spacing;
        let code_gap = px(theme.dimensions.code_bg_pad_x) + font_size * 0.125;
        let code_size = runs.iter().filter_map(|run| run.font_size).fold(px(0.0), Pixels::max);
        let line_height = window.line_height().max(code_size * 1.35);
        let source_line_start = input.source_line_start();
        // 栏宽按全文档基准算（整篇总行数），分块上下才右对齐；基准没挂上时
        // 退回块内口径。
        let gutter_width_basis = match input.source_line_gutter_basis() {
            0 => source_line_start + source_line_count,
            basis => basis,
        };
        let source_line_number_gutter_width = show_source_line_numbers
            .then(|| source_line_number_gutter_width(gutter_width_basis, font_size))
            .unwrap_or(px(0.0));

        // P3：shape 备忘键。任何影响 shape 结果的输入都进键：文本代数、
        // 换行宽、基准字号、字体（偏好 + 主题排版）、主题（run 颜色烘焙）。
        use std::hash::{Hash, Hasher};
        let mut fingerprint_hasher = std::hash::DefaultHasher::new();
        let font_prefs = crate::config::EditorSettings::scaled_fonts(cx);
        font_prefs.markdown_family.hash(&mut fingerprint_hasher);
        font_prefs.markdown_size.hash(&mut fingerprint_hasher);
        font_prefs.code_family.hash(&mut fingerprint_hasher);
        font_prefs.code_size.hash(&mut fingerprint_hasher);
        let style_font = style.font();
        style_font.family.hash(&mut fingerprint_hasher);
        style_font.weight.hash(&mut fingerprint_hasher);
        style_font.style.hash(&mut fingerprint_hasher);
        let font_fingerprint = fingerprint_hasher.finish();
        let theme_fingerprint =
            std::sync::Arc::as_ptr(&theme) as usize as u64;
        let memo_key_base = ShapeMemoKey {
            generation: input.display_generation(),
            wrap_width: None,
            wrap_prose,
            space_prose,
            font_size: f32::from(font_size).to_bits(),
            font_fingerprint,
            theme_fingerprint,
            long_line_wrap_generation: input.long_line_wrap_generation(),
        };
        let cached_memo = input.shape_memo_entry();
        let input_entity = self.input.clone();
        // 长行折叠计划（带行号的源码块才有）：决定布局宽度语义与按行 shape。
        let long_line_plan = if !is_placeholder && input.long_line_folding_enabled() {
            Some(input_entity.update(cx, |block, _block_cx| block.long_line_plan()))
        } else {
            None
        };

        let shared_lines: Rc<RefCell<Option<std::sync::Arc<Vec<WrappedLine>>>>> =
            Rc::new(RefCell::new(None));
        let shared_lines_clone = shared_lines.clone();

        let mut layout_style = Style::default();
        if long_line_plan.is_some() {
            // 长行不换行：块宽保持容器宽，超出部分由外层 overflow_hidden
            // 裁切显示（定版：不允许横向滚动）。
            layout_style.min_size.width = relative(1.).into();
        } else {
            layout_style.size.width = relative(1.).into();
            layout_style.min_size.width = px(0.0).into();
            layout_style.max_size.width = relative(1.).into();
        }

        let layout_id = window.request_measured_layout(
            layout_style,
            move |known_dimensions, available_space, window, closure_cx| {
                let wrap_width = known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(x) => Some(x),
                    AvailableSpace::MinContent => Some(px(1.0)),
                    AvailableSpace::MaxContent => Some(window.viewport_size().width.max(px(1.0))),
                });
                let text_wrap_width =
                    wrap_width.map(|width| (width - source_line_number_gutter_width).max(px(1.0)));

                let mut key = memo_key_base;
                key.wrap_width = text_wrap_width.map(|width| f32::from(width).to_bits());
                // P3：min-content 测量（≈1px 宽）会让整块文本按每字符一行
                // 病态换行（53KB → 数万行）。最小宽度对布局无意义（块宽由
                // centered_width 固定），直接返回保守下界，跳过 shape。
                if text_wrap_width.is_some_and(|width| width <= px(2.0)) {
                    let estimate_lines = display_text.split('\n').count().max(1) as f32;
                    return Size::new(
                        px(1.0),
                        px(estimate_lines * f32::from(line_height)),
                    );
                }
                if let Some(memo) = cached_memo.as_ref()
                    && memo.key == key
                {
                    // P3：键命中，跳过 build_text_runs 产物与 shape_text。
                    let lines = memo.lines.clone();
                    let mut total_size: Size<Pixels> = Size::default();
                    for line in lines.iter() {
                        let ls = line.size(line_height);
                        total_size.height += ls.height;
                        total_size.width = total_size.width.max(ls.width);
                    }
                    total_size.width += source_line_number_gutter_width;
                    *shared_lines_clone.borrow_mut() = Some(lines);
                    return total_size;
                }
                // 有超长行时按行 shape：折叠的超长行不换行（单行占一个
                // WrappedLine 条目，块高度不再爆炸，超出部分裁切显示），
                // 其余行照常按容器宽换行。没有超长行时保持整块一次 shape
                // 的原路径。
                let mut lines: Vec<WrappedLine> =
                    if let Some(plan) = long_line_plan
                        .as_ref()
                        .filter(|plan| !plan.long_lines.is_empty())
                    {
                        let expanded = input_entity.update(closure_cx, |block, _block_cx| {
                            block.expanded_long_lines.clone()
                        });
                        let mut all_lines = Vec::with_capacity(plan.ranges.len());
                        for (line_idx, range) in plan.ranges.iter().enumerate() {
                            let line_src = &display_text[range.clone()];
                            let collapsed_long =
                                plan.is_long(line_idx) && !expanded.contains(&line_idx);
                            let (shaped_text, line_runs, line_wrap_width) = if collapsed_long {
                                let char_count = line_src.chars().count();
                                let cut_end = if char_count
                                    > crate::components::LONG_LINE_DISPLAY_CHARS
                                {
                                    line_src
                                        .char_indices()
                                        .nth(crate::components::LONG_LINE_DISPLAY_CHARS)
                                        .map(|(offset, _)| offset)
                                        .unwrap_or(line_src.len())
                                } else {
                                    line_src.len()
                                };
                                let mut line_runs = slice_runs_for_range(
                                    &runs,
                                    range.start..range.start + cut_end,
                                );
                                let shaped_text = if cut_end < line_src.len() {
                                    let marker = format!(
                                        " ⋯⋯（本行共 {char_count} 字符，已截断；点行号展开）"
                                    );
                                    // 截断提示沿用行尾样式，保证 marker 字节有 run 覆盖，
                                    // 否则 shape_text 会把没有 run 的尾部直接丢掉。
                                    if let Some(last) = line_runs.last() {
                                        let mut marker_run = last.clone();
                                        marker_run.len = marker.len();
                                        line_runs.push(marker_run);
                                    }
                                    format!("{}{marker}", &line_src[..cut_end])
                                } else {
                                    line_src.to_string()
                                };
                                (shaped_text, line_runs, None)
                            } else {
                                (
                                    line_src.to_string(),
                                    slice_runs_for_range(&runs, range.clone()),
                                    text_wrap_width,
                                )
                            };
                            // 空源行 shape 不出条目会破坏「layout 行 ↔ 源行」
                            // 一一对应，用单个空格兜底（宽度贡献可忽略，索引
                            // 映射走原始行范围表，不受影响）。
                            let shaped_text = if shaped_text.is_empty() {
                                " ".to_string()
                            } else {
                                shaped_text
                            };
                            match window.text_system().shape_text(
                                shaped_text.into(),
                                font_size,
                                &line_runs,
                                line_wrap_width,
                                None,
                            ) {
                                Ok(line) => {
                                    debug_assert!(
                                        line.len() <= 1,
                                        "按行 shape 每个源行应恰好产出一条 WrappedLine"
                                    );
                                    all_lines.extend(line);
                                }
                                Err(_) => return Size::default(),
                            }
                        }
                        all_lines
                    } else {
                        match window.text_system().shape_text(
                            display_text.clone(),
                            font_size,
                            &runs,
                            text_wrap_width,
                            None,
                        ) {
                            Ok(lines) => lines.into_vec(),
                            Err(_) => return Size::default(),
                        }
                    };
                if space_prose {
                    add_render_spacing(
                        &mut lines,
                        &code_ranges,
                        letter_spacing,
                        code_gap,
                        font_size,
                    );
                }
                for line in lines.iter_mut() {
                    if wrap_prose || space_prose {
                        if line
                            .wrap_width
                            .is_some_and(|width| line.unwrapped_layout.width > width)
                        {
                            use unicode_segmentation::UnicodeSegmentation;
                            let emergency_breaks: Vec<_> = line.text.grapheme_indices(true)
                                .map(|(index, _)| index).collect();
                            line.wrap_at_boundaries(&prose_line_breaks(&line.text), &emergency_breaks);
                        }
                    }
                }
                let mut total_size: Size<Pixels> = Size::default();
                for line in lines.iter() {
                    let ls = line.size(line_height);
                    total_size.height += ls.height;
                    total_size.width = total_size.width.max(ls.width);
                }
                total_size.width += source_line_number_gutter_width;
                let lines = std::sync::Arc::new(lines);
                // 立即写入备忘：同一次布局内 taffy 可能多次 measure，
                // 迟写会让每次 measure 都重新 shape。
                input_entity.update(closure_cx, |block, _block_cx| {
                    block.set_shape_memo(ShapeMemoEntry {
                        key,
                        lines: lines.clone(),
                    });
                });
                *shared_lines_clone.borrow_mut() = Some(lines);
                total_size
            },
        );

        (layout_id, shared_lines)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = cx.global::<ThemeManager>().current_arc();
        let input = self.input.read(cx);
        let editor_selection_range = input
            .editor_selection_range
            .as_ref()
            .filter(|range| !range.is_empty())
            .cloned();
        let selected_range = editor_selection_range
            .clone()
            .unwrap_or_else(|| input.selected_range.clone());
        let cursor = input.cursor_offset();
        let code_size = if input.inline_spans().iter().any(|span| span.style.code) {
            px(crate::config::EditorSettings::scaled_font_sizes(cx).1)
        } else { px(0.0) };
        let line_height = window.line_height().max(code_size * 1.35);
        let focused = input.focus_handle.is_focused(window);
        let show_inline_code_backgrounds = !input.is_source_raw_mode();
        let show_source_line_numbers = input.show_source_line_numbers();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());

        let lines = request_layout.borrow_mut().take().unwrap_or_default();
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        // 与 request_layout 一致：栏宽按全文档基准（整篇总行数），分块对齐。
        let gutter_width_basis = match input.source_line_gutter_basis() {
            0 => lines.len().max(1) + input.source_line_start() - 1,
            basis => basis,
        };
        let source_line_number_gutter_width = show_source_line_numbers
            .then(|| source_line_number_gutter_width(gutter_width_basis, font_size))
            .unwrap_or(px(0.0));
        let text_bounds = source_text_bounds(bounds, source_line_number_gutter_width);
        let source_line_numbers = if show_source_line_numbers {
            let run_color = theme.colors.text_placeholder;
            let line_start = input.source_line_start();
            (line_start..line_start + lines.len().max(1))
                .map(|line_number| {
                    let label = line_number.to_string();
                    window.text_system().shape_line(
                        SharedString::from(label.clone()),
                        font_size,
                        &[TextRun {
                            len: label.len(),
                            font: style.font(),
                            color: run_color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                            font_size: None,
                        }],
                        None,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };

        // 超长行指示符（▸ 折叠 / ▾ 展开）：画在行号槽最左侧，点击行号切换。
        // 只有真的存在超长行时才 shape，常规块零开销。
        let has_long_lines = input
            .long_line_plan
            .as_ref()
            .is_some_and(|(_, plan)| !plan.long_lines.is_empty());
        let (long_line_chevron_collapsed, long_line_chevron_expanded, long_line_markers) =
            if has_long_lines {
                let chevron = |label: &'static str| {
                    window.text_system().shape_line(
                        SharedString::from(label),
                        font_size,
                        &[TextRun {
                            len: label.len(),
                            font: style.font(),
                            color: theme.colors.text_placeholder,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                            font_size: None,
                        }],
                        None,
                    )
                };
                let markers = input
                    .long_line_plan
                    .as_ref()
                    .map(|(_, plan)| {
                        plan.long_lines
                            .iter()
                            .map(|&line_idx| {
                                (line_idx, input.expanded_long_lines.contains(&line_idx))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (
                    Some(chevron("▸")),
                    Some(chevron("▾")),
                    markers,
                )
            } else {
                (None, None, Vec::new())
            };

        let cursor_opacity = input.cursor_opacity();
        let cursor_color = {
            let mut c = theme.colors.cursor;
            c.a *= cursor_opacity;
            c
        };
        let cursor_width = theme.dimensions.cursor_width;
        let selection_color = theme.colors.selection;
        let text_align = input.text_align();

        let (current_line_quads, selection_quads, bracket_quads, cursor_quad) =
            if (focused || editor_selection_range.is_some()) && !lines.is_empty() {
                if self.is_placeholder {
                    // Placeholder: cursor after the placeholder text
                    let layout = &lines[0];
                    let origin_x = aligned_line_left(layout, text_bounds, text_align);
                    let cursor_pos = layout
                        .position_for_index(0, line_height)
                        .unwrap_or_default();
                    (
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                        Some(fill(
                            Bounds::new(
                                point(origin_x + cursor_pos.x, text_bounds.top() + cursor_pos.y),
                                size(px(cursor_width), line_height),
                            ),
                            cursor_color,
                        )),
                    )
                } else if selected_range.is_empty() {
                    // No selection: current-line wash + bracket pair + cursor.
                    let text = input.display_text();
                    let cursor_bounds = cursor_bounds_for_offset(
                        &lines,
                        text_bounds,
                        line_height,
                        text,
                        cursor,
                        text_align,
                        px(cursor_width),
                    );
                    // 当前行高亮：光标所在视觉行全宽一条（无选区时才有意义）。
                    let current_line = cursor_bounds
                        .map(|bounds| {
                            fill(
                                Bounds::new(
                                    point(text_bounds.left(), bounds.origin.y),
                                    size(text_bounds.size.width, bounds.size.height),
                                ),
                                theme.colors.current_line_bg,
                            )
                        })
                        .into_iter()
                        .collect();
                    let bracket_highlights = matching_bracket_pair(&text, cursor)
                        .map(|(open_offset, close_offset)| {
                            [open_offset, close_offset]
                                .into_iter()
                                .filter_map(|offset| {
                                    cursor_bounds_for_offset(
                                        &lines,
                                        text_bounds,
                                        line_height,
                                        text,
                                        offset,
                                        text_align,
                                        px(cursor_width.max(1.0)),
                                    )
                                })
                                .map(|bounds| fill(bounds, theme.colors.matching_bracket_bg))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    (
                        current_line,
                        Vec::new(),
                        bracket_highlights,
                        cursor_bounds.map(|bounds| fill(bounds, cursor_color)),
                    )
                } else {
                    let text = input.display_text();
                    let quads = range_segment_bounds(
                        &lines,
                        text_bounds,
                        line_height,
                        text,
                        selected_range,
                        text_align,
                    )
                    .into_iter()
                    .map(|bounds| fill(bounds, selection_color))
                    .collect();
                    (Vec::new(), quads, Vec::new(), None)
                }
            } else {
                (Vec::new(), Vec::new(), Vec::new(), None)
            };

        // Compute code-span background quads with rounded corners and padding.
        let mut code_quads = Vec::new();
        if show_inline_code_backgrounds && !self.is_placeholder {
            let text = input.display_text();
            let hard_ranges = hard_line_ranges(text);
            let code_color = theme.colors.code_bg;
            let pad_x = px(theme.dimensions.code_bg_pad_x);
            let pad_y = px(theme.dimensions.code_bg_pad_y);
            let radius = px(theme.dimensions.code_bg_radius);
            for span in input.inline_spans() {
                if !span.style.code || span.range.is_empty() {
                    continue;
                }
                for segment in range_segment_bounds(
                    &lines,
                    text_bounds,
                    line_height,
                    text,
                    span.range.clone(),
                    text_align,
                ) {
                    let (line_idx, _) = wrapped_line_for_y(
                        &lines, line_height, segment.top() - text_bounds.top(),
                    ).expect("code fragment belongs to a shaped line");
                    let layout = &lines[line_idx];
                    let row_idx = ((segment.top() - text_bounds.top() - wrapped_line_top(&lines, line_height, line_idx)) / line_height) as usize;
                    let row_end = wrap_boundary_offset(layout, row_idx).unwrap_or(layout.len());
                    let end = span.range.end.saturating_sub(hard_ranges[line_idx].start).min(row_end);
                    let code_run = layout.runs().iter().find(|run| run.font_size.is_some())
                        .expect("inline code has a font-size run");
                    let code_size = code_run.font_size.unwrap();
                    let code_ascent = cx.text_system().ascent(code_run.font_id, code_size);
                    let code_descent = px(f32::from(cx.text_system().descent(code_run.font_id, code_size)).abs());
                    let baseline = (line_height - layout.ascent() - layout.descent()) / 2.0 + layout.ascent();
                    let mut quad_bounds = inline_code_background_bounds(
                        segment, baseline, code_ascent, code_descent, point(pad_x, pad_y),
                    );
                    quad_bounds.size.width -= layout.spacing_before(end);
                    code_quads.push({
                        let mut q = fill(quad_bounds, code_color);
                        q.corner_radii = Corners::all(radius);
                        q
                    });
                }
            }
        }

        // In-document search matches (roadmap B2): translucent quads under
        // the text, computed like selection segments. 活动命中用更深的
        // search_active_highlight_bg，循环跳转时能看出当前在哪一个。
        let mut search_highlights = Vec::new();
        {
            let highlight_color = theme.colors.search_highlight_bg;
            let active_color = theme.colors.search_active_highlight_bg;
            let active_range = input.search_active_range.clone();
            let text = input.display_text();
            for range in &input.search_highlight_ranges {
                let is_active = active_range.as_ref().is_some_and(|active| active == range);
                let color = if is_active { active_color } else { highlight_color };
                for segment in range_segment_bounds(
                    &lines,
                    text_bounds,
                    line_height,
                    text,
                    range.clone(),
                    text_align,
                ) {
                    let mut quad = fill(segment, color);
                    quad.corner_radii = Corners::all(px(2.0));
                    search_highlights.push(quad);
                }
            }
        }

        PrepaintState {
            lines,
            source_line_numbers,
            source_line_number_gutter_width,
            long_line_chevron_collapsed,
            long_line_chevron_expanded,
            long_line_markers,
            cursor: cursor_quad,
            current_line: current_line_quads,
            bracket_highlights: bracket_quads,
            selection: selection_quads,
            code_backgrounds: code_quads,
            search_highlights,
            line_height,
            hitbox,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, hovering_link) = {
            let input = self.input.read(cx);
            let text_bounds = source_text_bounds(bounds, prepaint.source_line_number_gutter_width);
            let hovering_link = !self.is_placeholder
                && !input.is_source_raw_mode()
                && prepaint.hitbox.is_hovered(window)
                && link_at_position(
                    input,
                    &prepaint.lines,
                    text_bounds,
                    prepaint.line_height,
                    window.mouse_position(),
                )
                .is_some();
            (input.focus_handle.clone(), hovering_link)
        };

        if hovering_link {
            // 悬停在链接上就显示小手，提示可点击；Cmd/Ctrl+点击仍由
            // `on_mouse_up` 负责跟随链接。
            window.set_cursor_style(CursorStyle::PointingHand, &prepaint.hitbox);
        }

        if focus_handle.is_focused(window) {
            let text_bounds = source_text_bounds(bounds, prepaint.source_line_number_gutter_width);
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(text_bounds, self.input.clone()),
                cx,
            );
        }

        // Paint code backgrounds behind text.
        for code_bg in prepaint.code_backgrounds.drain(..) {
            window.paint_quad(code_bg);
        }

        // Search matches sit above code backgrounds and below the selection.
        for highlight in prepaint.search_highlights.drain(..) {
            window.paint_quad(highlight);
        }

        // 当前行高亮垫底，选区叠上，括号匹配再上（都在文本之下）。
        for line_wash in prepaint.current_line.drain(..) {
            window.paint_quad(line_wash);
        }

        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }

        for bracket in prepaint.bracket_highlights.drain(..) {
            window.paint_quad(bracket);
        }

        let line_height = prepaint.line_height;
        let lines = std::mem::take(&mut prepaint.lines);
        let text_align = self.input.read(cx).text_align();
        let text_bounds = source_text_bounds(bounds, prepaint.source_line_number_gutter_width);
        let line_number_tops = source_line_number_tops(&lines, line_height);
        let line_number_gap = px(SOURCE_LINE_NUMBER_GAP);
        let line_numbers = std::mem::take(&mut prepaint.source_line_numbers);
        for (line_number, y_offset) in line_numbers.iter().zip(line_number_tops.iter()) {
            let line_number_width = line_number.x_for_index(line_number.len());
            line_number
                .paint(
                    point(
                        text_bounds.left() - line_number_gap - line_number_width,
                        bounds.origin.y + *y_offset,
                    ),
                    line_height,
                    window,
                    cx,
                )
                .ok();
        }

        // 超长行的折叠指示符：贴行号槽最左缘，指向可点。
        if !prepaint.long_line_markers.is_empty() {
            let gutter_left = text_bounds.left() - prepaint.source_line_number_gutter_width;
            for (line_idx, expanded) in &prepaint.long_line_markers {
                let Some(y_top) = line_number_tops.get(*line_idx) else {
                    continue;
                };
                let chevron = if *expanded {
                    prepaint.long_line_chevron_expanded.as_ref()
                } else {
                    prepaint.long_line_chevron_collapsed.as_ref()
                };
                if let Some(chevron) = chevron {
                    chevron
                        .paint(
                            point(gutter_left + px(2.0), bounds.origin.y + *y_top),
                            line_height,
                            window,
                            cx,
                        )
                        .ok();
                }
            }
        }

        let mut y_offset = Pixels::default();
        for line in lines.iter() {
            let origin_x = aligned_line_left(line, text_bounds, text_align);
            line.paint(
                point(origin_x, text_bounds.origin.y + y_offset),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
            y_offset += wrapped_line_height(line, line_height);
        }

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some((*lines).clone());
            input.last_bounds = Some(text_bounds);
            input.last_line_height = line_height;
            input.last_gutter_width = prepaint.source_line_number_gutter_width;
        });
    }
}
