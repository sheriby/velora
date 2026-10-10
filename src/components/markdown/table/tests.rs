mod tests {
    use super::super::{
        TableColumnAlignment, TableColumnLayout, TableData, cell_chrome_width,
        collect_pipeless_table_region, collect_root_table_candidate_region,
        is_root_table_candidate_line, measure_preferred_column_widths, parse_root_table_region,
        parse_table_region, serialize_table_markdown_lines,
    };
    use crate::components::InlineTextTree;
    use gpui::{AppContext, Hsla, SharedString, TestAppContext, TextRun, font, px, rgba};

    /// 用户报修场景的端到端不变量：水位法钉住的列，必须放得下按「渲染格式」
    /// （行内代码换 code 字体族/字号、渲染期水平间距）排出来的最长行。
    /// 测量端任何与渲染端的管线错位（曾经先后漏掉：写作列宽上限、渲染间距、
    /// code 字体族）都会在这里以 mid-word 折行暴露。
    #[gpui::test]
    async fn pinned_columns_fit_render_formatted_lines(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            crate::theme::ThemeManager::init(cx);
            let theme = cx.global::<crate::theme::ThemeManager>().current_arc();
            let fonts = crate::config::EditorSettings::fonts(cx);

            let documents = [
                // 首张表：窄代码列 + 长散文列。
                "| File / folder | Purpose |\n| --- | --- |\n| `config.toml` | Preferences — most entries are editable in the preferences window |\n| `session.json` | Per-workspace open tabs, active tab and sidebar width |\n| `languages/` | External language packs |\n| `themes/` | External theme packs |",
                // 第二张表：窄代码列 + 超长代码清单列 + 散文列。
                "| Section | Keys | Controls |\n| --- | --- | --- |\n| `[window]` | `default_window_width`, `default_window_height`, `open_position`, `remember_bounds`, `zoom_percent` | Default size, centered vs. remembered opening position, window memory, UI zoom |\n| `[editor]` | `tree_sort`, `autosave_debounce_ms`, `new_file_template`, `smart_punctuation`, `external_change_policy`, `delete_policy`, `workspace_sidebar_width` | File-tree sort, autosave interval, new-file template (`{date}` expands), smart punctuation, external-change handling, Trash vs. permanent delete, sidebar width |\n| `[export]` | `theme` | `current` / `light` / `dark` for exported HTML, PDF and PNG |",
            ];
            let table_width = 760.0;

            for source in documents {
                let lines: Vec<String> = source.lines().map(str::to_string).collect();
                let table = parse_table_region(&lines).expect("用户报修表格应能解析");
                let layout = TableColumnLayout::measure(&table, table_width, window, &theme, cx);
                let preferred = measure_preferred_column_widths(&table, window, &theme, &fonts, true);
                let chrome = cell_chrome_width(&theme);

                for (column, &preferred) in preferred.iter().enumerate() {
                    let assigned = px(layout.fraction(column) * table_width);
                    if assigned + px(0.01) < preferred {
                        // 被挤压的列按设计必然换行，不检查。
                        continue;
                    }

                    let cells = std::iter::once(&table.header[column])
                        .chain(table.rows.iter().map(|row| &row[column]));
                    for cell in cells {
                        let markdown = super::super::serialize_table_cell_markdown(
                            &InlineTextTree::clone(cell),
                        );
                        let block = cx.new(|cx| {
                            crate::components::Block::with_record(
                                cx,
                                crate::components::BlockRecord::new(
                                    crate::components::BlockKind::Paragraph,
                                    InlineTextTree::from_markdown(&markdown),
                                ),
                            )
                        });
                        let (display, runs, code_ranges) = block.read_with(cx, |block, _cx| {
                            let display = SharedString::from(block.display_text().to_string());
                            let base = TextRun {
                                len: display.len(),
                                font: font(".SystemUIFont"),
                                color: Hsla::from(rgba(0x000000ff)),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                                font_size: None,
                            };
                            let runs = crate::components::block::element::build_text_runs(
                                block,
                                &display,
                                &base,
                                px(1.0),
                                base.color,
                                base.color,
                                true,
                                &fonts.code_family,
                                px(fonts.code_size as f32),
                                base.background_color.unwrap_or_default(),
                            );
                            let code_ranges: Vec<_> = block
                                .inline_spans()
                                .iter()
                                .filter(|span| span.style.code)
                                .map(|span| span.range.clone())
                                .collect();
                            (display, runs, code_ranges)
                        });

                        let mut shaped = window
                            .text_system()
                            .shape_text(
                                display.clone(),
                                px(theme.typography.text_size),
                                &runs,
                                None,
                                None,
                            )
                            .expect("cell should shape");
                        let letter_spacing =
                            px(theme.typography.text_size) * theme.typography.text_letter_spacing;
                        let code_gap = px(theme.dimensions.code_bg_pad_x)
                            + px(theme.typography.text_size) * 0.125;
                        crate::components::block::element::add_render_spacing(
                            &mut shaped,
                            &code_ranges,
                            letter_spacing,
                            code_gap,
                            px(theme.typography.text_size),
                        );
                        let longest = shaped
                            .iter()
                            .map(|line| line.width())
                            .max()
                            .unwrap_or(px(0.0));

                        let content = assigned - chrome;
                        assert!(
                            longest <= content,
                            "列 {column} 被钉在 {:.1}px（内容区 {:.1}px），\
                             但按渲染格式量出的最长行 {:.1}px 放不下，会 mid-word 折行：\
                             {markdown:?}",
                            f32::from(assigned),
                            f32::from(content),
                            f32::from(longest)
                        );
                    }
                }
            }
        });
    }

    fn assert_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() < 0.0001,
            "expected {left} to be close to {right}"
        );
    }

    #[test]
    fn parses_valid_root_table_region() {
        let lines = vec![
            "| Left | Center | Right |".to_string(),
            "| :--- | :---: | ---: |".to_string(),
            "| a | b | c |".to_string(),
        ];
        let table = parse_root_table_region(&lines).expect("table should parse");
        assert_eq!(table.alignments.len(), 3);
        assert_eq!(
            table.alignments,
            vec![
                TableColumnAlignment::Left,
                TableColumnAlignment::Center,
                TableColumnAlignment::Right
            ]
        );
        assert_eq!(table.header[0].serialize_markdown(), "Left");
        assert_eq!(table.rows[0][2].serialize_markdown(), "c");
    }

    #[test]
    fn rejects_invalid_alignment_row() {
        let lines = vec!["| Left | Right |".to_string(), "| nope | --- |".to_string()];
        assert!(parse_root_table_region(&lines).is_none());
    }

    #[test]
    fn rejects_alignment_row_with_wrong_column_count() {
        let lines = vec!["| A | B | C |".to_string(), "| --- | --- |".to_string()];
        assert!(parse_root_table_region(&lines).is_none());
    }

    #[test]
    fn accepts_short_alignment_dashes() {
        // GFM needs one hyphen per delimiter cell, so `| -- |` and `|:--|` are
        // tables. Both shapes came back from real documents that failed to render.
        let pipeless = vec![
            "源文件 | 行数 | 目标文件 | 动作 | 内容映射 |".to_string(),
            "----------------------------------------------------------------------- | --- | --------------- | -- | ---------------------------------------------------------------------------------------- |".to_string(),
            "`ascendc-dev-guide/references/ascendc-hardware-guide.md` | 160 | `references/npu-hardware-params.md` | 增强 | 分离模式与 SPMD".to_string(),
        ];
        let table = parse_root_table_region(&pipeless).expect("two-hyphen cell must parse");
        assert_eq!(table.alignments.len(), 5);
        assert_eq!(table.rows.len(), 1);

        let aligned = vec![
            "| 分组 | 总数 | 保留 | 存疑 | 剔除 |".to_string(),
            "|:--|:--:|:--:|:--:|:--:|".to_string(),
            "| 1a 产物分 >0.8 | 1 | 1 | 0 | 0 |".to_string(),
            "| **合计** | **3** | **2** | **0** | **1** |".to_string(),
        ];
        let table = parse_root_table_region(&aligned).expect("`:--` cell must parse");
        assert_eq!(
            table.alignments,
            vec![
                TableColumnAlignment::Left,
                TableColumnAlignment::Center,
                TableColumnAlignment::Center,
                TableColumnAlignment::Center,
                TableColumnAlignment::Center,
            ]
        );
        assert_eq!(table.rows.len(), 2);
    }

    #[test]
    fn rejects_alignment_cells_without_hyphens() {
        for cell in ["", ":", "::", "-:-", "--- ---", "abc"] {
            let lines = vec![
                "| A | B |".to_string(),
                format!("| {cell} | --- |"),
            ];
            assert!(
                parse_root_table_region(&lines).is_none(),
                "{cell:?} must not pass as a delimiter cell"
            );
        }
    }

    #[test]
    fn preserves_explicit_left_alignment_colon() {
        // ":---" is explicit left and must survive a parse/serialize round-trip
        // instead of being silently rewritten to a bare "---".
        let lines = vec![
            "| L | D | R |".to_string(),
            "| :--- | --- | ---: |".to_string(),
            "| a | b | c |".to_string(),
        ];
        let table = parse_root_table_region(&lines).expect("table should parse");
        assert_eq!(
            table.alignments,
            vec![
                TableColumnAlignment::Left,
                TableColumnAlignment::Default,
                TableColumnAlignment::Right
            ]
        );
        assert_eq!(
            serialize_table_markdown_lines(&table)[1],
            "| :--- | --- | ---: |"
        );
    }

    #[test]
    fn pads_short_body_rows_and_truncates_long_ones() {
        let lines = vec![
            "| A | B | C |".to_string(),
            "| --- | --- | --- |".to_string(),
            "| short |".to_string(),
            "| 1 | 2 | 3 | 4 |".to_string(),
        ];
        let table = parse_root_table_region(&lines).expect("table should parse");
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].len(), 3);
        assert_eq!(table.rows[0][0].serialize_markdown(), "short");
        assert!(table.rows[0][1].serialize_markdown().is_empty());
        assert!(table.rows[0][2].serialize_markdown().is_empty());
        assert_eq!(table.rows[1].len(), 3);
        assert_eq!(table.rows[1][2].serialize_markdown(), "3");
    }

    #[test]
    fn parses_pipeless_table() {
        let lines = vec![
            "Name | Score".to_string(),
            "--- | ---".to_string(),
            "Alice | 10".to_string(),
            "Bob | 7".to_string(),
        ];
        let end = collect_pipeless_table_region(&lines, 0).expect("region");
        assert_eq!(end, 4);
        let table = parse_root_table_region(&lines[..end]).expect("table should parse");
        assert_eq!(table.header.len(), 2);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.header[0].serialize_markdown(), "Name");
        assert_eq!(table.rows[1][1].serialize_markdown(), "7");
    }

    #[test]
    fn prose_with_pipe_is_not_a_pipeless_table() {
        let lines = vec!["this | that".to_string(), "and the next line".to_string()];
        assert!(collect_pipeless_table_region(&lines, 0).is_none());
    }

    #[test]
    fn pipeless_table_requires_valid_delimiter_row() {
        let lines = vec!["Name | Score".to_string(), "Alice | 10".to_string()];
        assert!(collect_pipeless_table_region(&lines, 0).is_none());
    }

    #[test]
    fn single_column_pipeless_is_not_a_table() {
        // Ambiguous with a setext heading; must not be captured as a table.
        let lines = vec!["Title".to_string(), "---".to_string()];
        assert!(collect_pipeless_table_region(&lines, 0).is_none());
    }

    #[test]
    fn serializes_canonical_pipe_table() {
        let table = TableData {
            header: vec![
                InlineTextTree::from_markdown("**bold**"),
                InlineTextTree::from_markdown("[link](https://example.com)"),
            ],
            rows: vec![vec![
                InlineTextTree::plain("A | B".to_string()),
                InlineTextTree::plain("value".to_string()),
            ]],
            alignments: vec![TableColumnAlignment::Default, TableColumnAlignment::Right],
        };
        assert_eq!(
            serialize_table_markdown_lines(&table),
            vec![
                "| **bold** | [link](https://example.com) |".to_string(),
                "| --- | ---: |".to_string(),
                "| A \\| B | value |".to_string(),
            ]
        );
    }

    #[test]
    fn cell_splitting_keeps_code_span_and_math_backslashes_literal() {
        // 用户报修（cases/02-table-code.md、cases/02-table-math.md）：切分单元格时
        // 对 `\\`/`\|` 做了一次无条件反转义——那是发生在「还不知道这段文字是不是
        // 代码段/公式」之前的预处理，把 `a\\b` 显示成 `a\b`、把 `$a\|b$` 的转义竖线
        // 洗成裸竖线，改了文档的意思。切分必须尊重 markdown 上下文，反转义留给
        // 单元格自己的行内解析。
        let lines = vec![
            "| Code | Escaped bar |".to_string(),
            "| --- | --- |".to_string(),
            "| `a\\\\b` | `a\\|b` |".to_string(),
            "| `C:\\tmp` | `x\\|y` |".to_string(),
        ];
        let table = parse_table_region(&lines).expect("代码段反斜杠表应能解析");
        assert_eq!(table.rows[0][0].visible_text(), "a\\\\b");
        assert_eq!(table.rows[0][1].visible_text(), "a\\|b");
        assert_eq!(table.rows[1][0].visible_text(), "C:\\tmp");
        assert_eq!(table.rows[1][1].visible_text(), "x\\|y");

        // 显示层不许动源码：保存按逐字节还原（这几行源文本已是规范写法）。
        assert_eq!(serialize_table_markdown_lines(&table), lines);

        let math_lines = vec![
            "| Escaped double bar | Word command |".to_string(),
            "| --- | --- |".to_string(),
            "| $a\\|b$ | $a\\Vert b$ |".to_string(),
        ];
        let table = parse_table_region(&math_lines).expect("公式转义竖线表应能解析");
        let norm = &table.rows[0][0];
        assert_eq!(norm.visible_text(), "$a\\|b$");
        let norm_cache = norm.render_cache();
        let math_span = norm_cache
            .inline_math_at(0)
            .expect("单元格公式应识别为公式");
        assert_eq!(math_span.body, "a\\|b", "公式体的转义竖线不许被洗掉");
        assert_eq!(table.rows[0][1].visible_text(), "$a\\Vert b$");
        assert_eq!(serialize_table_markdown_lines(&table), math_lines);

        let matrix_lines = vec![
            "| Value |".to_string(),
            "| --- |".to_string(),
            "| $\\begin{matrix}1&2\\\\3&4\\end{matrix}$ |".to_string(),
        ];
        let table = parse_table_region(&matrix_lines).expect("矩阵单元格应能解析");
        assert_eq!(
            table.rows[0][0].visible_text(),
            "$\\begin{matrix}1&2\\\\3&4\\end{matrix}$"
        );
        assert_eq!(serialize_table_markdown_lines(&table), matrix_lines);
    }

    #[test]
    fn escaped_pipe_outside_code_stays_one_cell_and_round_trips() {
        // 上下文规则的正面：代码段之外的 `\|` 是转义竖线，不作为分隔符切分；
        // 单元格内保存逐字节还原。
        let lines = vec![
            "| A | B |".to_string(),
            "| --- | --- |".to_string(),
            "| a\\|b | c |".to_string(),
        ];
        let table = parse_table_region(&lines).expect("转义竖线表应能解析");
        assert_eq!(table.rows[0].len(), 2);
        assert_eq!(table.rows[0][0].visible_text(), "a|b");
        assert_eq!(serialize_table_markdown_lines(&table), lines);
    }

    #[test]
    fn table_cell_br_renders_line_break_not_an_underlined_tag_name() {
        // 用户报修（cases/11-table-br.md，见 screenshots/11-table-br.png）：
        // 单元格里的 `first<br>second` 渲染成带下划线的 "br"。期望：真正断行、
        // 无下划线、无字面标签名，保存仍是 `<br>` 写法。
        let lines = vec![
            "| Item | Lines |".to_string(),
            "| --- | --- |".to_string(),
            "| A | first<br>second |".to_string(),
            "| B | **bold** and *italic* |".to_string(),
        ];
        let table = parse_table_region(&lines).expect("含 <br> 的表应能解析");
        let cell = &table.rows[0][1];
        assert_eq!(cell.visible_text(), "first\nsecond");
        assert!(
            cell.render_cache()
                .spans()
                .iter()
                .all(|span| span.link.is_none() && !span.style.underline),
            "`<br>` 单元格不许出现链接/下划线"
        );
        // 强调行不受影响。
        assert_eq!(table.rows[1][1].visible_text(), "bold and italic");
        assert_eq!(serialize_table_markdown_lines(&table), lines);
    }

    #[test]
    fn detects_root_table_candidate_runs() {
        let lines = vec![
            "| A | B |".to_string(),
            "| --- | --- |".to_string(),
            "| 1 | 2 |".to_string(),
            "paragraph".to_string(),
        ];
        assert!(is_root_table_candidate_line(&lines[0]));
        assert_eq!(collect_root_table_candidate_region(&lines, 0), 3);
    }

    #[test]
    fn roomy_table_keeps_columns_uniform() {
        let layout = TableColumnLayout::from_preferred_widths(&[32.0, 64.0, 48.0], 360.0, 60.0);
        let fractions = layout.fractions();
        assert_eq!(fractions.len(), 3);
        assert_close(fractions[0], 1.0 / 3.0);
        assert_close(fractions[1], 1.0 / 3.0);
        assert_close(fractions[2], 1.0 / 3.0);
    }

    #[test]
    fn content_pressure_redistributes_width_across_the_whole_column() {
        let layout = TableColumnLayout::from_preferred_widths(&[48.0, 220.0, 48.0], 360.0, 60.0);
        let fractions = layout.fractions();
        assert_eq!(fractions.len(), 3);
        assert!(fractions[1] > fractions[0]);
        assert!(fractions[1] > fractions[2]);
        assert_close(fractions[0], fractions[2]);
    }

    #[test]
    fn minimum_column_floor_prevents_neighbor_collapse() {
        let layout = TableColumnLayout::from_preferred_widths(&[16.0, 900.0, 16.0], 300.0, 70.0);
        let fractions = layout.fractions();
        let widths = fractions
            .iter()
            .map(|fraction| fraction * 300.0)
            .collect::<Vec<_>>();
        assert!(widths[0] >= 70.0 - 0.001);
        assert!(widths[2] >= 70.0 - 0.001);
        assert_close(fractions.iter().sum::<f32>(), 1.0);
    }

    #[test]
    fn moderate_single_cell_growth_stays_equal_when_share_is_sufficient() {
        let layout = TableColumnLayout::from_preferred_widths(&[56.0, 92.0, 56.0], 360.0, 60.0);
        let fractions = layout.fractions();
        assert_close(fractions[0], 1.0 / 3.0);
        assert_close(fractions[1], 1.0 / 3.0);
        assert_close(fractions[2], 1.0 / 3.0);
    }

    #[test]
    fn narrow_column_does_not_steal_the_average_share() {
        // 用户报修：一列内容很短、另外两列内容装不下时，短的列不该拿到平均份额
        // （1000/3≈333），而应该只拿「内容宽 + 一点点」，腾出的空间给宽列。
        let layout =
            TableColumnLayout::from_preferred_widths(&[60.0, 500.0, 500.0], 1000.0, 60.0);
        let widths = layout
            .fractions()
            .iter()
            .map(|fraction| fraction * 1000.0)
            .collect::<Vec<_>>();
        assert!(widths[0] <= 70.0, "窄列 {:.1} 宽于内容宽度", widths[0]);
        assert_close(widths[1], widths[2]);
        assert!(
            widths[1] > 1000.0 / 3.0 + 60.0,
            "宽列只拿到 {:.1}，没比平均份额多",
            widths[1]
        );
        assert_close(widths.iter().sum::<f32>(), 1000.0);
    }

    #[test]
    fn leftover_is_split_between_wide_columns_not_dumped_on_the_last_one() {
        // 钉住窄列后剩余空间在还缺空间的宽列之间平分；绝不能出现「最后一列独吞
        // 全部剩余」的情况。
        let layout =
            TableColumnLayout::from_preferred_widths(&[60.0, 500.0, 500.0], 1000.0, 60.0);
        let widths = layout
            .fractions()
            .iter()
            .map(|fraction| fraction * 1000.0)
            .collect::<Vec<_>>();
        assert!(widths[0] <= 70.0, "窄列 {:.1} 宽于内容宽度", widths[0]);
        assert_close(widths[1], widths[2]);
        assert!(
            widths[2] < 500.0,
            "最后一列拿到 {:.1}，把剩余全吞了",
            widths[2]
        );
        assert_close(widths.iter().sum::<f32>(), 1000.0);
    }

    #[test]
    fn wide_cell_does_not_squeeze_narrow_columns() {
        // 用户报修：最后一列内容极长的 5 列表格，前三列被挤成几个字符。列宽应该
        // 先满足内容少的列，剩下的空间才归长列。
        let layout = TableColumnLayout::from_preferred_widths(
            &[260.0, 60.0, 150.0, 60.0, 3000.0],
            1000.0,
            60.0,
        );
        let widths = layout
            .fractions()
            .iter()
            .map(|fraction| fraction * 1000.0)
            .collect::<Vec<_>>();
        assert_close(widths[0], 270.0);
        assert_close(widths[1], 70.0);
        assert_close(widths[2], 160.0);
        assert_close(widths[3], 70.0);
        assert_close(widths[4], 430.0);
        assert_close(widths.iter().sum::<f32>(), 1000.0);
    }

    #[test]
    fn append_row_preserves_column_count_and_creates_empty_cells() {
        let mut table = TableData::new_empty(1, 3);
        table.append_row();

        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[1].len(), 3);
        assert!(
            table.rows[1]
                .iter()
                .all(|cell| cell.serialize_markdown().is_empty())
        );
    }

    #[test]
    fn append_column_extends_every_row_and_uses_requested_alignment() {
        let mut table = TableData {
            header: vec![
                InlineTextTree::plain("A".to_string()),
                InlineTextTree::plain("B".to_string()),
            ],
            rows: vec![
                vec![
                    InlineTextTree::plain("1".to_string()),
                    InlineTextTree::plain("2".to_string()),
                ],
                vec![
                    InlineTextTree::plain("3".to_string()),
                    InlineTextTree::plain("4".to_string()),
                ],
            ],
            alignments: vec![TableColumnAlignment::Left, TableColumnAlignment::Right],
        };

        table.append_column(TableColumnAlignment::Right);

        assert_eq!(table.header.len(), 3);
        assert_eq!(table.rows[0].len(), 3);
        assert_eq!(table.rows[1].len(), 3);
        assert_eq!(
            table.alignments,
            vec![
                TableColumnAlignment::Left,
                TableColumnAlignment::Right,
                TableColumnAlignment::Right,
            ]
        );
        assert!(table.header[2].serialize_markdown().is_empty());
        assert!(table.rows[0][2].serialize_markdown().is_empty());
        assert!(table.rows[1][2].serialize_markdown().is_empty());
    }

    #[test]
    fn append_column_pads_missing_alignments_with_default() {
        let mut table = TableData {
            header: vec![InlineTextTree::plain("A".to_string())],
            rows: vec![vec![InlineTextTree::plain("1".to_string())]],
            alignments: Vec::new(),
        };

        table.append_column(TableColumnAlignment::Left);

        assert_eq!(
            table.alignments,
            vec![TableColumnAlignment::Default, TableColumnAlignment::Left]
        );
        assert_eq!(table.header.len(), 2);
        assert_eq!(table.rows[0].len(), 2);
    }

    #[test]
    fn set_column_alignment_updates_requested_column() {
        let mut table = TableData::new_empty(2, 3);
        table.set_column_alignment(1, TableColumnAlignment::Center);
        assert_eq!(
            table.alignments,
            vec![
                TableColumnAlignment::Default,
                TableColumnAlignment::Center,
                TableColumnAlignment::Default
            ]
        );
    }

    #[test]
    fn swap_visual_rows_exchanges_header_with_first_body_row() {
        let mut table = TableData {
            header: vec![InlineTextTree::plain("A".to_string())],
            rows: vec![
                vec![InlineTextTree::plain("1".to_string())],
                vec![InlineTextTree::plain("2".to_string())],
            ],
            alignments: vec![TableColumnAlignment::Left],
        };
        // Visual row 0 is the header; swapping it with visual row 1 exchanges
        // header and first-body content.
        table.swap_visual_rows(0, 1);
        assert_eq!(table.header[0].serialize_markdown(), "1");
        assert_eq!(table.rows[0][0].serialize_markdown(), "A");
        assert_eq!(table.rows[1][0].serialize_markdown(), "2");

        // Two body rows (visual 1 and 2) swap like ordinary rows.
        table.swap_visual_rows(1, 2);
        assert_eq!(table.rows[0][0].serialize_markdown(), "2");
        assert_eq!(table.rows[1][0].serialize_markdown(), "A");
    }

    #[test]
    fn swap_columns_exchanges_header_body_and_alignment() {
        let mut table = TableData {
            header: vec![
                InlineTextTree::plain("A".to_string()),
                InlineTextTree::plain("B".to_string()),
            ],
            rows: vec![vec![
                InlineTextTree::plain("1".to_string()),
                InlineTextTree::plain("2".to_string()),
            ]],
            alignments: vec![TableColumnAlignment::Left, TableColumnAlignment::Right],
        };
        table.swap_columns(0, 1);
        assert_eq!(table.header[0].serialize_markdown(), "B");
        assert_eq!(table.rows[0][0].serialize_markdown(), "2");
        assert_eq!(
            table.alignments,
            vec![TableColumnAlignment::Right, TableColumnAlignment::Left]
        );
    }

    #[test]
    fn remove_body_row_can_empty_the_table() {
        let mut table = TableData::new_empty(2, 2);
        table.remove_body_row(0);
        assert_eq!(table.rows.len(), 1);
        table.remove_body_row(0);
        // The last body row can be removed, leaving a header-only table.
        assert!(table.rows.is_empty());
        // Out-of-range removal is a no-op.
        table.remove_body_row(0);
        assert!(table.rows.is_empty());
    }

    #[test]
    fn remove_header_row_promotes_first_body_row() {
        let mut table = parse_root_table_region(&[
            "| A | B |".to_string(),
            "| --- | --- |".to_string(),
            "| 1 | 2 |".to_string(),
            "| 3 | 4 |".to_string(),
        ])
        .expect("valid table");

        assert!(table.remove_header_row());
        assert_eq!(table.header[0].serialize_markdown(), "1");
        assert_eq!(table.header[1].serialize_markdown(), "2");
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0][0].serialize_markdown(), "3");

        // Promoting the last remaining row leaves a header-only table.
        assert!(table.remove_header_row());
        assert!(table.rows.is_empty());
        assert!(!table.remove_header_row());
        assert_eq!(table.header[0].serialize_markdown(), "3");
    }

    #[test]
    fn remove_column_preserves_at_least_one_column() {
        let mut table = TableData::new_empty(2, 2);
        table.remove_column(0);
        assert_eq!(table.column_count(), 1);
        table.remove_column(0);
        assert_eq!(table.column_count(), 1);
    }
}
