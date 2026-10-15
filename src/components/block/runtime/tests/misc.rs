
use crate::components::markdown::inline::InlineTextTree;
use crate::components::{
    Block, BlockKind, BlockRecord, DeleteBack, TableCellPosition,
};
use gpui::{
    AppContext, EntityInputHandler,
    TestAppContext,
};



#[gpui::test]
async fn editing_link_anchor_in_math_block_matches_plain_paragraph(cx: &mut TestAppContext) {
    // A block mixing inline math with a link is "source preserving", which used
    // to route its link edits through the markdown-space path. That path assumed
    // the anchor label began right after `[`, so the anchor's own emphasis
    // markers shifted the mapping and edits landed on the wrong character. Inline
    // links now edit through the link projection in every block, so deleting a
    // revealed anchor delimiter touches the delimiter, not a label character.
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("$x^2$ [**bold**](https://e.com)"),
            ),
        )
    });

    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.move_to("$x^2$ bo".len(), cx);
            block.sync_inline_projection_for_focus(true);

            // Caret just past the revealed opening `**` of the bold anchor.
            let projected = block.display_text().to_string();
            assert_eq!(projected, "$x^2$ [**bold**](https://e.com)");
            let after_open = projected.find("[**").unwrap() + "[**".len();
            block.selected_range = after_open..after_open;

            block.on_delete_back(&DeleteBack, window, cx);

            let markdown = block.record.title.serialize_markdown();
            assert!(
                markdown.starts_with("$x^2$ "),
                "math source preserved: {markdown:?}"
            );
            assert!(
                markdown.contains("bold"),
                "anchor label must stay intact, only the delimiter is edited: {markdown:?}"
            );
        });
    });
}

#[gpui::test]
async fn completing_link_in_math_block_places_caret_after_closing_paren(cx: &mut TestAppContext) {
    // A block mixing math with a link edits in markdown space. Typing the closing
    // `)` completes the link, and the caret must land just past it (like a plain
    // paragraph) rather than inside the anchor before `]`.
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("$x$ [link](google.com"),
            ),
        )
    });

    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.move_to(block.visible_len(), cx);
            block.sync_inline_projection_for_focus(true);
            block.replace_text_in_range(None, ")", window, cx);
            block.sync_inline_projection_for_focus(true);

            assert_eq!(
                block.record.title.serialize_markdown(),
                "$x$ [link](google.com)"
            );
            assert_eq!(block.display_text(), "$x$ [link](google.com)");
            let end = block.visible_len();
            assert_eq!(block.selected_range, end..end);
        });
    });
}

#[gpui::test]
async fn rtl_selection_across_trailing_link_keeps_block_end_anchor(cx: &mut TestAppContext) {
    // A link sitting at the very end of a block that also contains inline math
    // stays expanded while the projection is rebuilt on every render. Dragging a
    // selection right-to-left from the block end across the link used to collapse
    // the anchor onto the closing `]` of the anchor text, because the trailing
    // `](url)` delimiters all share one clean offset and the remap snapped back
    // to the inner cursor position. The anchor must stay at the block end.
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("$x$ [link](google.com)"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.move_to(block.visible_len(), cx);
        block.sync_inline_projection_for_focus(true);
        let end = block.visible_len();
        assert_eq!(block.display_text(), "$x$ [link](google.com)");

        // Start an RTL selection at the block end and drag the head left,
        // re-syncing the projection after each move like the render loop does.
        block.move_to(end, cx);
        block.sync_inline_projection_for_focus(true);
        for target in (0..end).rev() {
            block.select_to(target, cx);
            block.sync_inline_projection_for_focus(true);
            assert_eq!(
                block.selected_range,
                target..end,
                "RTL selection anchor must stay at the block end (head {target})"
            );
            assert!(block.selection_reversed);
        }
    });
}

#[gpui::test]
async fn typing_destination_into_empty_link_parens_keeps_caret_inside(cx: &mut TestAppContext) {
    // Fixes an edge case where batched auto-pair macro `()+Left` caused first character typed
    // into `()` of link to snap the caret past `)` with rest of URL landing outside the link.
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("[GitHub]"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        // Auto-pair the `()` after the label, then drop the caret between them.
        block.selected_range = 8..8;
        block.sync_inline_projection_for_focus(true);
        block.replace_text_in_visible_range(8..8, "()", None, false, cx);
        block.sync_inline_projection_for_focus(true);
        let between = block.display_text().find(')').expect("closing paren");
        block.selected_range = between..between;
        block.sync_inline_projection_for_focus(true);
        for ch in "https://github.com".chars() {
            let at = block.selected_range.clone();
            block.replace_text_in_visible_range(at, &ch.to_string(), None, false, cx);
            block.sync_inline_projection_for_focus(true);
        }
    });

    block.read_with(cx, |block, _cx| {
        assert_eq!(
            block.record.title.serialize_markdown(),
            "[GitHub](https://github.com)"
        );
        assert_eq!(
            block.inline_link_at(1).map(str::to_string),
            Some("https://github.com".to_string())
        );
        // Caret stays inside `()`, just before the closing `)`.
        let close = block.display_text().find(')').expect("closing paren");
        assert_eq!(block.selected_range, close..close);
    });
}

