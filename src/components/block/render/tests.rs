use super::*;

mod tests {
    use super::{
        bulleted_list_marker, effective_table_width, inline_display_font_size,
        promotes_inline_images, tag_query, wikilink_target,
    };
    use crate::components::{BlockRecord, InlineScript, InlineSpan, InlineStyle};
    use gpui::AppContext;

    #[gpui::test]
    async fn table_measure_width_stays_within_the_writing_column_cap(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|_window, cx| {
            crate::theme::ThemeManager::init(cx);
            let theme = cx.global::<super::ThemeManager>().current_arc();
            let d = &theme.dimensions;
            let viewport_width = 1600.0;
            let cap =
                crate::config::EditorSettings::writing_width(cx).max_width(d.writing_max_width);
            assert!(
                crate::editor::Editor::centered_column_width(viewport_width, d) > cap,
                "前提失效：1600px 视口应宽到触发写作列上限（{cap}px）"
            );

            let block = cx.new(|cx| {
                Block::with_record(
                    cx,
                    BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown("x")),
                )
            });
            let table_width =
                block.read_with(cx, |block, cx| {
                    effective_table_width(block, viewport_width, d, cx)
                });
            assert!(
                table_width <= cap,
                "表格测量宽 {table_width}px 超过写作列上限 {cap}px：宽窗口下水位法按超宽容器\
                 算比例，套回真实窄容器后钉住列会被压到内容宽以下折行（用户报修）"
            );
        });
    }

    #[gpui::test]
    async fn table_column_layout_memo_skips_remeasure(cx: &mut TestAppContext) {
        use crate::components::markdown::table::parse_table_region;
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            crate::theme::ThemeManager::init(cx);
            let theme = cx.global::<super::ThemeManager>().current_arc();
            let table_for = |cells: [&str; 2]| {
                let source = format!("| {} | x |\n| --- | --- |\n| {} | y |", cells[0], cells[1]);
                let lines: Vec<String> = source.lines().map(str::to_string).collect();
                parse_table_region(&lines).expect("table parses")
            };

            let block = cx.new(|cx| {
                Block::with_record(cx, BlockRecord::table(table_for(["a", "1"])))
            });

            let first = block.update(cx, |block, cx| {
                block.cached_table_column_layout(760.0, &theme, window, cx)
            });
            assert!(first.is_some());

            // 篡改备忘值：同键再次调用必须原样返回备忘值，证明没有重测。
            block.update(cx, |block, _cx| {
                if let Some(memo) = block.column_layout_memo.as_mut() {
                    memo.layout =
                        crate::components::TableColumnLayout::equal(2);
                }
            });
            let second = block.update(cx, |block, cx| {
                block.cached_table_column_layout(760.0, &theme, window, cx)
            });
            assert_eq!(second.unwrap().fraction(0), 0.5, "同键调用应命中备忘而非重测");

            // 表内容变化：备忘必须被替换成基于新内容的条目。
            block.update(cx, |block, cx| {
                let changed = table_for(["much_wider_column", "2"]);
                block.record.table = Some(changed.clone());
                block.cached_table_column_layout(760.0, &theme, window, cx);
                let memo = block
                    .column_layout_memo()
                    .expect("测量后应写入备忘");
                assert_eq!(memo.table, changed, "表内容变化后备忘必须失效重测");
            });
        });
    }

    #[test]
    fn bulleted_list_marker_matches_browser_disc_circle_square() {
        // 一级实心圆、二级空心圆、三级及更深入全是实心方块（用户报修：三级显示了
        // 白色空心方块 U+25A1）。
        assert_eq!(bulleted_list_marker(0), "\u{2022}");
        assert_eq!(bulleted_list_marker(1), "\u{25E6}");
        assert_eq!(bulleted_list_marker(2), "\u{25AA}");
        assert_eq!(bulleted_list_marker(9), "\u{25AA}");
    }

    #[test]
    fn inline_code_content_never_becomes_an_image_widget() {
        let code = InlineStyle {
            code: true,
            ..InlineStyle::default()
        };
        assert!(!promotes_inline_images("![alt](path){width=NN%}", &code));
        assert!(promotes_inline_images("![alt](path)", &InlineStyle::default()));
    }

    #[test]
    fn inline_code_uses_the_code_font_size_while_scripts_shrink() {
        // 用户报修：点击含行内代码的行，代码字号会跳回正文大小；且行内代码
        // 应当跟随「代码块字体大小」设置。显示态与编辑态现在同字号。
        let span_with = |style: InlineStyle| InlineSpan {
            range: 0..1,
            style,
            html_style: None,
            link: None,
            footnote: None,
            math: None,
        };
        let code_span = span_with(InlineStyle {
            code: true,
            ..InlineStyle::default()
        });
        assert_eq!(inline_display_font_size(&code_span, 16.0, 13.0), 13.0);
        let superscript = span_with(InlineStyle {
            script: InlineScript::Superscript,
            ..InlineStyle::default()
        });
        assert_eq!(inline_display_font_size(&superscript, 16.0, 13.0), 11.52);
        let plain = span_with(InlineStyle::default());
        assert_eq!(inline_display_font_size(&plain, 16.0, 13.0), 16.0);
    }

    #[test]
    fn wikilink_target_extracts_trimmed_target() {
        assert_eq!(
            wikilink_target("[[meeting notes]]"),
            Some("meeting notes".to_string())
        );
        assert_eq!(
            wikilink_target("前缀 [[note]] 后缀"),
            Some("note".to_string())
        );
        assert_eq!(wikilink_target("[[ ]]"), None);
        assert_eq!(wikilink_target("no brackets"), None);
    }

    #[test]
    fn tag_query_accepts_words_and_rejects_empty_or_spaced() {
        assert_eq!(
            tag_query("#writing"),
            Some("#writing".to_string())
        );
        assert_eq!(
            tag_query("#中文标签"),
            Some("#中文标签".to_string())
        );
        assert_eq!(tag_query("#"), None);
        assert_eq!(tag_query("#has space"), None);
        assert_eq!(tag_query("plain"), None);
    }

    use super::{
        HtmlComputedStyle, html_node_visual_style, inline_word_chunks,
    };
    use crate::components::{Block, BlockKind, InlineTextTree, parse_html_document};
    use crate::i18n::I18nManager;
    use crate::theme::{Theme, ThemeManager};
    use gpui::{Hsla, Rgba, TestAppContext, px};

    #[test]
    fn table_axis_highlight_keeps_grid_and_header_distinct() {
        use crate::components::TableAxisHighlight;
        for theme in [Theme::forest_theme(), Theme::light_theme(), Theme::default_theme()] {
            for highlight in [TableAxisHighlight::Preview, TableAxisHighlight::Selected] {
                let colors = &theme.colors;
                let (body, border) = super::table_cell_colors(colors.table_cell_bg, highlight, false, colors);
                let (header, _) = super::table_cell_colors(colors.table_header_bg, highlight, false, colors);
                assert_eq!(border, colors.table_border, "列高亮必须保留网格线");
                assert_ne!(body, header, "选中列也应保留表头与正文的层次");
                assert_ne!(body, border, "网格线不能与背景融成一片");
            }
        }
    }

    fn assert_color_near(color: Hsla, red: u8, green: u8, blue: u8, alpha: u8) {
        let color = Rgba::from(color);
        let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as i16;
        assert!((channel(color.r) - red as i16).abs() <= 1);
        assert!((channel(color.g) - green as i16).abs() <= 1);
        assert!((channel(color.b) - blue as i16).abs() <= 1);
        assert!((channel(color.a) - alpha as i16).abs() <= 1);
    }

    #[test]
    fn inline_word_chunks_split_text_runs_for_wrapping() {
        // Plain runs split per word so the flex-wrap row can break between
        // words and keep neighboring inline math on the same visual line.
        assert_eq!(
            inline_word_chunks("Fusce x malesuada", false, false),
            vec!["Fusce ", "x ", "malesuada"],
        );
        // Trailing whitespace stays attached so spacing survives the split.
        assert_eq!(inline_word_chunks("end ", false, false), vec!["end "]);
        assert!(inline_word_chunks("", false, false).is_empty());
    }

    #[test]
    fn inline_word_chunks_keep_boxed_runs_whole() {
        // Inline code and background highlights keep their box continuous.
        assert_eq!(
            inline_word_chunks("let x = 2", true, false),
            vec!["let x = 2"],
        );
        assert_eq!(
            inline_word_chunks("highlighted text", false, true),
            vec!["highlighted text"],
        );
    }

    #[test]
    fn html_render_style_inherits_color_and_font_size() {
        let theme = Theme::default_theme();
        let doc = parse_html_document(
            "<div style=\"color:blue; font-size:20px\"><span style=\"font-size:120%\">x</span></div>",
        );
        let root = HtmlComputedStyle::root(&theme);
        let parent = html_node_visual_style(&doc.nodes[0], root, &theme);
        let child = html_node_visual_style(&doc.nodes[0].children[0], parent.computed, &theme);

        assert_color_near(parent.computed.color, 0, 0, 255, 255);
        assert_color_near(child.computed.color, 0, 0, 255, 255);
        assert!((child.computed.font_size - 24.0).abs() < 0.01);
    }

    #[test]
    fn html_render_style_overrides_link_and_mark_defaults() {
        let theme = Theme::default_theme();
        let link_doc = parse_html_document("<a style=\"color:red\">x</a>");
        let link_style =
            html_node_visual_style(&link_doc.nodes[0], HtmlComputedStyle::root(&theme), &theme);
        assert_color_near(link_style.computed.color, 255, 0, 0, 255);

        let mark_doc = parse_html_document("<mark style=\"background-color:#123\">x</mark>");
        let mark_style =
            html_node_visual_style(&mark_doc.nodes[0], HtmlComputedStyle::root(&theme), &theme);
        assert_color_near(mark_style.background.unwrap(), 0x11, 0x22, 0x33, 0xff);
    }

    #[test]
    fn html_render_style_does_not_inherit_background_color() {
        let theme = Theme::default_theme();
        let doc =
            parse_html_document("<div style=\"background-color:#112233\"><span>child</span></div>");
        let root = HtmlComputedStyle::root(&theme);
        let parent = html_node_visual_style(&doc.nodes[0], root, &theme);
        let child = html_node_visual_style(&doc.nodes[0].children[0], parent.computed, &theme);

        assert_color_near(parent.background.unwrap(), 0x11, 0x22, 0x33, 0xff);
        assert!(child.background.is_none());
    }

    #[gpui::test]
    async fn code_language_input_sits_in_header_above_code(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init(cx);
            ThemeManager::init(cx);
        });
        let (block, cx) = cx.add_window_view(|_window, cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::CodeBlock {
                        language: Some("rust".into()),
                    },
                    InlineTextTree::plain("fn main() {}\n"),
                ),
            )
        });

        cx.update(|window, cx| {
            block.update(cx, |block, _cx| {
                block.focus_handle.focus(window);
            });
            window.draw(cx).clear();
        });
        cx.run_until_parked();

        let (text_bounds, language_bounds) = block.read_with(cx, |block, _cx| {
            (
                block.last_bounds.expect("code text should render"),
                block
                    .code_language_last_bounds
                    .expect("language input should render"),
            )
        });
        assert!(language_bounds.left() > text_bounds.left());
        assert!(language_bounds.bottom() < text_bounds.top());
        let copy_bounds = cx.debug_bounds("code-copy-button").expect("顶部复制按钮");
        assert!(language_bounds.right() < copy_bounds.left());
        assert!(language_bounds.size.width <= px(156.0));
    }
}
