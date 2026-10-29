use super::common::*;

#[gpui::test]
async fn core_repair_document_height_cache_tracks_code_fonts_and_theme_metrics(cx: &mut TestAppContext) {
    use crate::config::preferences::{AppPreferences, open_preferences_window_with_state};
    init_editor_test_app(cx);
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    let preferences = cx.update(|cx| open_preferences_window_with_state(cx, AppPreferences::default(), Vec::new(), "字体".into()));
    let source = (0..80).map(|index| format!("paragraph {index}\n\n```rust\nlet value = {index};\n```\n\n")).collect::<String>();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source, None));
    redraw(cx);
    redraw(cx);
    let mut before = editor.read_with(cx, |editor, _| editor.row_stride_cache_clears.get());
    let mut failures = Vec::new();
    let mut settings = AppPreferences::default();
    for (label, family, size) in [("代码字体", "Courier New", 14), ("代码字号", "Courier New", 20)] {
        settings.fonts.code_family = family.into();
        settings.fonts.code_size = size;
        preferences.update(cx, |preferences, window, cx| preferences.apply_saved_preferences(settings.clone(), window, cx)).unwrap();
        redraw(cx);
        let after = editor.read_with(cx, |editor, _| editor.row_stride_cache_clears.get());
        if after <= before { failures.push(label); }
        before = after;
    }
    cx.update(|_, cx| {
        let mut theme = cx.global::<ThemeManager>().current_arc().as_ref().clone();
        theme.typography.text_line_height += 0.3;
        cx.global_mut::<ThemeManager>().set_theme(theme);
    });
    redraw(cx);
    let after = editor.read_with(cx, |editor, _| editor.row_stride_cache_clears.get());
    if after <= before { failures.push("主题行高"); }
    assert!(failures.is_empty(), "这些排版输入变更未作废旧高度：{failures:?}");
    redraw(cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.row_stride_cache_clears.get()), after);
}

/// 只换正文字体族（行内代码字体不动）也要重测列宽。
///
/// 列宽备忘当时只把「窗口字体 + 代码字体」混进指纹：正文族的偏好位
/// （`fonts.markdown_family`）不在键里，于是设置里换正文字体后 memo 全键命中、
/// 不重测，列宽按旧字体钉住——渲染按新字体排，mid-word 折行。
#[gpui::test]
async fn review_table_body_font_family_alone_invalidates_column_measurement(
    cx: &mut TestAppContext,
) {
    use crate::config::preferences::{AppPreferences, open_preferences_window_with_state};
    init_editor_test_app(cx);
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    let preferences = cx.update(|cx| {
        open_preferences_window_with_state(cx, AppPreferences::default(), Vec::new(), "字体".into())
    });
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(
            cx,
            "| thin | `wide` |\n| --- | --- |\n| content | `code` |".into(),
            None,
        )
    });
    redraw(cx);
    redraw(cx);
    let table = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    let before = table.read_with(cx, |table, _| table.column_layout_measurements);

    let mut settings = AppPreferences::default();
    settings.fonts.markdown_family = "Arial".into();
    preferences
        .update(cx, |preferences, window, cx| {
            preferences.apply_saved_preferences(settings, window, cx)
        })
        .unwrap();
    redraw(cx);
    let after = table.read_with(cx, |table, _| table.column_layout_measurements);
    assert!(
        after > before,
        "正文（markdown）字体族换了，列宽却没重测：{before} -> {after}"
    );
}

#[gpui::test]
async fn core_repair_table_font_families_invalidate_column_measurement(cx: &mut TestAppContext) {
    use crate::config::preferences::{AppPreferences, open_preferences_window_with_state};
    init_editor_test_app(cx);
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    let preferences = cx.update(|cx| open_preferences_window_with_state(cx, AppPreferences::default(), Vec::new(), "字体".into()));
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "| thin | `wide` |\n| --- | --- |\n| content | `code` |".into(), None));
    redraw(cx);
    redraw(cx);
    let table = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    let before = table.read_with(cx, |table, _| table.column_layout_measurements);
    redraw(cx);
    assert_eq!(table.read_with(cx, |table, _| table.column_layout_measurements), before);
    let mut settings = AppPreferences::default();
    settings.fonts.markdown_family = "Arial".into();
    settings.fonts.code_family = "Courier New".into();
    preferences.update(cx, |preferences, window, cx| preferences.apply_saved_preferences(settings, window, cx)).unwrap();
    redraw(cx);
    let after = table.read_with(cx, |table, _| table.column_layout_measurements);
    assert!(after > before, "字体家族变化后列宽须重新测量：{before} -> {after}");
    redraw(cx);
    assert_eq!(table.read_with(cx, |table, _| table.column_layout_measurements), after);
}

