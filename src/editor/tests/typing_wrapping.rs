use super::common::*;

#[gpui::test]
async fn typography_lists_share_a_hanging_indent_and_compact_spacing(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "- **图片**——粘贴或拖入即自动复制到文档资源目录。\n- **编辑顺手事**——列表回车自动续写。\n1. **工作区**——打开文件夹。\n2. **知识链接**——打开同名笔记。\n- [ ] **任务**——待完成。\n- [x] **任务**——已完成。";
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source.to_string(), None));
    cx.simulate_resize(gpui::size(px(1100.0), px(1000.0)));

    for theme in [
        Theme::default_theme(),
        Theme::light_theme(),
        Theme::paper_theme(),
        Theme::forest_theme(),
        Theme::midnight_theme(),
        Theme::ink_theme(),
    ] {
        let theme_name = theme.name.clone();
        cx.update(|_window, cx| cx.global_mut::<ThemeManager>().set_theme(theme));
        editor.update(cx, |_editor, cx| cx.notify());
        redraw(cx);
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let items: Vec<_> = editor
                .document
                .visible_blocks()
                .iter()
                .filter(|visible| visible.entity.read(cx).kind().is_list_item())
                .map(|visible| visible.entity.read(cx))
                .collect();
            assert_eq!(items.len(), 6);
            let first = items[0].last_bounds.expect("列表正文完成排版");
            for item in &items {
                let bounds = item.last_bounds.expect("列表正文完成排版");
                assert!(
                    (bounds.left() - first.left()).abs() <= px(1.0),
                    "{theme_name} 的无序、有序、任务列表正文应从同一列开始：{first:?} / {bounds:?}"
                );
            }
            // 紧列表原先叠加了每项上下内边距与行计划间距，项间空白超过半行。
            for pair in items.windows(2) {
                let previous = pair[0].last_bounds.expect("上一项排版");
                let current = pair[1].last_bounds.expect("当前项排版");
                let gap = current.top() - previous.bottom();
                assert!(
                    gap >= px(0.0) && gap <= pair[1].last_line_height * 0.25,
                    "{theme_name} 的列表项间距应小于四分之一行高，实际 {gap:?}"
                );
            }
            assert_eq!(editor.document.markdown_text(cx), source);
        });
    }
    drop(editor);
}

#[gpui::test]
async fn typography_h1_h2_center_text_and_hit_testing_in_every_theme(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            "# 一级标题\n\n## ✨ 功能\n\n### 写作与编辑\n\n正文".into(),
            None,
        )
    });
    for theme in [
        Theme::default_theme(),
        Theme::light_theme(),
        Theme::paper_theme(),
        Theme::forest_theme(),
        Theme::midnight_theme(),
        Theme::ink_theme(),
    ] {
        cx.update(|_window, cx| cx.global_mut::<ThemeManager>().set_theme(theme));
        editor.update(cx, |_editor, cx| cx.notify());
        redraw(cx);
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            for visible in editor.document.visible_blocks() {
                let block = visible.entity.read(cx);
                let expected = match block.kind() {
                    BlockKind::Heading { level: 1 | 2 } => gpui::TextAlign::Center,
                    _ => gpui::TextAlign::Left,
                };
                // 标题原先始终沿用表格以外的左对齐，绘制与点击必须共用新对齐口径。
                assert_eq!(block.text_align(), expected);
                if !matches!(block.kind(), BlockKind::Heading { .. }) {
                    continue;
                }
                let bounds = block.last_bounds.expect("标题排版");
                let line = &block.last_layout.as_ref().expect("标题行")[0];
                let left = crate::components::element::aligned_line_left(line, bounds, expected);
                if expected == gpui::TextAlign::Center {
                    assert!(
                        ((left + line.width() / 2.0) - (bounds.left() + bounds.size.width / 2.0))
                            .abs()
                            <= px(1.0)
                    );
                }
                for (index, _) in block.display_text().char_indices() {
                    let position = line
                        .position_for_index(index, block.last_line_height)
                        .expect("字形位置");
                    let hit = block.index_for_mouse_position(gpui::point(
                        left + position.x - px(0.1),
                        bounds.top() + block.last_line_height / 2.0,
                    ));
                    assert_eq!(hit, index, "居中标题点击必须仍命中原文字节");
                }
            }
        });
    }
    drop(editor);
}

