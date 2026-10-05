    use super::{
        link_at_position, source_line_number_gutter_width, source_line_number_tops,
        source_text_bounds, wrapped_line_height,
    };
    use crate::components::{Block, BlockKind, BlockRecord, InlineTextTree, TableCellPosition};
    use gpui::{
        AppContext, Bounds, Hsla, Modifiers, MouseButton, MouseDownEvent, SharedString,
        TestAppContext, TextAlign, TextRun, VisualTestContext, font, point, px, rgba, size,
    };

    #[test]
    fn prose_wrapping_preserves_punctuation_and_graphemes() {
        use unicode_segmentation::UnicodeSegmentation;
        let text = "alpha! commas, periods. questions? semicolons; colons: (closing) [bracket] {brace} 中文（右括号）👩‍💻e\u{301} words";
        let breaks = super::prose_line_breaks(text);
        let graphemes: Vec<_> = text.grapheme_indices(true).map(|(ix, _)| ix).collect();
        assert!(!breaks.contains(&1), "普通英文单词内部不应断行");
        for ix in breaks.into_iter().filter(|ix| *ix > 0 && *ix < text.len()) {
            assert!(graphemes.contains(&ix), "不能拆开 emoji 或组合字符");
            let first = text[ix..].chars().next().unwrap();
            assert!(
                !"!！,，.。?？;；:：)]}）】”’".contains(first),
                "成熟断行器也必须应用严格标点禁则：{first:?}"
            );
        }
    }

    #[test]
    fn wrapped_inline_code_backgrounds_leave_space_between_rows() {
        let row = Bounds::new(point(px(0.0), px(0.0)), size(px(160.0), px(28.0)));
        let next_row = Bounds::new(point(px(0.0), px(28.0)), row.size);
        let first = super::inline_code_background_bounds(row, px(22.0), px(13.0), px(4.0), point(px(2.0), px(1.0)));
        let second = super::inline_code_background_bounds(next_row, px(22.0), px(13.0), px(4.0), point(px(2.0), px(1.0)));
        assert!(first.bottom() < second.top(), "多行代码背景不能粘连");
        assert!(first.size.height >= px(17.0), "背景必须覆盖代码字形");
        assert!(first.size.height < row.size.height, "背景不能使用整行高度");
    }

    #[test]
    fn prose_spacing_keeps_code_fixed_and_adds_margins_and_autospace() {
        let text = "中文hello文 abc";
        let code = "中文".len().."中文hello".len();
        let spacing = super::prose_spacing(text, &[code.clone()], px(0.25), px(4.0), px(4.0));
        assert!(spacing.contains(&(code.start, px(8.0))), "代码前应保留 margin/padding 和中西间距");
        assert!(spacing.contains(&(code.end, px(8.0))), "代码后应保留 margin/padding 和中西间距");
        assert!(!spacing.iter().any(|(index, _)| code.start < *index && *index < code.end), "代码内部仍须等宽");
        assert!(spacing.contains(&(text.len() - 1, px(0.25))), "正文应有独立字距");
    }

    #[gpui::test]
    async fn prose_spacing_keeps_text_indices_and_hit_testing(cx: &mut TestAppContext) {
        use unicode_segmentation::UnicodeSegmentation;
        let cx = cx.add_empty_window();
        let text = "中文hello世界 abc continuation";
        let mut lines = shaped_lines(text, px(130.0), cx);
        let indices: Vec<_> = lines[0].runs().iter().flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index)).collect();
        let spacing = super::prose_spacing(text, &[], px(0.3), px(4.0), px(4.0));
        lines[0].add_horizontal_spacing(&spacing);
        let emergency: Vec<_> = text.grapheme_indices(true).map(|(index, _)| index).collect();
        lines[0].wrap_at_boundaries(&super::prose_line_breaks(text), &emergency);
        assert_eq!(lines[0].text.as_ref(), text, "排版不应写入空格");
        assert_eq!(indices, lines[0].runs().iter().flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index)).collect::<Vec<_>>());
        for index in indices {
            if lines[0].wrap_boundaries().iter().any(|boundary| lines[0].runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index == index) { continue; }
            let position = lines[0].position_for_index(index, px(28.0)).unwrap();
            assert_eq!(lines[0].closest_index_for_position(position, px(28.0)).unwrap(), index, "字距变化后点击位置必须仍匹配原文");
        }
    }

    fn shaped_lines(
        text: &str,
        width: gpui::Pixels,
        cx: &mut VisualTestContext,
    ) -> Vec<gpui::WrappedLine> {
        cx.update(|window, _app| {
            window
                .text_system()
                .shape_text(
                    text.to_string().into(),
                    px(16.0),
                    &[TextRun {
                        len: text.len(),
                        font: font(".SystemUIFont"),
                        color: Hsla::from(rgba(0xffffffff)),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                        font_size: None,
                    }],
                    Some(width),
                    None,
                )
                .expect("text should shape")
                .into_vec()
        })
    }

    #[test]
    fn source_line_number_gutter_grows_with_digit_count() {
        let one_digit = source_line_number_gutter_width(9, px(16.0));
        let two_digits = source_line_number_gutter_width(10, px(16.0));
        let three_digits = source_line_number_gutter_width(100, px(16.0));

        assert_eq!(one_digit, two_digits);
        assert!(three_digits > two_digits);
    }

    #[test]
    fn source_text_bounds_are_offset_by_gutter_width() {
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(300.0), px(120.0)));
        let text_bounds = source_text_bounds(bounds, px(48.0));

        assert_eq!(text_bounds.left(), px(58.0));
        assert_eq!(text_bounds.top(), px(20.0));
        assert_eq!(text_bounds.size.width, px(252.0));
        assert_eq!(text_bounds.size.height, px(120.0));
    }

    #[gpui::test]
    async fn source_line_number_tops_follow_soft_wrapped_hard_lines(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let lines = shaped_lines(
            "this line should wrap before the next hard line\nsecond",
            px(92.0),
            cx,
        );
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "first hard line should soft-wrap"
        );

        let tops = source_line_number_tops(&lines, px(20.0));
        assert_eq!(tops[0], px(0.0));
        assert_eq!(tops[1], wrapped_line_height(&lines[0], px(20.0)));
    }

    #[gpui::test]
    async fn link_hit_matches_only_rendered_link_text(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[link](https://example.com)"),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(320.0), cx);
        let (hit, miss_right) = block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let layout = &lines[0];
            let start = layout
                .position_for_index(span.range.start, px(20.0))
                .expect("start position");
            let end = layout
                .position_for_index(span.range.end, px(20.0))
                .expect("end position");
            let hit = point((start.x + end.x) / 2.0, px(10.0));
            let miss_right = point(end.x + px(24.0), px(10.0));
            (
                link_at_position(block, &lines, bounds, px(20.0), hit)
                    .map(|link| link.open_target.clone()),
                link_at_position(block, &lines, bounds, px(20.0), miss_right)
                    .map(|link| link.open_target.clone()),
            )
        });

        assert_eq!(hit, Some("https://example.com".to_string()));
        assert_eq!(miss_right, None);
    }

    #[gpui::test]
    async fn secondary_click_follows_link_while_plain_click_edits(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("a [link](https://example.com) bbbb"),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(320.0), cx);
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(320.0), px(20.0)));

        let link_position = block.read_with(cx, |block, _cx| {
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let layout = &lines[0];
            let start = layout
                .position_for_index(span.range.start, px(20.0))
                .expect("start position");
            let end = layout
                .position_for_index(span.range.end, px(20.0))
                .expect("end position");
            point((start.x + end.x) / 2.0, px(10.0))
        });

        block.update(cx, |block, _cx| {
            block.last_layout = Some(lines.clone());
            block.last_bounds = Some(bounds);
            block.last_line_height = px(20.0);
            block.selected_range = 0..0;
        });

        let mut event = MouseDownEvent {
            button: MouseButton::Left,
            position: link_position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        };

        // A plain click on the link moves the caret into the text for editing.
        cx.update(|window, app| {
            block.update(app, |block, cx| block.on_mouse_down(&event, window, cx));
        });
        block.read_with(cx, |block, _cx| {
            assert_ne!(block.selected_range, 0..0);
        });

        // Cmd/Ctrl+click follows the link instead: the caret is left untouched
        // and no drag-selection begins.
        block.update(cx, |block, _cx| block.selected_range = 0..0);
        event.modifiers = Modifiers::secondary_key();
        cx.update(|window, app| {
            block.update(app, |block, cx| block.on_mouse_down(&event, window, cx));
        });
        block.read_with(cx, |block, _cx| {
            assert_eq!(block.selected_range, 0..0);
            assert!(!block.is_selecting);
        });
    }

    #[gpui::test]
    async fn link_hit_respects_center_alignment(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            let mut block = Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("[link](https://example.com)"),
                ),
            );
            block.set_table_cell_mode(
                TableCellPosition { row: 0, column: 0 },
                crate::components::TableColumnAlignment::Center,
            );
            block
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(240.0), cx);
        let (miss_left, hit_center) = block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(240.0), px(20.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let layout = &lines[0];
            let origin_x = super::aligned_line_left(layout, bounds, block.text_align());
            let start = layout
                .position_for_index(span.range.start, px(20.0))
                .expect("start position");
            let end = layout
                .position_for_index(span.range.end, px(20.0))
                .expect("end position");
            let miss_left = point(origin_x - px(12.0), px(10.0));
            let hit_center = point(origin_x + (start.x + end.x) / 2.0, px(10.0));
            (
                link_at_position(block, &lines, bounds, px(20.0), miss_left)
                    .map(|link| link.open_target.clone()),
                link_at_position(block, &lines, bounds, px(20.0), hit_center)
                    .map(|link| link.open_target.clone()),
            )
        });

        assert_eq!(miss_left, None);
        assert_eq!(hit_center, Some("https://example.com".to_string()));
    }

    #[gpui::test]
    async fn text_runs_apply_inline_html_color_and_background(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown(
                        "before <span style='color:blue;background-color:#ff0'>marked</span>",
                    ),
                ),
            )
        });

        block.read_with(cx, |block, _cx| {
            let display_text: SharedString = block.display_text().to_string().into();
            let base_run = TextRun {
                len: display_text.len(),
                font: font(".SystemUIFont"),
                color: Hsla::from(rgba(0xffffffff)),
                background_color: None,
                underline: None,
                strikethrough: None,
                font_size: None,
            };
            let runs = super::build_text_runs(
                block,
                &display_text,
                &base_run,
                px(1.0),
                Hsla::from(rgba(0x0066ccff)),
                Hsla::from(rgba(0x111111ff)),
                true,
                "Menlo",
                px(13.0),
                Hsla::from(rgba(0xfff4ce99)),
            );
            let marked_run = runs.last().expect("styled text should create a final run");

            assert_eq!(block.display_text(), "before marked");
            assert_eq!(marked_run.len, "marked".len());
            assert_eq!(marked_run.color, Hsla::from(rgba(0x0000ffff)));
            assert_eq!(
                marked_run.background_color,
                Some(Hsla::from(rgba(0xffff00ff)))
            );
        });
    }

    #[gpui::test]
    async fn inline_code_runs_use_the_code_font_size(cx: &mut TestAppContext) {
        // 用户报修：点击含行内代码的行，代码字号跳回正文。这要求可编辑文本
        // 也带上逐段字号（vendored gpui 本地补丁），且行内代码跟随
        // 「代码块字体大小」设置——显示态（分段路径）与编辑态（本路径）同值。
        let cx = cx.add_empty_window();
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown("plain `code` tail"),
                ),
            )
        });

        cx.update(|window, app| {
            let display_text: SharedString = block.read(app).display_text().to_string().into();
            let base_run = TextRun {
                len: display_text.len(),
                font: font(".SystemUIFont"),
                color: Hsla::from(rgba(0xffffffff)),
                background_color: None,
                underline: None,
                strikethrough: None,
                font_size: None,
            };
            let runs = block.read_with(app, |block, _| {
                super::build_text_runs(
                    block,
                    &display_text,
                    &base_run,
                    px(1.0),
                    Hsla::from(rgba(0x0066ccff)),
                    Hsla::from(rgba(0x111111ff)),
                    true,
                    "Menlo",
                    px(13.0),
                    Hsla::from(rgba(0xfff4ce99)),
                )
            });

            let code_run = runs
                .iter()
                .find(|run| run.font_size.is_some())
                .expect("行内代码段应带字号覆盖");
            assert_eq!(code_run.len, "code".len());
            assert_eq!(code_run.font_size, Some(px(13.0)));
            assert!(
                runs.iter().filter(|run| run.font_size.is_none()).count() >= 2,
                "代码段两侧的正文段不带字号覆盖"
            );

            let lines = window
                .text_system()
                .shape_text(display_text.clone(), px(16.0), &runs, None, None)
                .expect("text should shape");
            let layout = &lines[0];
            let code_shaped = layout
                .runs()
                .iter()
                .find(|run| run.font_size == Some(px(13.0)))
                .expect("整形结果应保留代码段字号");
            let body_shaped = layout
                .runs()
                .iter()
                .find(|run| run.font_size.is_none())
                .expect("整形结果应保留正文段");

            let advance = |run: &gpui::ShapedRun| {
                let first = run.glyphs.first().expect("run should have glyphs");
                let last = run.glyphs.last().expect("run should have glyphs");
                (last.position.x - first.position.x) / (run.glyphs.len().max(2) - 1) as f32
            };
            assert!(
                advance(code_shaped) < advance(body_shaped),
                "代码段步进 {:?} 应小于正文段 {:?}",
                advance(code_shaped),
                advance(body_shaped)
            );
        });
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_platform_text_system_applies_per_run_font_size() {
        // 真实 CoreText 的逐段字号：步进必须按字号缩放，且 ShapedRun 要带上字号
        // 供绘制取用（测试平台的 NoopTextSystem 只是模拟这条契约）。
        let run_with = |len: usize, family: &'static str, font_size: Option<gpui::Pixels>| TextRun {
            len,
            font: font(family),
            color: Hsla::from(rgba(0xffffffff)),
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size,
        };
        let text = "plain code";
        let runs = [
            run_with("plain ".len(), "Menlo", None),
            run_with("code".len(), "Menlo", Some(px(13.0))),
        ];
        let layout = gpui::shape_line_with_platform_text_system(text.into(), px(16.0), &runs);

        let advance = |run: &gpui::ShapedRun| {
            let first = run.glyphs.first().expect("run should have glyphs");
            let last = run.glyphs.last().expect("run should have glyphs");
            (last.position.x - first.position.x) / (run.glyphs.len().max(2) - 1) as f32
        };
        let code_run = layout
            .runs
            .iter()
            .find(|run| run.font_size == Some(px(13.0)) && !run.glyphs.is_empty())
            .expect("CoreText 排版结果应保留逐段字号");
        assert_eq!(code_run.glyphs.len(), "code".len());
        let body_run = layout
            .runs
            .iter()
            .find(|run| run.font_size.is_none() && !run.glyphs.is_empty())
            .expect("正文段不带字号覆盖");
        assert!(
            advance(code_run) < advance(body_run) * 0.9,
            "13px 代码段步进 {:?} 应明显小于 16px 正文段步进 {:?}",
            advance(code_run),
            advance(body_run)
        );
    }

    #[test]
    fn vendored_text_system_keeps_the_per_run_font_size_patch() {
        // 行内代码跟随「代码块字体大小」依赖 vendored gpui 的本地补丁：逐段字号
        // 必须一路走到各平台排版层。mac 这条路径需要 Metal 工具链才能编译测试
        // （本机没有），这里守住补丁本身，升级 vendored 副本时不会被悄悄覆盖。
        let mac = include_str!("../../../../vendor/gpui/src/platform/mac/text_system.rs");
        assert!(
            mac.contains("run.font_size.unwrap_or(font_size)"),
            "mac 排版层应把逐段字号用于字体实例"
        );
        assert!(
            mac.contains("clone_with_font_size(run_font_size.into())"),
            "mac 排版层应按逐段字号创建 CTFont"
        );
        assert!(
            mac.contains("font_size: run_font_size"),
            "mac 排版层应把逐段字号写回 ShapedRun"
        );

        let windows = include_str!("../../../../vendor/gpui/src/platform/windows/direct_write.rs");
        assert!(
            windows.contains("run.font_size.unwrap_or(font_size).0"),
            "windows 排版层应把逐段字号写进 DirectWrite 布局"
        );
        assert!(
            windows.contains("Some(px(glyphrun.fontEmSize))"),
            "windows 绘制层应把逐段字号写回 ShapedRun"
        );
    }

    #[gpui::test]
    async fn soft_wrapped_range_segments_stay_within_wrap_width(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let text = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
        let lines = shaped_lines(text, px(80.0), cx);
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "test text should soft-wrap"
        );

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(80.0), px(120.0)));
        let segments = super::range_segment_bounds(
            &lines,
            bounds,
            px(20.0),
            text,
            0..text.len(),
            TextAlign::Left,
        );

        assert!(segments.len() > 1);
        for segment in segments {
            assert!(segment.left() >= bounds.left());
            assert!(segment.right() <= bounds.right() + px(0.5));
        }
    }

    #[gpui::test]
    async fn wrapped_link_hit_matches_only_visible_segments(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let label = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
        let block = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Paragraph,
                    InlineTextTree::from_markdown(&format!("[{label}](https://example.com)")),
                ),
            )
        });

        let display_text = block.read_with(cx, |block, _cx| block.display_text().to_string());
        let lines = shaped_lines(&display_text, px(80.0), cx);
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "link text should soft-wrap"
        );

        let (hit, miss_right) = block.read_with(cx, |block, _cx| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(80.0), px(120.0)));
            let span = block
                .inline_spans()
                .iter()
                .find(|span| span.link.is_some())
                .expect("link span should exist");
            let segments = super::range_segment_bounds(
                &lines,
                bounds,
                px(20.0),
                &display_text,
                span.range.clone(),
                block.text_align(),
            );
            assert!(segments.len() > 1);
            let second_segment = segments[1];
            let hit = point(
                (second_segment.left() + second_segment.right()) / 2.0,
                (second_segment.top() + second_segment.bottom()) / 2.0,
            );
            let miss_right = point(second_segment.right() + px(24.0), hit.y);
            (
                link_at_position(block, &lines, bounds, px(20.0), hit)
                    .map(|link| link.open_target.clone()),
                link_at_position(block, &lines, bounds, px(20.0), miss_right)
                    .map(|link| link.open_target.clone()),
            )
        });

        assert_eq!(hit, Some("https://example.com".to_string()));
        assert_eq!(miss_right, None);
    }

    #[gpui::test]
    async fn wrapped_hard_line_top_accumulates_soft_wrap_height(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let text = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz\nnext";
        let lines = shaped_lines(text, px(80.0), cx);
        assert_eq!(lines.len(), 2);
        assert!(
            !lines[0].wrap_boundaries().is_empty(),
            "first hard line should soft-wrap"
        );

        let first_height = lines[0].size(px(20.0)).height;
        assert!(first_height > px(20.0));
        assert_eq!(super::wrapped_line_top(&lines, px(20.0), 1), first_height);
    }

    #[test]
    fn matching_bracket_pair_forward_backward_and_depth() {
        use super::matching_bracket_pair;
        let text = "fn f(a: (b, c)) { vec![x] }";
        // 光标在 '(' 之后（贴括号）→ 向后匹配到对应 ')'。
        let cursor = text.find('(').unwrap() + 1;
        let (open, close) = matching_bracket_pair(text, cursor).expect("pair");
        assert_eq!(&text[open..=open], "(");
        assert_eq!(&text[close..=close], ")");
        // 嵌套：内层 '(' 与内层 ')' 配对，不跳到外层。
        let inner_open = text.find("(b").unwrap();
        let (open, close) =
            matching_bracket_pair(text, inner_open + 1).expect("inner pair");
        assert_eq!(open, inner_open);
        assert_eq!(&text[close..=close], ")");
        // 光标停在最后一个闭括号右侧 → 向前匹配到外层开括号。
        let close_offset = text.rfind(')').unwrap();
        let (open, close) =
            matching_bracket_pair(text, close_offset + 1).expect("backward pair");
        assert_eq!(&text[open..=open], "(");
        assert_eq!(close, close_offset);
        // 无配对：孤立开括号 → None。
        assert_eq!(matching_bracket_pair("((", 1), None);
        // 非括号字符 → None。
        assert_eq!(matching_bracket_pair("abc", 1), None);
    }