#[gpui::test]
async fn core_repair_style_changes_with_identical_text_refresh_shaped_runs(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "first\n\nabc".into(), None));
    redraw(cx);
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| editor.document.root_blocks()[1].clone());
    let before = block.read_with(cx, |block, _| block.shape_memo_entry().unwrap().lines);
    block.update(cx, |block, cx| {
        let mut title = block.record.title.clone();
        title.toggle_bold(0..3);
        block.record.set_title(title);
        block.sync_render_cache();
        cx.notify();
    });
    redraw(cx);
    let after = block.read_with(cx, |block, _| {
        assert_eq!(block.display_text(), "abc");
        assert!(block.inline_spans().iter().any(|span| span.style.bold));
        let after = block.shape_memo_entry().unwrap().lines;
        assert!(!Arc::ptr_eq(&before, &after));
        after
    });
    redraw(cx);
    let unchanged = block.read_with(cx, |block, _| block.shape_memo_entry().unwrap().lines);
    assert!(Arc::ptr_eq(&after, &unchanged), "输入未变的帧应继续复用排版产物");
}

#[gpui::test]
async fn core_repair_same_text_composition_refreshes_and_removes_underline(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "abc".into(), None));
    redraw(cx);
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| editor.document.first_root().unwrap().clone());
    let before = block.read_with(cx, |block, _| block.shape_memo_entry().unwrap().lines);
    block.update_in(cx, |block, window, cx| block.replace_and_mark_text_in_range(Some(0..3), "abc", Some(3..3), window, cx));
    redraw(cx);
    let marked = block.read_with(cx, |block, _| block.shape_memo_entry().unwrap().lines);
    assert!(!Arc::ptr_eq(&before, &marked), "预编辑标记必须进入当前排版产物");
    block.update_in(cx, |block, window, cx| block.unmark_text(window, cx));
    redraw(cx);
    let committed = block.read_with(cx, |block, _| block.shape_memo_entry().unwrap().lines);
    assert!(!Arc::ptr_eq(&marked, &committed), "取消组合标记不能继续复用预编辑的排版产物");
}

#[gpui::test]
async fn changing_font_categories_relayouts_body_and_code_independently(cx: &mut TestAppContext) {
    use crate::config::preferences::{
        AppPreferences, FontPreferences, open_preferences_window_with_state,
    };
    init_editor_test_app(cx);
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    let preferences = cx.update(|cx| {
        open_preferences_window_with_state(cx, AppPreferences::default(), Vec::new(), "字体".into())
    });
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(
            cx,
            "# Heading\n\nBody `inline`.\n\n```rust\nlet answer = 42;\n```\n".into(),
            None,
        )
    });
    redraw(cx);
    redraw(cx);
    let snapshot = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, cx| {
            editor
                .document
                .visible_blocks()
                .iter()
                .filter_map(|visible| {
                    let block = visible.entity.read(cx);
                    if block.display_text().is_empty() {
                        return None;
                    }
                    let memo = block.shape_memo_entry().expect("文本块应完成排版");
                    let line = block
                        .last_layout
                        .as_ref()
                        .and_then(|lines| lines.first())
                        .expect("文本应有绘制布局");
                    Some((
                        line.unwrapped_layout.font_size,
                        memo.key.font_fingerprint,
                        line.runs()
                            .iter()
                            .filter_map(|run| run.font_size)
                            .collect::<Vec<_>>(),
                    ))
                })
                .collect::<Vec<_>>()
        })
    };
    let before = snapshot(cx);
    assert_eq!(before.len(), 3);
    let mut fonts = FontPreferences::default();
    fonts.ui_family = "Courier New".into();
    fonts.ui_size = 28;
    preferences
        .update(cx, |preferences, window, cx| {
            preferences.apply_saved_preferences(
                AppPreferences {
                    fonts: fonts.clone(),
                    ..AppPreferences::default()
                },
                window,
                cx,
            );
        })
        .expect("偏好窗口应可更新");
    redraw(cx);
    assert_eq!(
        snapshot(cx),
        before,
        "更换 UI 字体和字号不应改变正文与代码的排版输入"
    );

    fonts.markdown_family = "Arial".into();
    fonts.markdown_size = 20;
    preferences
        .update(cx, |preferences, window, cx| {
            preferences.apply_saved_preferences(
                AppPreferences {
                    fonts: fonts.clone(),
                    ..AppPreferences::default()
                },
                window,
                cx,
            );
        })
        .expect("偏好窗口应可更新");
    redraw(cx);
    let body_changed = snapshot(cx);
    assert_eq!(
        body_changed[0].0,
        before[0].0 * 1.25,
        "标题随正文字号按比例变化"
    );
    assert_eq!(body_changed[1].0, px(20.0));
    assert_ne!(body_changed[1].1, before[1].1, "正文应使用新字体重新排版");
    assert_eq!(body_changed[2].0, before[2].0, "正文字号设置不应影响代码");
    assert_eq!(
        body_changed[1].2,
        vec![px(14.0)],
        "正文中的行内代码仍用代码字号"
    );

    fonts.code_family = "Consolas".into();
    fonts.code_size = 18;
    preferences
        .update(cx, |preferences, window, cx| {
            preferences.apply_saved_preferences(
                AppPreferences {
                    fonts: fonts.clone(),
                    ..AppPreferences::default()
                },
                window,
                cx,
            );
        })
        .expect("偏好窗口应可更新");
    redraw(cx);
    let code_changed = snapshot(cx);
    assert_eq!(code_changed[0].0, body_changed[0].0);
    assert_eq!(code_changed[1].0, body_changed[1].0);
    assert_eq!(
        code_changed[1].2,
        vec![px(18.0)],
        "行内代码也随代码字号调整"
    );
    assert_eq!(code_changed[2].0, px(18.0));
    assert_ne!(code_changed[2].1, before[2].1, "代码应使用新字体重新排版");
}