#[gpui::test]
async fn typography_wrapped_heading_keeps_caret_selection_and_clicks_aligned(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let source = format!(
        "## {}",
        "标题居中以后换行时光标和选区必须跟随文字的位置".repeat(3)
    );
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, source, None));
    cx.simulate_resize(gpui::size(px(480.0), px(800.0)));
    redraw(cx);
    redraw(cx);
    let heading = editor.read_with(cx, |editor, _cx| {
        editor.document.first_root().expect("标题").clone()
    });
    heading.update(cx, |block, _cx| {
        let lines = block.last_layout.as_ref().expect("标题排版");
        let line = &lines[0];
        let boundary = line.wrap_boundaries().last().expect("标题应实际软换行");
        let start = line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index;
        let character_len = block.display_text()[start..]
            .chars()
            .next()
            .expect("换行后的字符")
            .len_utf8();
        let selected = block
            .visible_range_bounds(start..start + character_len)
            .expect("文字选区");
        block.selected_range = start..start;
        let caret = block.active_range_or_cursor_bounds().expect("光标位置");
        // 逐行居中后，最后一行的留白不同于第一行，不能继续复用整块的左边界。
        assert!(
            (caret.left() - selected.left()).abs() <= px(1.0),
            "换行标题的光标和选区应从同一个字开始：{caret:?} / {selected:?}"
        );
        assert_eq!(
            block.index_for_mouse_position(gpui::point(
                selected.left(),
                selected.top() + block.last_line_height / 2.0
            )),
            start
        );
        let bounds = block.last_bounds.expect("标题范围");
        assert!(
            (block.vertical_anchor_x() - (caret.left() - bounds.left())).abs() <= px(1.0),
            "上下键的水平锚点必须包含标题每行的居中留白"
        );
        assert_eq!(
            block.entry_offset_for_vertical_focus(
                true,
                Some(caret.left() - bounds.left() - px(0.1))
            ),
            start
        );
    });
}

#[gpui::test]
async fn typing_consecutive_backslashes_keeps_every_one(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "alpha".to_string(), None));

    let backslashes = |count: usize| "\\".repeat(count);

    // 用户报修：渲染模式里连按反斜杠只能得到一个（`\\` 被当成“转义的反斜杠”塔缩）。
    for count in 1..=3 {
        cx.simulate_input("\\");
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            assert_eq!(
                block.read(cx).display_text(),
                format!("{}alpha", backslashes(count))
            );
        });
    }
    // 写回保真（最小转义）：只有会被重读吃掉的反斜杠才翻倍——前两个后面
    // 跟着 `\` 要保护，第三个后面是字母 `a` 原样保留（3 个可见 -> 5 个字符）。
    let file = editor.read_with(cx, |editor, cx| editor.document.markdown_text(cx));
    assert_eq!(file, format!("{}alpha", backslashes(5)));
    // 重读一遍：用户敲的三个反斜杠一个不能少。
    let (reloaded, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, file, None));
    reloaded.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.visible_blocks()[0].entity.read(cx).display_text(),
            format!("{}alpha", backslashes(3))
        );
    });

    // 反斜杠不再吃掉后面的标记字符
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "seed".to_string(), None));
    cx.simulate_input("\\");
    cx.simulate_input("*");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(
            block.read(cx).display_text(),
            format!("{}*seed", backslashes(1))
        );
        // `\` 后面跟着 `*`（可转义）：写回翻倍成 `\\`，重读还是「反斜杠 + 星号」。
        assert_eq!(
            editor.document.markdown_text(cx),
            format!("{}*seed", backslashes(2))
        );
    });

    // 一次插入一段文本（粘贴）：UNC 路径的反斜杠必须保真
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "seed".to_string(), None));
    cx.simulate_input(&format!("{}server{}share", backslashes(2), backslashes(1)));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        assert_eq!(
            block.read(cx).display_text(),
            format!("{}server{}shareseed", backslashes(2), backslashes(1))
        );
    });
}

