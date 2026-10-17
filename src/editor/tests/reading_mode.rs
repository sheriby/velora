use super::common::*;
use crate::components::{Copy, ToggleReadingMode};

#[gpui::test]
fn reading_mode_icon_is_registered_and_renders_visible_pixels(cx: &mut TestAppContext) {
    let source = crate::VeloraAssets;
    let bytes = gpui::AssetSource::load(&source, "icon/workspace/reading-mode.svg")
        .expect("加载阅读模式图标");
    assert!(
        bytes.is_some(),
        "正式应用资源表必须提供阅读模式图标，不能只显示空白按钮"
    );
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Svg, bytes.unwrap().into_owned());
    let decoded = cx
        .update(|cx| image.to_image_data(cx.svg_renderer()))
        .expect("解析并绘制阅读模式图标");
    assert!(
        decoded
            .as_bytes(0)
            .unwrap()
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0),
        "图标必须实际绘制可见像素"
    );
}

#[gpui::test]
fn reading_mode_also_protects_blocks_loaded_in_later_chunks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let original = (0..600)
        .map(|index| format!("> 第 {index} 段\n\n| 列 |\n| --- |\n| 值 |\n\n正文 {index}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let (editor, cx) = cx.add_window_view(|_, cx| {
        let mut editor = Editor::from_markdown_with_chunk_budget(cx, original.clone(), None, 1);
        editor.pending_materialization_task = None;
        editor
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.pending_materialization_task = None;
        assert!(editor.document.pending_tail().is_some());
        editor.toggle_reading_mode(cx);
        assert!(
            editor.materialize_next_pending_chunk(cx),
            "应覆盖文档尚未完成加载的阶段"
        );
        let blocks = editor.document.visible_blocks().to_vec();
        let mut editable = Vec::new();
        for visible in blocks.iter().skip(1) {
            let block = &visible.entity;
            let before = block.read(cx).display_text().to_string();
            block.update(cx, |block, cx| {
                block.replace_text_in_range(None, "误修改", window, cx)
            });
            if block.read(cx).display_text() != before {
                editable.push(block.read(cx).kind());
            }
            let cells = block
                .read(cx)
                .table_runtime
                .as_ref()
                .map(|runtime| {
                    runtime
                        .header
                        .iter()
                        .chain(runtime.rows.iter().flatten())
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for cell in cells {
                let before = cell.read(cx).display_text().to_string();
                cell.update(cx, |cell, cx| {
                    cell.replace_text_in_range(None, "误修改", window, cx)
                });
                if cell.read(cx).display_text() != before {
                    editable.push(BlockKind::Table);
                }
            }
        }
        assert!(editable.is_empty(), "后加载的这些块仍可编辑：{editable:?}");
        assert_eq!(editor.buffer.text(), original);
    });
}

#[gpui::test]
fn reading_mode_protects_full_document_selection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let original = "alpha\n\nbeta";
    let mut changed = Vec::new();
    for operation in ["delete", "backspace", "cut", "paste"] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.into(), None));
        redraw(cx);
        editor.update(cx, |editor, cx| {
            editor.toggle_reading_mode(cx);
            editor.select_document(cx);
        });
        redraw(cx);
        match operation {
            "delete" => cx.dispatch_action(crate::components::Delete),
            "backspace" => cx.dispatch_action(crate::components::DeleteBack),
            "cut" => cx.dispatch_action(crate::components::Cut),
            _ => cx.dispatch_action(crate::components::Paste),
        }
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            if editor.buffer.text() != original || editor.document_dirty {
                changed.push(operation);
            }
        });
    }
    assert!(changed.is_empty(), "全选正文后仍可修改：{changed:?}");
}

#[gpui::test]
fn reading_mode_navigation_does_not_append_editable_paragraphs(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let original = "```rust\nlet value = 1;\n```";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.into(), None));
    redraw(cx);
    editor.update(cx, |editor, cx| {
        editor.toggle_reading_mode(cx);
        let block = editor.document.first_root().unwrap().clone();
        let roots_before = editor.document.root_count();
        editor.on_block_event(
            block,
            &BlockEvent::RequestFocusNext { preferred_x: None },
            cx,
        );
        assert_eq!(
            editor.buffer.text(),
            original,
            "阅读时向下导航不应在文末补空行"
        );
        assert_eq!(
            editor.document.root_count(),
            roots_before,
            "阅读时不应新增编辑段落"
        );
    });
}