#[gpui::test]
async fn h2_defaults_to_left_alignment_while_h1_stays_centered(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    for (level, expected) in [(2, gpui::TextAlign::Left), (1, gpui::TextAlign::Center)] {
        let heading = cx.new(|cx| {
            Block::with_record(
                cx,
                BlockRecord::new(
                    BlockKind::Heading { level },
                    InlineTextTree::plain("标题"),
                ),
            )
        });
        heading.read_with(cx, |heading, _| {
            assert_eq!(heading.text_align(), expected, "H{level} 的默认对齐");
        });
    }
}

#[gpui::test]
async fn table_header_cells_center_by_default_and_respect_explicit_alignment(
    cx: &mut TestAppContext,
) {
    // 表头默认居中；显式对齐语法（:--- / :---: / ---:）优先。
    use crate::components::TableColumnAlignment as Align;

    let cx = cx.add_empty_window();
    let make_cell = |cx: &mut gpui::VisualTestContext, row: usize, alignment| {
        cx.new(|cx| {
            let mut block = Block::with_record(
                cx,
                BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("单元格")),
            );
            block.set_table_cell_mode(
                TableCellPosition { row, column: 0 },
                alignment,
            );
            block
        })
    };
    let header_default = make_cell(cx, 0, Align::Default);
    let body_default = make_cell(cx, 1, Align::Default);
    let header_explicit_left = make_cell(cx, 0, Align::Left);
    let header_explicit_center = make_cell(cx, 0, Align::Center);
    let header_explicit_right = make_cell(cx, 0, Align::Right);

    header_default.read_with(cx, |block, _| {
        assert_eq!(
            block.text_align(),
            gpui::TextAlign::Center,
            "无对齐语法的表头应默认居中"
        );
    });
    body_default.read_with(cx, |block, _| {
        assert_eq!(block.text_align(), gpui::TextAlign::Left, "数据行默认左对齐");
    });
    header_explicit_left.read_with(cx, |block, _| {
        assert_eq!(
            block.text_align(),
            gpui::TextAlign::Left,
            "显式 :--- 的表头应保持左对齐"
        );
    });
    header_explicit_center.read_with(cx, |block, _| {
        assert_eq!(block.text_align(), gpui::TextAlign::Center);
    });
    header_explicit_right.read_with(cx, |block, _| {
        assert_eq!(block.text_align(), gpui::TextAlign::Right);
    });
}

#[gpui::test]
async fn double_click_selects_word_at_offset(cx: &mut TestAppContext) {
    // 双击选词：拉丁词、CJK 分组、标点分段、行尾兜底。
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::plain("hello 世界, world"),
            ),
        )
    });

    // "hello 世界, world" 字节布局（UAX#29：CJK 单字成段）：
    // hello=0..5, 世=6..9, 界=9..12, 逗号=12..13, world=14..19。
    let mut select_at = |offset: usize| {
        block.update(cx, |block, block_cx| {
            block.select_word_at(offset, block_cx);
            block.selected_range.clone()
        })
    };

    assert_eq!(select_at(1), 0..5, "点 hello 中间应选中整个 hello");
    assert_eq!(select_at(6), 6..9, "点 CJK 字符应选中该字");
    assert_eq!(select_at(10), 9..12, "第二个 CJK 字同理");
    assert_eq!(select_at(12), 12..13, "点标点应选中标点段");
    assert_eq!(select_at(17), 14..19, "点 world 中间应选中 world");
    assert_eq!(select_at(19), 14..19, "点文本末尾应选中最后一个词");
    assert_eq!(select_at(1_000), 14..19, "超界偏移应收敛到最后一个词");
}