#[gpui::test]
async fn typing_backslashes_in_link_blocks_does_not_multiply(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let count_backslashes = |value: &str| value.matches('\\').count();

    // 用户报修：行首是自动链接的块里按反斜杠，可见数量按「两倍加一」翻倍
    // （1 -> 3 -> 7）。这三类块都走 markdown 源直编路径。
    // 反斜杠的翻倍只发生在**插入点**（escape_markdown_insertion，防连锁翻倍）；
    // 序列化按转义区间原样保留、不再叠加——文件里恒为「可见数 × 2」，重读不变。
    for source in [
        "<https://example.com> tail",
        "[a][b] tail",
        "[a](https://example.com) tail",
    ] {
        let (editor, cx) = cx.add_window_view({
            let source = source.to_string();
            move |_window, cx| Editor::from_markdown(cx, source, None)
        });
        cx.simulate_keystrokes("home");
        redraw(cx);
        for typed in 1..=3 {
            cx.simulate_input("\\");
            redraw(cx);
            editor.read_with(cx, |editor, cx| {
                let block = editor.document.visible_blocks()[0].entity.clone();
                let screen = block.read(cx).display_text();
                let file = editor.document.markdown_text(cx);
                assert_eq!(
                    count_backslashes(&screen),
                    typed,
                    "{source:?} 输入 {typed} 次后可见反斜杠数量（屏幕 {screen:?}）"
                );
                assert_eq!(
                    count_backslashes(&file),
                    typed * 2,
                    "{source:?} 输入 {typed} 次后源文件反斜杠数量（文件 {file:?}）"
                );
            });
        }
    }

    // 光标贴在自动链接末尾（行尾）时同样不能把插入点落进 `<...>` 里。
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "tail <https://example.com>".to_string(), None)
    });
    cx.simulate_keystrokes("end");
    redraw(cx);
    for typed in 1..=2 {
        cx.simulate_input("\\");
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            let screen = block.read(cx).display_text();
            assert_eq!(
                count_backslashes(&screen),
                typed,
                "行尾输入 {typed} 次后可见反斜杠数量（屏幕 {screen:?}）"
            );
        });
    }
}

#[gpui::test]
async fn caret_lands_outside_leading_and_trailing_markup(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 用户报修：块首是 `**`、`<...>`、`[...]()` 这类标记时，打开文件后光标不在行首
    // （跑到标记里面），行尾光标也会落进标记内部甚至越过可见文本。
    for source in [
        "alpha",
        "**bold** tail",
        "<https://example.com> tail",
        "tail <https://example.com>",
        "[a](https://example.com) tail",
    ] {
        let (editor, cx) = cx.add_window_view({
            let source = source.to_string();
            move |_window, cx| Editor::from_markdown(cx, source, None)
        });
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            let block = block.read(cx);
            assert_eq!(block.selected_range, 0..0, "{source:?} 初始光标应在块首");
        });

        cx.simulate_keystrokes("end");
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.clone();
            let block = block.read(cx);
            let display_len = block.display_text().len();
            let clean_len = block.record.title.visible_text().len();
            assert_eq!(
                block.selected_range,
                display_len..display_len,
                "{source:?} 行尾光标应停在显示文本末尾"
            );
            assert_eq!(
                block.current_to_clean_offset(block.selected_range.start),
                clean_len,
                "{source:?} 行尾光标在可见文本里的位置应是末尾"
            );
        });
    }
}

#[gpui::test]
async fn cjk_wrapping_does_not_start_lines_with_punctuation(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 中文没有词间空格，断行器逐字断行；没有行首禁则时标点会被推到下一行行首
    // （用户报修：标点经常出现在一行开头）。断点落在哪个字由容器宽度决定，所以
    // 这里把所有宽度都扫一遍——旧实现在其中不少宽度上会让标点开头。
    let text = "这是一段用来验证中文行首禁则的文字，里面有逗号、顿号；还有冒号：和分号。句号也不该落到行首，问号呢？感叹号也是！如果标点出现在行首，就说明禁则没有生效。".to_string();
    let font = gpui::Font {
        family: ".SystemUIFont".into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: gpui::FontWeight::NORMAL,
        style: gpui::FontStyle::Normal,
    };
    let mut wrapped_lines = 0usize;
    for width in (60..260).step_by(2) {
        let boundaries = cx.update(|cx| {
            let text_system = cx.text_system().clone();
            let mut wrapper = text_system.line_wrapper(font.clone(), px(12.0));
            wrapper
                .wrap_line(&[gpui::LineFragment::text(&text)], px(width as f32))
                .map(|boundary| boundary.ix)
                .collect::<Vec<_>>()
        });
        wrapped_lines += boundaries.len();
        for ix in boundaries {
            let rest = text.get(ix..).unwrap_or_default();
            let first = rest.chars().next().unwrap_or(' ');
            assert!(
                !"，、；：。？！）】”’".contains(first),
                "{width}px 宽时空行断点让标点出现在行首 {first:?}：{rest}"
            );
        }
    }
    assert!(wrapped_lines > 100, "折行样本太少（{wrapped_lines}），测试没跑够");
}

