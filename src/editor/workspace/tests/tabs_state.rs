use super::super::Editor;
use super::tabs_sessions::{init_review_workspace_app, review_workspace_fixture};
use gpui::{TestAppContext, px};
use std::fs;

#[gpui::test]
async fn dragging_tabs_can_insert_after_the_last_tab_and_before_the_first(
    cx: &mut TestAppContext,
) {
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "tab-drop-sides");
    let _config = crate::config::override_test_config_root(root.join("config"));
    let paths = [
        root.join("alpha.md"),
        root.join("beta.md"),
        root.join("gamma.md"),
    ];
    for path in &paths {
        fs::write(path, "正文\n").expect("标签夹具");
    }
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update_in(cx, |editor, window, cx| {
        for path in &paths {
            editor.open_workspace_file(path.clone(), window, cx);
        }
    });
    let drag_tab = |cx: &mut gpui::VisualTestContext, source: &'static str, target: &'static str, after: bool| {
        use gpui::{Modifiers, MouseButton, point};
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        let source_bounds = cx.debug_bounds(source).expect("起点标签");
        let target_bounds = cx.debug_bounds(target).expect("落点标签");
        let start = point(source_bounds.origin.x + px(30.0), source_bounds.center().y);
        let end = point(
            target_bounds.origin.x + target_bounds.size.width * if after { 0.75 } else { 0.25 },
            target_bounds.center().y,
        );
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start + point(px(15.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        assert!(cx.cx.read(|cx| cx.has_active_drag()), "拖动应启动标签排序");
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    };
    drag_tab(cx, "document-tab-0", "document-tab-2", true);
    editor.read_with(cx, |editor, _| {
        let ordered: Vec<_> = editor.workspace.open_documents.iter()
            .map(|tab| tab.path.clone()).collect();
        assert_eq!(
            ordered,
            vec![paths[1].clone(), paths[2].clone(), paths[0].clone()],
            "右半区应插到目标后面，允许移到末尾"
        );
        assert_eq!(editor.workspace.active_document.as_ref(), Some(&paths[2]));
    });
    drag_tab(cx, "document-tab-2", "document-tab-0", false);
    editor.read_with(cx, |editor, _| {
        let ordered: Vec<_> = editor.workspace.open_documents.iter()
            .map(|tab| tab.path.clone()).collect();
        assert_eq!(ordered, paths, "左半区应插到目标前面");
    });
    drag_tab(cx, "document-tab-2", "document-tab-0", true);
    editor.read_with(cx, |editor, _| {
        let ordered: Vec<_> = editor.workspace.open_documents.iter()
            .map(|tab| tab.path.clone()).collect();
        assert_eq!(
            ordered,
            vec![paths[0].clone(), paths[2].clone(), paths[1].clone()],
            "向左拖动同样按目标右半区插到后面"
        );
    });
    drag_tab(cx, "document-tab-0", "document-tab-0", true);
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let first = cx.debug_bounds("document-tab-0").expect("待关闭标签");
    cx.simulate_click(
        gpui::point(first.right() - px(18.0), first.center().y),
        gpui::Modifiers::none(),
    );
    editor.read_with(cx, |editor, _| {
        let ordered: Vec<_> = editor.workspace.open_documents.iter()
            .map(|tab| tab.path.clone()).collect();
        assert_eq!(ordered, vec![paths[2].clone(), paths[1].clone()], "半区落点不能挡住关闭按钮");
    });
}

#[gpui::test]
async fn switching_tabs_preserves_each_documents_undo_history(cx: &mut TestAppContext) {
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "state-audit-undo");
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    fs::write(&alpha, "alpha\n").unwrap();
    fs::write(&beta, "beta\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx)
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.simulate_input("X");
    cx.run_until_parked();
    editor.update(cx, |editor, cx| editor.finalize_pending_undo_capture(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.buffer.text().starts_with('X'));
        assert!(
            !editor.undo_history.is_empty(),
            "前置：A 中已经有可撤销的编辑"
        );
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta, window, cx);
            editor.open_workspace_file(alpha, window, cx);
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "alpha\n",
        "切回后应能撤销切走前的编辑"
    );
}