#[gpui::test]
async fn render_structure_snapshot_for_key_blocks(cx: &mut TestAppContext) {
    // roadmap G7：关键块渲染结构的黄金快照。任何解析/渲染回归改动若
    // 改变块序列或文本，需同步更新此快照并在 PR 中说明。
    let source = "# Title\n\nBody with **bold**, `code` and [link](https://x).\n\n- one\n- two\n\n- [ ] task\n\n> quoted\n\n```rust\nlet x = 1;\n```\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    editor.read_with(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        let snapshot: Vec<(String, String)> = visible
            .iter()
            .map(|visible| {
                let block = visible.entity.read(cx);
                (
                    format!("{:?}", block.kind()),
                    block.display_text().to_string(),
                )
            })
            .collect();

        let expected = vec![
            ("Heading { level: 1 }".to_string(), "Title".to_string()),
            (
                "Paragraph".to_string(),
                "Body with bold, code and link.".to_string(),
            ),
            (
                "BulletedListItem".to_string(),
                "one".to_string(),
            ),
            (
                "BulletedListItem".to_string(),
                "two".to_string(),
            ),
            // 列表组与下一块之间的空段落分隔（设计使然）。
            ("Paragraph".to_string(), String::new()),
            (
                "TaskListItem { checked: false }".to_string(),
                "task".to_string(),
            ),
            ("Quote".to_string(), "quoted".to_string()),
            (
                "CodeBlock { language: Some(\"rust\") }".to_string(),
                "let x = 1;".to_string(),
            ),
            // 表格内容由 table runtime 渲染，display_text 为空（设计使然）。
            ("Table".to_string(), String::new()),        ];
        assert_eq!(snapshot, expected, "render structure snapshot mismatch");
    });
}

#[gpui::test]
async fn heading_fold_hides_section_content(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "## Section\n\nalpha\n\nbeta\n\n## Next\n\ngamma";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    editor.update(cx, |editor, cx| {
        let visible = editor.document.visible_blocks();
        assert_eq!(visible.len(), 5); // H, alpha, beta, H2, gamma
        let heading = visible[0].entity.clone();
        heading.update(cx, |block, _cx| block.folded = true);

        let filtered = editor
            .apply_heading_fold_filter(cx)
            .iter()
            .map(|&index| {
                editor.document.visible_blocks()[index as usize]
                    .entity
                    .read(cx)
                    .display_text()
                    .to_string()
            })
            .collect::<Vec<_>>();

        // 折叠章节内容 alpha/beta 被隐藏，下一同级标题保持可见。
        assert_eq!(
            filtered,
            vec!["Section".to_string(), "Next".to_string(), "gamma".to_string()]
        );
    });
}