#[gpui::test]
async fn shaped_text_wrapping_respects_punctuation_rules(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let cx = cx.add_empty_window();
    // 编辑器走 shape_text，而不是 LineWrapper；覆盖截图中的中英文混排和窄行回退。
    let texts = [
        "Velora 把你的写作文件夹作为工作区打开，边打字边渲染 Markdown，在以 MiB 计的长篇手稿上依然流畅。代码文件在同一窗口内以语法高亮编辑，所有确认与提示都是应用内模态——不会有系统弹窗打断写作。",
        "这是一段中文，含有逗号、顿号；冒号：句号。问号？感叹号！以及连续标点？！和省略号……末尾。",
        "中文（Markdown）和“Velora”以及《Rust》文字【混排】结束。",
        "甲，乙。丙？！丁……戊",
        "（Markdown）“Velora”《Rust》【GPUI】",
    ];
    let mut wrapped_lines = 0usize;
    for text in texts {
        let run = gpui::TextRun {
            len: text.len(),
            font: gpui::Font {
                family: ".SystemUIFont".into(),
                features: gpui::FontFeatures::default(),
                fallbacks: None,
                weight: gpui::FontWeight::NORMAL,
                style: gpui::FontStyle::Normal,
            },
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size: None,
        };
        for width in (1..920).step_by(2) {
            cx.update(|window, _cx| {
                let lines = window
                    .text_system()
                    .shape_text(
                        text.into(),
                        px(18.0),
                        &[run.clone()],
                        Some(px(width as f32)),
                        None,
                    )
                    .expect("text should shape");
                for line in lines {
                    let mut previous_ix = 0;
                    for boundary in line.wrap_boundaries() {
                        let ix = line.unwrapped_layout.runs[boundary.run_ix].glyphs
                            [boundary.glyph_ix]
                            .index;
                        assert!(ix > previous_ix, "{width}px 换行断点必须递增");
                        previous_ix = ix;
                        let first = line.text[ix..].chars().next().unwrap();
                        let last = line.text[..ix].chars().next_back().unwrap();
                        assert!(
                            !"，、；：。？！…）】》”’".contains(first),
                            "{width}px 实际排版让标点出现在行首 {first:?}：{}",
                            &line.text[ix..]
                        );
                        assert!(
                            !"（【《“‘".contains(last),
                            "{width}px 实际排版让起始符号出现在行末 {last:?}：{}",
                            &line.text[..ix]
                        );
                        wrapped_lines += 1;
                    }
                }
            });
        }
    }
    assert!(wrapped_lines > 100, "必须实际产生足够的软换行");
}

#[gpui::test]
async fn rendered_prose_wraps_to_width_without_leading_punctuation(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "持有 `image_reference_definitions/link_reference_definitions/footnote_registry`（共享），以及 `table_cells: HashMap<EntityId, TableCellBinding>`。这是 mixed text! 含有 commas, periods. questions? semicolons; colons: 以及右括号 (closing) [bracket] {brace} 和中文（右括号）【方括号】《书名》、“右引号”，都不该落在行首。";
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, markdown.to_string(), None)
    });
    for width in [480.0, 640.0, 820.0] {
        cx.simulate_resize(gpui::size(px(width), px(800.0)));
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            let lines = block.last_layout.as_ref().expect("正文应完成排版");
            let mut wraps = 0;
            for line in lines {
                let available = line.wrap_width.expect("正文应有换行宽度");
                let mut start_x = px(0.0);
                for boundary in line.wrap_boundaries() {
                    let glyph = &line.unwrapped_layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
                    let first = line.text[glyph.index..].trim_start().chars().next().unwrap();
                    assert!(
                        !"!！,，.。?？;；:：)]}>）］｝】》〉」』”’…、".contains(first),
                        "{width}px 行首出现 {first:?}：{}", &line.text[glyph.index..]
                    );
                    let row_width = glyph.position.x - start_x;
                    assert!(row_width <= available + px(0.5), "正文不应溢出");
                    for word in ["mixed", "text", "commas", "periods", "questions", "semicolons", "colons", "closing", "bracket", "brace"] {
                        for (start, _) in line.text.match_indices(word) {
                            assert!(
                                !(start < glyph.index && glyph.index < start + word.len()),
                                "{width}px 普通英文单词 {word} 被拆开"
                            );
                        }
                    }
                    start_x = glyph.position.x;
                    wraps += 1;
                }
            }
            assert!(wraps > 2, "应实际覆盖多行中英文混排");
        });
    }
}