/// 后台窗口不为光标闪烁（评审报告维度四建议 11）：`is_focused` 判不出窗口已不是
/// key window（macOS 红绿灯变灰时 gpui 不清 window.focus），只看焦点时后台窗口
/// 的 33ms 闪烁任务每拍 `cx.notify()`，前台线程持续全块重绘。闪烁开关要同时看
/// `window.is_window_active()`——公式编辑器的草稿光标（formula_editor.rs）同语义。
#[gpui::test]
async fn background_window_stops_the_cursor_blink_task(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::config::EditorSettings::install_test_settings(cx, true);
    });
    let (block, cx) = cx.add_window_view(|_, cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("正文")),
        )
    });

    // 前置：活动窗口 + 块聚焦，闪烁任务在跑。测试窗口默认失活，先显式激活。
    cx.update(|window, cx| {
        window.activate_window();
        let handle = block.read(cx).focus_handle.clone();
        window.focus(&handle);
    });
    cx.update(|window, cx| {
        window.draw(cx).clear();
    });
    cx.run_until_parked();
    assert!(
        block.read_with(cx, |block, _| block.cursor_blink_task.is_some()),
        "活动窗口 + 聚焦时闪烁任务应在跑"
    );

    // 窗口失活（用户切到别的应用/窗口）：闪烁任务必须停。
    cx.deactivate_window();
    cx.update(|window, cx| {
        window.draw(cx).clear();
    });
    cx.run_until_parked();
    assert!(
        block.read_with(cx, |block, _| block.cursor_blink_task.is_none()),
        "后台窗口不应保留光标闪烁任务（每 33ms 一次 notify 重绘）"
    );
}

/// 帧末布局写回只做引用计数（评审报告维度四建议 3 前半）：paint 里
/// `(*lines).clone()` 每帧深拷整份 Vec<WrappedLine>（每条含 runs/glyphs），
/// 被 30Hz 光标闪烁放大。last_layout 存 Arc 后，文本未变的下一帧 shape
/// 备忘命中、同一份布局只 bump 引用计数——两帧的 Arc 指针必须相等。
#[gpui::test]
async fn unchanged_text_reuses_the_same_layout_arc_across_frames(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
    });
    let (block, cx) = cx.add_window_view(|_, cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("第一行\n第二行")),
        )
    });

    cx.update(|window, cx| {
        window.draw(cx).clear();
    });
    cx.run_until_parked();
    let first = block.read_with(cx, |block, _| block.last_layout.clone());
    let first = first.expect("第一帧后应有布局");

    cx.update(|window, cx| {
        window.draw(cx).clear();
    });
    cx.run_until_parked();
    let second = block.read_with(cx, |block, _| block.last_layout.clone());
    let second = second.expect("第二帧后应有布局");

    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "文本未变的两帧应共用同一份布局（只 bump 引用计数，不深拷 Vec）"
    );
}

/// 换出/换进代码块的两条旁路入口也要清「代码族账本」（评审报告维度二建议
/// 12）：`convert_to_paragraph` 与 `enter_code_block` 直改 `record.kind`，
/// `code_is_indented`/`source_fence_lines` 留着的话，下一次序列化仍按旧
/// 形状落笔——缩进代码退回正文再敲 ``` 回车，刚打的围栏会被缩进账吃掉。
/// `set_kind_in_place` 早已清这两本账，三个入口必须同一口径。
#[gpui::test]
async fn code_kind_side_entries_clear_the_code_family_books(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();

    // 缩进代码 → 正文 → 围栏：账必须清干净。
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock { language: None },
                InlineTextTree::plain("缩进正文"),
            ),
        );
        block.record.code_is_indented = true;
        block
    });
    block.update(cx, |block, cx| {
        block.convert_to_paragraph(cx);
        block.enter_code_block(None, cx);
    });
    block.read_with(cx, |block, _cx| {
        assert!(
            !block.record.code_is_indented && block.record.source_fence_lines.is_none(),
            "缩进/围栏两本账必须清掉（code_is_indented={}, fence={:?}），否则写回按旧形状落笔",
            block.record.code_is_indented,
            block.record.source_fence_lines
        );
    });

    // 围栏代码 → 正文：同样不许留账。
    let block = cx.new(|cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::CodeBlock { language: Some("rust".into()) },
                InlineTextTree::plain("fn main() {}"),
            ),
        );
        block.record.source_fence_lines = Some((0, 2));
        block
    });
    block.update(cx, |block, cx| {
        block.convert_to_paragraph(cx);
    });
    block.read_with(cx, |block, _cx| {
        assert!(
            !block.record.code_is_indented && block.record.source_fence_lines.is_none(),
            "换出代码块后不得留围栏账"
        );
    });
}