#[gpui::test]
async fn heading_fold_hides_descendant_headings_and_preserves_child_fold_state(
    cx: &mut TestAppContext,
) {
    // 折叠 H1/H2 时子标题仍保留，已折叠的子标题还会覆盖父级的隐藏范围。
    // 子标题和正文必须一起隐藏，直到遇到同级或更高标题；子级状态仍需保留。
    init_editor_test_app(cx);
    for (parent_level, boundary_level) in [(1, 1), (2, 2), (2, 1)] {
        for child_folded in [false, true] {
            let source = format!(
                "前文\n\n{} 父节\n\n父节正文\n\n{} 子节\n\n子节正文\n\n{} 孙节\n\n孙节正文\n\n{} 另一个子节\n\n另一子正文\n\n{} 下一节\n\n后文",
                "#".repeat(parent_level),
                "#".repeat(parent_level + 1),
                "#".repeat(parent_level + 2),
                "#".repeat(parent_level + 1),
                "#".repeat(boundary_level),
            );
            let editor = cx.new(|cx| Editor::from_markdown(cx, source, None));
            editor.update(cx, |editor, cx| {
                let parent = editor
                    .document
                    .visible_blocks()
                    .iter()
                    .find(|entry| entry.entity.read(cx).display_text() == "父节")
                    .expect("父标题")
                    .entity
                    .clone();
                let child = editor
                    .document
                    .visible_blocks()
                    .iter()
                    .find(|entry| entry.entity.read(cx).display_text() == "子节")
                    .expect("子标题")
                    .entity
                    .clone();
                parent.update(cx, |block, _| block.folded = true);
                child.update(cx, |block, _| block.folded = child_folded);
                let filtered = editor
                    .apply_heading_fold_filter(cx)
                    .iter()
                    .map(|&index| {
                        editor.document.visible_blocks()[index as usize]
                            .entity
                            .read(cx)
                            .display_text()
                            .to_string()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    filtered,
                    ["前文", "父节", "下一节", "后文"],
                    "折叠 H{parent_level} 应隐藏全部子标题，子节折叠状态 {child_folded}"
                );
                parent.update(cx, |block, _| block.folded = false);
                let restored = editor
                    .apply_heading_fold_filter(cx)
                    .iter()
                    .map(|&index| {
                        editor.document.visible_blocks()[index as usize]
                            .entity
                            .read(cx)
                            .display_text()
                            .to_string()
                    })
                    .collect::<Vec<_>>();
                assert!(restored.iter().any(|text| text == "另一个子节"));
                assert_eq!(restored.iter().any(|text| text == "孙节"), !child_folded);
                assert_eq!(child.read(cx).folded, child_folded);
            });
        }
    }
}

#[gpui::test]
async fn heading_fold_chevron_marks_only_foldable_headings(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "## Section\n\nalpha\n\n### Child\n\nbeta\n\n## Empty\n\n## Next\n\ngamma";
    let editor = cx.new(|cx| Editor::from_markdown(cx, source.into(), None));

    editor.update(cx, |editor, cx| {
        editor.apply_heading_fold_filter(cx);
        let visible = editor.document.visible_blocks().to_vec();
        let foldable = visible
            .iter()
            .filter(|visible| matches!(visible.entity.read(cx).kind(), BlockKind::Heading { .. }))
            .map(|visible| visible.entity.read(cx).foldable)
            .collect::<Vec<_>>();
        // Section / Child / Next 后方有章节内容；Empty 紧跟同级标题，没有可折叠内容。
        assert_eq!(foldable, vec![true, true, false, true]);
    });
}

#[gpui::test]
async fn broken_image_placeholder_stays_compact_inside_the_column(cx: &mut TestAppContext) {
    // callout 列表项里的图片读不出来时，占位框必须贴着文字收紧，上限只到所在列的
    // 可用宽度；按「视口估算宽度」估算会画出横穿整屏、冲出 callout 右边界的空心条。
    init_editor_test_app(cx);
    let markdown = "> [!IMPORTANT] 混合块\n>\n> - bold\n> - ![image](missing-image.png)\n";
    let (_editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, markdown.into(), None));
    // 宽窗口：旧实现按视口估算出 definite 宽度，占位框会拉成一条空心长条。
    cx.update(|window, _cx| window.resize(gpui::size(px(1400.0), px(900.0))));
    redraw(cx);

    let viewport_width = cx.update(|window, _cx| window.viewport_size().width);
    let bounds = cx
        .debug_bounds("image-placeholder")
        .expect("broken image should render a placeholder box");
    assert!(
        bounds.size.width <= px(400.0),
        "占位框应贴着文字收紧，实测宽度 {:?}",
        bounds.size.width
    );
    assert!(
        bounds.right() < viewport_width,
        "占位框右边 {:?} 不应超出视口宽度 {viewport_width:?}",
        bounds.right()
    );
}

/// 稳态帧不许深拷渲染主题：约 200 个字段的 Theme 每帧克隆一份，与热路径
/// 用 Arc 的设计直接冲突。设置不变时应当复用缓存。
#[gpui::test]
async fn steady_frames_reuse_the_cached_paint_theme(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "alpha\n\nbeta".to_string(), None)
    });
    redraw(cx);
    redraw(cx);
    let before = editor.read_with(cx, |editor, _cx| editor.paint_theme_clones.get());
    for _ in 0..3 {
        redraw(cx);
    }
    let after = editor.read_with(cx, |editor, _cx| editor.paint_theme_clones.get());
    assert_eq!(
        after, before,
        "稳态帧又深拷了 {} 份主题",
        after - before
    );
}