#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
async fn undo_scroll_after_document_replace_stays_single_shot(cx: &mut TestAppContext) {
    // 用户报修：编辑中按 Ctrl+Z，窗口来回滚动。撤销会替换整篇块，这一帧的
    // 块边界/行高都是旧布局或估计值，最容易出现「先按旧几何滚一次、下一帧
    // 再按新几何纠正」。这里守住：撤销后滚动应用 ≤1 次、逐帧偏移不反向，
    // 且撤销后的活动块最终落在视口内。
    init_editor_test_app(cx);
    let markdown = (0..300)
        .map(|index| {
            let code = (0..20)
                .map(|line| format!("line {line} of block {index}\n"))
                .collect::<String>();
            format!("## 第 {index} 节\n\n第 {index} 段。\n\n```rust\n{code}```\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (editor, cx) = cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown, None));
    for _ in 0..4 {
        redraw(cx);
    }

    let offset = |editor: &gpui::Entity<Editor>, cx: &mut gpui::VisualTestContext| {
        f32::from(editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y))
    };
    let target = editor.read_with(cx, |editor, _| {
        let blocks = editor.document.visible_blocks();
        blocks[blocks.len() * 3 / 4].entity.entity_id()
    });
    editor.update(cx, |editor, _cx| editor.focus_block(target));
    redraw(cx);

    let mut previous = offset(&editor, cx);
    for round in 0..3 {
        cx.simulate_input("x");
        redraw(cx);
        let before = editor.read_with(cx, |editor, _| editor.caret_scroll_applications.get());
        editor.update(cx, |editor, cx| editor.undo_document(cx));
        let mut frames = Vec::new();
        for _ in 0..6 {
            redraw(cx);
            frames.push(offset(&editor, cx));
            cx.executor().advance_clock(Duration::from_millis(32));
            cx.run_until_parked();
        }
        let applications =
            editor.read_with(cx, |editor, _| editor.caret_scroll_applications.get()) - before;
        eprintln!("[measure] 第 {} 次撤销：应用 {applications} 次，逐帧 {frames:?}", round + 1);

        // 允许一次「行高收敛」后的同向校正，但不许来回反复。
        assert!(
            applications <= 2,
            "第 {} 次撤销后滚动应用了 {applications} 次（应一次到位 + 至多一次同向校正）",
            round + 1
        );
        let span = frames
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), value| {
                (lo.min(*value), hi.max(*value))
            });
        assert!(
            span.1 - span.0 <= 2.0 * 1052.0,
            "第 {} 次撤销后视野漂移 {:.0}px（超过两个视口）：{frames:?}",
            round + 1,
            span.1 - span.0
        );
        // 逐帧偏移不许反向（允许一次到位后保持不动）。
        let mut direction = 0.0f32;
        for pair in frames.windows(2) {
            let delta = pair[1] - pair[0];
            if delta.abs() <= 0.5 {
                continue;
            }
            if direction == 0.0 {
                direction = delta.signum();
            } else {
                assert_eq!(
                    delta.signum(),
                    direction,
                    "第 {} 次撤销后滚动反向：{frames:?}",
                    round + 1
                );
            }
        }
        previous = frames.last().copied().unwrap_or(previous);
    }

    // 撤销后的活动块必须落在视口内（撤销后整篇块都换了新实体，要按当前
    // 活动块查，而不是撤销前那个 id）。
    let diag = editor.read_with(cx, |editor, cx| {
        let viewport = editor.scroll_handle.bounds();
        let active = editor.active_entity_id;
        let lookup = active.and_then(|id| editor.focusable_entity_by_id(id));
        let bounds = lookup
            .as_ref()
            .and_then(|block| block.read_with(cx, |block, _| block.active_range_or_cursor_bounds()));
        let index = active.and_then(|id| editor.document.visible_index_for_entity_id(id));
        (viewport, active.is_some(), lookup.is_some(), bounds, index)
    });
    eprintln!(
        "[probe] viewport {:?} / active? {} / lookup? {} / bounds {:?} / index {:?}",
        diag.0, diag.1, diag.2, diag.3, diag.4
    );
    let visible = diag
        .3
        .as_ref()
        .map(|bounds| bounds.bottom() > diag.0.top() && bounds.top() < diag.0.bottom())
        .unwrap_or(false);
    assert!(visible, "撤销后活动块应在视口内，当前偏移 {previous:.1}");
}