#[gpui::test]
fn reading_mode_blocks_typing_ime_and_editing_keys_in_both_views(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut changed = Vec::new();
    for source in [false, true] {
        for operation in [
            "typing",
            "ime",
            "backspace",
            "delete",
            "enter",
            "tab",
            "cut",
            "paste",
            "bold",
        ] {
            let original = "alpha\n\nbeta";
            let (editor, cx) =
                cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.into(), None));
            editor.update(cx, |editor, cx| {
                if source {
                    editor.toggle_view_mode(cx);
                }
                editor.toggle_reading_mode(cx);
            });
            redraw(cx);
            redraw(cx);
            let projection_before =
                editor.read_with(cx, |editor, cx| editor.document.markdown_text(cx));
            cx.update(|window, cx| {
                window.activate_window();
                let block = editor.read(cx).document.first_root().unwrap().clone();
                block.update(cx, |block, _| block.selected_range = 0..5);
                cx.write_to_clipboard(gpui::ClipboardItem::new_string("替换内容".into()));
                if operation == "ime" {
                    block.update(cx, |block, cx| {
                        block.replace_and_mark_text_in_range(None, "输入法", None, window, cx);
                    });
                }
            });
            match operation {
                "typing" => cx.simulate_input("误输入"),
                "ime" => {}
                "backspace" => cx.dispatch_action(crate::components::DeleteBack),
                "delete" => cx.dispatch_action(crate::components::Delete),
                "enter" => cx.dispatch_action(crate::components::Newline),
                "tab" => cx.simulate_keystrokes("tab"),
                "cut" => cx.dispatch_action(crate::components::Cut),
                "paste" => cx.dispatch_action(crate::components::Paste),
                _ => cx.dispatch_action(crate::components::BoldSelection),
            }
            cx.run_until_parked();
            editor.read_with(cx, |editor, cx| {
                if editor.buffer.text() != original
                    || editor.document_dirty
                    || editor.document.markdown_text(cx) != projection_before
                {
                    changed.push((source, operation));
                }
            });
        }
    }
    assert!(changed.is_empty(), "阅读模式仍能修改文档：{changed:?}");
}

#[gpui::test]
fn reading_mode_blocks_document_commands_and_keeps_undo_history(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let original = "标题\n===\n\n__alpha__";
    let mut changed = Vec::new();
    for operation in ["format", "heading", "insert", "replace", "undo", "redo"] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.into(), None));
        redraw(cx);
        if operation == "undo" || operation == "redo" {
            editor.update(cx, |editor, cx| editor.format_document(cx));
            if operation == "redo" {
                editor.update(cx, |editor, cx| editor.undo_document(cx));
            }
        }
        let before = editor.read_with(cx, |editor, _| {
            (
                editor.buffer.text(),
                editor.undo_history.len(),
                editor.redo_history.len(),
            )
        });
        editor.update(cx, |editor, cx| editor.toggle_reading_mode(cx));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| match operation {
                "format" => editor.format_document(cx),
                "heading" => {
                    editor.apply_heading_level_to_selection(2, cx);
                }
                "insert" => {
                    editor.insert_block_after_selection(
                        crate::editor::insert_ops::InsertBlockTarget::Separator,
                        cx,
                    );
                }
                "replace" => {
                    editor.workspace.search_query = "alpha".into();
                    editor.replace_all_document_matches(window, cx);
                }
                "undo" => editor.undo_document(cx),
                _ => editor.redo_document(cx),
            });
        });
        let after = editor.read_with(cx, |editor, _| {
            (
                editor.buffer.text(),
                editor.undo_history.len(),
                editor.redo_history.len(),
            )
        });
        if after != before {
            changed.push(operation);
        }
    }
    assert!(
        changed.is_empty(),
        "这些命令仍能在阅读模式修改文档或历史：{changed:?}"
    );
}