/// 用户主动移标/选择后 IME 组合跨度必须作废（评审报告维度二建议 10）：
/// replace_text_in_range 在不带范围时回落到 marked_range——拼音组合中按
/// ←/→ 或点别处再打字，过期的跨度会把字写回已离开的区间。清的动作放在
/// move_to/select_to/select_word_at 三个用户意图入口，不放在
/// assign_collapsed_selection_offset 那层（IME 自己刚挂的跨度走那条路）。
#[gpui::test]
async fn moving_the_cursor_invalidates_a_stale_ime_marked_range(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain("alpha beta")),
        )
    });
    block.update(cx, |block, _cx| {
        block.marked_range = Some(4..9); // IME preedit 假象
    });
    cx.update(|_window, cx| {
        block.update(cx, |block, block_cx| block.move_to(1, block_cx));
    });
    block.read_with(cx, |block, _cx| {
        assert_eq!(
            block.marked_range,
            None,
            "移标后过期的组合跨度必须作废，否则下一次输入写回旧区间"
        );
    });

    // 下游效果：不带范围输入一个字符应落在光标处（1），不是旧 preedit 区间。
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "X", window, block_cx,
            );
        });
    });
    block.read_with(cx, |block, _cx| {
        assert_eq!(
            block.display_text().to_string(),
            "aXlpha beta",
            "输入应落在当前光标处"
        );
    });
}

/// Home/End 按光标所在**硬行**落点（评审报告维度二建议 8）：原来只认块首
/// 块尾——多行块里按 End 直接跳到整块末尾。行界判据与 Up/Down 同源
/// （shared_hard_line_ranges + line_index_for_offset），收进
/// cursor_line_display_range 一处。语料 "alpha\nbeta gamma"（行界 0..5 / 6..16）。
#[gpui::test]
async fn home_and_end_move_within_the_current_hard_line(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::plain("alpha\nbeta gamma"),
            ),
        )
    });

    // End：光标在第一行中间（2），应落第一行行尾 5。
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.move_to(2, block_cx);
            block.on_end(&crate::components::End, window, block_cx);
        });
    });
    block.read_with(cx, |block, _cx| {
        assert_eq!(block.cursor_offset(), 5, "End 应落在第一行行尾而非块尾 16");
    });

    // Home：光标在第二行中间（8），应回第二行行首 6。
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.move_to(8, block_cx);
            block.on_home(&crate::components::Home, window, block_cx);
        });
    });
    block.read_with(cx, |block, _cx| {
        assert_eq!(block.cursor_offset(), 6, "Home 应落在第二行行首而非块首 0");
    });

    // Shift+Home：从 8 选到第二行行首，得 6..8。
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.move_to(8, block_cx);
            block.on_select_home(&crate::components::SelectHome, window, block_cx);
        });
    });
    block.read_with(cx, |block, _cx| {
        assert_eq!(block.selected_range, 6..8, "Shift+Home 应选到行首");
    });

    // Shift+End：从 2 选到第一行行尾，得 2..5。
    cx.update(|window, cx| {
        block.update(cx, |block, block_cx| {
            block.move_to(2, block_cx);
            block.on_select_end(&crate::components::SelectEnd, window, block_cx);
        });
    });
    block.read_with(cx, |block, _cx| {
        assert_eq!(block.selected_range, 2..5, "Shift+End 应选到行尾");
    });
}

/// 引用内行首块语法判据认 1~3 空格缩进的围栏（评审报告维度一建议 3）：
/// 读侧 strip_fence_indent 认 ≤3 空格——引用内敲 ` ```js ` 必须触发
/// reparse，否则块结构在光标底下重构。判据只留 normalize 一处，
/// editor/events/quotes.rs 的那份抄本改调它。
#[gpui::test]
async fn quote_line_block_syntax_recognizes_indented_fences(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("> 引用正文\n```js"),
            ),
        )
    });
    cx.update(|window, cx| {
        block.update(cx, |block, _block_cx| {
            block.set_runtime_context(None, std::sync::Arc::default(), std::sync::Arc::default(), std::sync::Arc::default());
            let _ = window;
        });
    });
    block.read_with(cx, |_block, _cx| {
        assert!(
            crate::components::block::runtime::Block::multiline_quote_edit_requires_reparse(
                "> 引用正文\n```js",
            ),
            "前置：顶格围栏要触发 reparse"
        );
    });
    // 一格缩进的围栏（引用内粘贴的常见形状）同样要触发。
    block.read_with(cx, |_block, _cx| {
        assert!(
            crate::components::block::runtime::Block::multiline_quote_edit_requires_reparse(
                "> 引用正文\n ```js",
            ),
            "≤3 空格缩进的围栏必须触发 reparse（读侧认这个形状）"
        );
    });
    let _ = block;
}