/// 在空段落里打 `+ ` 变成无序项，记号要留下用户打的那个 `+`。
///
/// 快捷语法只回答「这是无序项」，写法是它丢掉的半个信息；丢了写法，这块以后任何
/// 一次整块落笔都会把 `+` 写成 `-`（同理 `1)` 被写成 `1.`）。
#[gpui::test]
async fn typing_a_plus_bullet_shortcut_keeps_the_plus_marker(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    redraw(cx);

    cx.simulate_input("+ 项目");
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("list item");
        assert_eq!(block.read(cx).kind(), BlockKind::BulletedListItem);
        assert_eq!(
            block.read(cx).record.list_marker.bullet,
            Some('+'),
            "快捷语法把用户写的加号记号丢了"
        );
        assert_eq!(
            editor.document.markdown_text(cx),
            "+ 项目",
            "整块序列化把加号写成了减号"
        );
    });
}

/// 4K 屏（逻辑 3840 宽）上正文列不再卡在定值那一小截：宽度按窗口比例算，
/// 换行宽度跟着窗口长；窄窗口仍是标准档的 760px（比例算出来比下限小就取下限）。
#[gpui::test]
async fn the_writing_column_follows_a_4k_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            "alpha beta gamma delta epsilon zeta eta theta iota kappa\n".to_string(),
            None,
        )
    });
    redraw(cx);

    let wrap_width = |cx: &mut VisualTestContext| -> f32 {
        editor.read_with(cx, |editor, cx| {
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            let lines = block.last_layout.as_ref().expect("正文应完成排版");
            f32::from(lines[0].wrap_width.expect("正文应有换行宽度"))
        })
    };

    cx.simulate_resize(gpui::size(px(3840.0), px(2160.0)));
    redraw(cx);
    let four_k = wrap_width(cx);
    assert!(
        four_k > 3840.0 * 0.5,
        "4K 窗口下正文列只有 {four_k}px，不到半屏：写作宽度还是定值那一套"
    );

    cx.simulate_resize(gpui::size(px(1200.0), px(800.0)));
    redraw(cx);
    let narrow = wrap_width(cx);
    assert!(
        (730.0..=761.0).contains(&narrow),
        "1200px 窗口的正文列该还是标准档的 760px（少说掉两侧块的 24px 内边距），实得 {narrow}"
    );
}

// 撤销前的偏移也必须纳入断言；只比较撤销后的帧会漏掉先跳走再回来的闪烁。
#[gpui::test]
async fn undo_and_redo_a_visible_character_keep_the_viewport_still(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = (0..80)
        .map(|index| {
            format!(
                "## 第 {index} 节\n\n第 {index} 段。\n\n```rust\n{}```\n\n",
                "let value = 1;\n".repeat(20)
            )
        })
        .collect::<String>();
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
    for _ in 0..4 {
        redraw(cx);
    }
    let target = editor.read_with(cx, |editor, cx| {
        editor
            .document
            .visible_blocks()
            .iter()
            .find(|entry| entry.entity.read(cx).record.title.visible_text() == "第 60 段。")
            .expect("编辑目标")
            .entity
            .entity_id()
    });
    editor.update(cx, |editor, cx| {
        editor.focus_block(target);
        editor.pending_scroll_active_block_into_view = true;
        editor.pending_scroll_center_into_view = true;
        editor.pending_scroll_recheck_after_layout = true;
        cx.notify();
    });
    for _ in 0..20 {
        cx.executor().advance_clock(Duration::from_millis(32));
        redraw(cx);
    }
    cx.simulate_input("x");
    for _ in 0..8 {
        cx.executor().advance_clock(Duration::from_millis(32));
        redraw(cx);
    }
    let edited = editor.read_with(cx, |editor, _| editor.buffer.text());
    for redo in [false, true] {
        let before = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
        editor.update(cx, |editor, cx| {
            if redo {
                editor.redo_document(cx);
            } else {
                editor.undo_document(cx);
            }
        });
        for frame in 0..8 {
            redraw(cx);
            let offset = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
            assert!(
                (offset - before).abs() <= px(1.0),
                "{}第 {frame} 帧改变可见位置：{before:?} → {offset:?}",
                if redo { "重做" } else { "撤销" }
            );
            cx.executor().advance_clock(Duration::from_millis(32));
            cx.run_until_parked();
        }
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            if redo {
                edited.clone()
            } else {
                markdown.clone()
            }
        );
    }
}