#[gpui::test]
fn reading_mode_preserves_selection_copy_search_and_scrolling(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let original = (0..100)
        .map(|index| format!("alpha 第 {index} 段"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.clone(), None));
    cx.update(|window, _| window.activate_window());
    redraw(cx);
    let button = cx
        .debug_bounds("status-bar-reading-mode")
        .expect("阅读模式切换按钮");
    cx.simulate_click(button.center(), Modifiers::none());
    redraw(cx);
    redraw(cx);
    let (start, end) = editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().unwrap().read(cx);
        let bounds = block.visible_range_bounds(0..5).expect("正文选区几何");
        (
            gpui::point(bounds.left() + px(0.5), bounds.center().y),
            gpui::point(bounds.right() - px(0.5), bounds.center().y),
        )
    });
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, Modifiers::none());
    redraw(cx);
    cx.dispatch_action(Copy);
    cx.update(|_, cx| {
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("alpha")
        )
    });
    assert!(
        cx.debug_bounds("editor-selection-toolbar").is_none(),
        "阅读模式不应显示格式工具栏"
    );
    editor.update(cx, |editor, cx| editor.open_document_find(cx));
    redraw(cx);
    cx.simulate_input("alpha");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.workspace.search_query, "alpha");
        assert_eq!(editor.buffer.text(), original);
        assert!(!editor.document_dirty);
    });
    let before = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    cx.dispatch_action(crate::components::PageDown);
    redraw(cx);
    let after = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    assert!(after < before, "阅读模式应仍可向下滚动");
    cx.dispatch_action(ToggleReadingMode);
    redraw(cx);
    editor.read_with(cx, |editor, _| assert!(!editor.is_read_only()));
}

#[gpui::test]
fn reading_mode_blocks_task_table_and_code_document_edits(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    for original in [
        "- [ ] 未完成",
        "| 名称 |\n| --- |\n| 内容 |",
        "```rust\nlet value = 1;\n```",
    ] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.into(), None));
        redraw(cx);
        editor.update(cx, |editor, cx| {
            editor.toggle_reading_mode(cx);
            let block = editor.document.first_root().unwrap().clone();
            editor.on_block_event(block.clone(), &BlockEvent::ToggleTaskChecked, cx);
            if block.read(cx).kind() == BlockKind::Table {
                editor.append_table_row(&block, cx);
                let cell = block.read(cx).table_runtime.as_ref().unwrap().rows[0][0].clone();
                let mode = cell.read(cx).is_read_only();
                assert!(mode, "表格格子必须继承阅读模式");
            }
            assert_eq!(editor.buffer.text(), original);
            assert!(!editor.document_dirty);
        });
    }
    let path = temp_markdown_path("reading-code").with_extension("rs");
    fs::write(&path, "let value = 1;\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.toggle_reading_mode(cx);
        editor.open_workspace_file(path.clone(), window, cx);
    });
    redraw(cx);
    cx.simulate_input("误修改");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.text(), "let value = 1;\n");
        assert!(!editor.document_dirty);
    });
    fs::remove_file(path).unwrap();
}

#[gpui::test]
fn reading_mode_applies_to_tabs_splits_and_only_the_current_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = temp_fixture_dir().join(format!("reading-mode-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    cx.on_quit({
        let root = root.clone();
        move || fs::remove_dir_all(root).unwrap()
    });
    let first = root.join("first.md");
    let second = root.join("second.md");
    fs::write(&first, "第一篇").unwrap();
    fs::write(&second, "第二篇").unwrap();
    let _config = crate::config::override_test_config_root(root.join("config"));
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(first.clone(), window, cx);
        editor.toggle_reading_mode(cx);
        editor.open_workspace_file(second.clone(), window, cx);
        editor.split_active_document(
            crate::editor::workspace::split::SplitDirection::Right,
            window,
            cx,
        );
    });
    redraw(cx);
    redraw(cx);
    let groups = editor.read_with(cx, |editor, _| editor.split_groups());
    assert_eq!(groups.len(), 2);
    for group in &groups {
        group.update_in(cx, |editor, window, cx| {
            let block = editor.document.first_root().unwrap().clone();
            block.update(cx, |block, cx| {
                block.replace_text_in_range(None, "误修改", window, cx)
            });
            assert_eq!(editor.buffer.text(), "第二篇");
            assert!(!editor.document_dirty);
        });
    }
    {
        let (other, other_cx) = cx
            .cx
            .add_window_view(|_, cx| Editor::from_markdown(cx, "其他窗口".into(), None));
        redraw(other_cx);
        other_cx.simulate_input("可编辑");
        other.read_with(other_cx, |editor, _| {
            assert!(!editor.is_read_only());
            assert!(
                editor.buffer.text().contains("可编辑"),
                "其他窗口不应受阅读模式影响"
            );
        });
    }
    assert_eq!(fs::read_to_string(&second).unwrap(), "第二篇");
    groups[0].update(cx, |editor, cx| editor.toggle_reading_mode(cx));
    cx.run_until_parked();
    for group in &groups {
        group.read_with(cx, |editor, _| assert!(!editor.is_read_only()));
    }
}