#[gpui::test]
async fn switching_tabs_preserves_folded_sections(cx: &mut TestAppContext) {
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "state-audit-fold");
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    let hidden = (0..40)
        .map(|index| format!("隐藏的内容 {index}\n\n"))
        .collect::<String>();
    fs::write(
        &alpha,
        format!(
            "## 第一节\n\n{hidden}## 第二节\n\n{}",
            super::external_changes::long_markdown(150)
        ),
    )
    .unwrap();
    fs::write(&beta, "other\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx)
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    editor.update(cx, |editor, cx| {
        let heading = editor.document.root_blocks()[0].clone();
        editor.on_block_event(
            heading.clone(),
            &crate::components::BlockEvent::RequestToggleFold,
            cx,
        );
        assert!(heading.read(cx).folded, "前置：第一节已折叠");
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    editor.update(cx, |editor, cx| {
        editor.set_vertical_scroll_offset(px(-700.0), cx)
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    let offset = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta, window, cx);
            editor.open_workspace_file(alpha, window, cx);
        })
    });
    for frame in 0..8 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y),
            offset,
            "切回折叠文档的第 {frame} 帧不应滚动"
        );
    }
    editor.update(cx, |editor, cx| {
        let visible = editor
            .apply_heading_fold_filter(cx)
            .into_iter()
            .map(|index| {
                editor.document.visible_blocks()[index as usize]
                    .entity
                    .read(cx)
                    .display_text()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert!(
            !visible.iter().any(|text| text.contains("隐藏的内容")),
            "切回后折叠节不应展开，实测 {visible:?}"
        );
    });
}

#[gpui::test]
async fn switching_tabs_preserves_expanded_source_lines(cx: &mut TestAppContext) {
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "state-audit-long-line");
    let alpha = root.join("alpha.txt");
    let beta = root.join("beta.md");
    fs::write(&alpha, format!("head\n{}\ntail\n", "a ".repeat(1800))).unwrap();
    fs::write(&beta, "other\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx)
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    editor.update(cx, |editor, cx| {
        let block = editor.document.root_blocks()[0].clone();
        block.update(cx, |block, cx| {
            block.long_line_plan();
            assert!(
                block.toggle_long_line_expanded(1),
                "前置：第二行足够长，可展开"
            );
            cx.notify();
        });
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta, window, cx);
            editor.open_workspace_file(alpha, window, cx);
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.root_blocks()[0].read(cx);
        let lines = block
            .last_layout
            .as_ref()
            .expect("切回首帧应已完成源码排版");
        assert!(
            !lines[1].wrap_boundaries().is_empty(),
            "切回后已展开的源码长行应继续换行显示"
        );
    });
}

#[gpui::test]
async fn switching_tabs_preserves_table_column_geometry(cx: &mut TestAppContext) {
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "state-table-columns");
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    let source = format!(
        "| id | 描述 | 状态 |\n| --- | --- | --- |\n| 1 | {} | 正常 |\n",
        "很长的说明文字。".repeat(30)
    );
    fs::write(&alpha, source).unwrap();
    fs::write(&beta, "other\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.simulate_resize(gpui::size(px(1100.0), px(800.0)));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.workspace.is_open = true;
            editor.open_workspace_file(alpha.clone(), window, cx);
        })
    });
    for _ in 0..5 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    let widths = |cx: &gpui::VisualTestContext| {
        editor.read_with(cx, |editor, cx| {
            let table = editor.document.root_blocks()[0].read(cx);
            table
                .table_runtime
                .as_ref()
                .unwrap()
                .header
                .iter()
                .map(|cell| {
                    let bounds = cell.read(cx).last_bounds.expect("前置：表头已经完成布局");
                    (bounds.left(), bounds.size.width)
                })
                .collect::<Vec<_>>()
        })
    };
    let before = widths(cx);
    let before_layout = editor.read_with(cx, |editor, cx| {
        editor.document.root_blocks()[0]
            .read(cx)
            .column_layout_memo
            .as_ref()
            .unwrap()
            .layout
            .clone()
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta, window, cx)
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    // update 结束会处理 canvas 通知引起的重绘，切换回调内守住首轮列宽比例。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha, window, cx)
        });
        let table = editor.read(cx).document.root_blocks()[0].clone();
        table.update(cx, |table, cx| {
            drop(gpui::Render::render(table, window, cx));
            assert_eq!(
                table.column_layout_memo.as_ref().unwrap().layout,
                before_layout,
                "首轮布局不能使用估算列宽"
            );
        });
    });
    for frame in 0..8 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let after = widths(cx);
        assert_eq!(after, before, "切回第 {frame} 帧表格列宽不应变化");
    }
}

#[gpui::test]
async fn switching_tabs_isolates_undo_and_redo_for_both_documents(cx: &mut TestAppContext) {
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "state-history-isolation");
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    fs::write(&alpha, "alpha\n").unwrap();
    fs::write(&beta, "beta\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    for (path, input) in [(&alpha, "新"), (&beta, "文")] {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(path.clone(), window, cx)
            })
        });
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        cx.simulate_input(input);
        cx.run_until_parked();
        editor.update(cx, |editor, cx| editor.finalize_pending_undo_capture(cx));
    }
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "beta\n"
    );
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx)
        })
    });
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "alpha\n",
        "B 的编辑不能替换 A 的撤销栈"
    );
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta, window, cx)
        })
    });
    editor.update(cx, |editor, cx| editor.redo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "文beta\n",
        "B 的重做必须保留"
    );
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha, window, cx)
        })
    });
    editor.update(cx, |editor, cx| editor.redo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "新alpha\n",
        "A 的重做必须独立保留"
    );
}

#[gpui::test]
async fn switching_tabs_starts_a_new_typing_group_after_returning(cx: &mut TestAppContext) {
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "state-typing-groups");
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    fs::write(&alpha, "alpha\n").unwrap();
    fs::write(&beta, "other\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx)
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.simulate_input("X");
    cx.run_until_parked();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta, window, cx);
            editor.open_workspace_file(alpha, window, cx);
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.simulate_input("Y");
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.finalize_pending_undo_capture(cx);
        editor.undo_document(cx);
    });
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "Xalpha\n",
        "切回后的输入应单独撤销"
    );
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "alpha\n",
        "切走前的输入仍应可撤销"
    );
}

#[gpui::test]
async fn externally_rewritten_tabs_discard_incompatible_history(cx: &mut TestAppContext) {
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "state-external-history");
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    fs::write(&alpha, "alpha\n").unwrap();
    fs::write(&beta, "other\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha.clone(), window, cx)
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.simulate_input("X");
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.finalize_pending_undo_capture(cx);
        assert!(
            !editor.undo_history.is_empty(),
            "前置：存在旧内容的撤销记录"
        );
        editor.document_dirty = false;
    });
    fs::write(&alpha, "Xalpha\n").unwrap();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta, window, cx)
        })
    });
    fs::write(&alpha, "磁盘新内容\n").unwrap();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha, window, cx)
        })
    });
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        "磁盘新内容\n",
        "不能把旧字节区间重放到外部改写后的内容上"
    );
}

#[gpui::test]
async fn switching_tabs_remeasures_columns_when_layout_inputs_change(cx: &mut TestAppContext) {
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "state-table-invalidation");
    let alpha = root.join("alpha.md");
    let beta = root.join("beta.md");
    fs::write(
        &alpha,
        format!(
            "| id | 描述 | 状态 |\n| --- | --- | --- |\n| 1 | {} | 正常 |\n",
            "较长的正文说明。".repeat(30)
        ),
    )
    .unwrap();
    fs::write(&beta, "other\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.simulate_resize(gpui::size(px(1100.0), px(800.0)));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root, cx);
            editor.workspace.is_open = true;
            editor.open_workspace_file(alpha.clone(), window, cx);
        })
    });
    for _ in 0..5 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    for change in ["窗口宽度", "主题度量"] {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(beta.clone(), window, cx)
            })
        });
        if change == "窗口宽度" {
            cx.simulate_resize(gpui::size(px(1700.0), px(800.0)));
        } else {
            cx.update(|_, cx| {
                let mut theme = cx.global::<crate::theme::ThemeManager>().current().clone();
                theme.dimensions.table_cell_padding_x += 8.0;
                cx.global_mut::<crate::theme::ThemeManager>()
                    .set_theme(theme);
            });
        }
        for _ in 0..5 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(alpha.clone(), window, cx)
            });
            let (table, width) = editor.read_with(cx, |editor, cx| {
                let theme = cx.global::<crate::theme::ThemeManager>().derived_theme(cx);
                let column = Editor::writing_column_width(
                    f32::from(editor.scroll_handle.bounds().size.width),
                    &theme.dimensions,
                    cx,
                );
                (
                    editor.document.root_blocks()[0].clone(),
                    (column - theme.dimensions.block_padding_x * 2.0) * 0.95,
                )
            });
            table.update(cx, |table, cx| {
                let theme = cx.global::<crate::theme::ThemeManager>().derived_theme(cx);
                let expected = crate::components::TableColumnLayout::measure(
                    table.record.table.as_ref().unwrap(),
                    width,
                    window,
                    &theme,
                    cx,
                );
                drop(gpui::Render::render(table, window, cx));
                assert_eq!(
                    table.column_layout_memo.as_ref().unwrap().layout,
                    expected,
                    "{change} 改变后不能沿用旧列宽"
                );
            });
        });
        for _ in 0..5 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }
}

#[gpui::test]
fn native_document_tabs_preserve_keyboard_selection_and_close(cx: &mut TestAppContext) {
    use gpui::{AppContext as _, VisualTestContext};
    use gpui_kit::test::TestWindowExt as _;
    init_review_workspace_app(cx);
    let root = review_workspace_fixture(cx, "native-document-tabs")
        .canonicalize()
        .unwrap();
    let _config = crate::config::override_test_config_root(root.join("config"));
    let config = crate::config::VeloraConfigDirs::from_system()
        .unwrap()
        .app_config_file();
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(config, "[editor]\nui_font_size = 21\n").unwrap();
    cx.update(|cx| crate::config::EditorSettings::init(cx, true));
    let paths = [root.join("alpha.md"), root.join("beta.md")];
    fs::write(&paths[0], "alpha\n").unwrap();
    fs::write(&paths[1], "beta\n").unwrap();
    let handle = cx.update(|cx| {
        crate::ui::open_window(Default::default(), cx, |_, cx| {
            cx.new(|cx| Editor::from_markdown(cx, String::new(), None))
        })
        .unwrap()
    });
    handle
        .update(cx, |editor, window, cx| {
            window.activate_window();
            for path in &paths {
                editor.open_workspace_file(path.clone(), window, cx);
            }
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    for _ in 0..2 {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    }
    let first = (
        "document-tab",
        super::super::stable_node_hash(&paths[0].to_string_lossy()),
    );
    let second = (
        "document-tab",
        super::super::stable_node_hash(&paths[1].to_string_lossy()),
    );
    visual.update(|window, _| {
        assert_eq!(
            window.try_find(first).and_then(|row| row.role()),
            Some(gpui::Role::Tab)
        );
        assert_eq!(
            window.find(first).bounds().size.height,
            px(47.0),
            "标签必须填满 48px 标题栏扣除边框后的内容区"
        );
    });
    let tab_focus = handle
        .read_with(&visual, |editor, _| {
            editor.workspace.tabs_focus.clone().expect("标签焦点")
        })
        .unwrap();
    visual.update(|window, cx| {
        assert_eq!(window.find(second).selected(), Some(true));
        window.blur(cx);
        window.focus_next(cx);
        for _ in 0..32 {
            window.press("tab", cx);
            window.render_frame(cx);
            if tab_focus.is_focused(window) {
                break;
            }
        }
        assert!(
            tab_focus.is_focused(window),
            "标签组应可由 Tab 进入当前标签"
        );
        window.press("left", cx);
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(first).selected(), Some(true));
        assert!(tab_focus.is_focused(window), "方向键切换后焦点仍留在标签组");
        window.press("tab", cx);
        window.render_frame(cx);
        assert_eq!(
            window.find(("document-tab-close", first.1)).role(),
            Some(gpui::Role::Button)
        );
        assert_eq!(
            window.find(("document-tab-close", first.1)).focused(),
            Some(true)
        );
        window.press("space", cx);
    });
    visual.run_until_parked();
    handle
        .read_with(&visual, |editor, _| {
            assert_eq!(editor.workspace.open_documents.len(), 1);
            assert_eq!(editor.workspace.active_document.as_ref(), Some(&paths[1]));
            assert_eq!(editor.buffer.text(), "beta\n");
        })
        .unwrap();
    handle.update(&mut visual, |editor, window, cx| {
        editor.open_workspace_file(paths[0].clone(), window, cx);
    }).unwrap();
    visual.run_until_parked();
    let inactive = visual.update(|window, cx| {
        window.render_frame(cx);
        window.find(second).bounds().center()
    });
    visual.simulate_mouse_down(inactive, gpui::MouseButton::Middle, gpui::Modifiers::none());
    visual.simulate_mouse_up(inactive, gpui::MouseButton::Middle, gpui::Modifiers::none());
    visual.run_until_parked();
    handle.read_with(&visual, |editor, _| {
        assert_eq!(editor.workspace.open_documents.len(), 1);
        assert_eq!(editor.workspace.active_document.as_ref(), Some(&paths[0]));
        assert_eq!(editor.buffer.text(), "alpha\n");
    }).unwrap();
}
