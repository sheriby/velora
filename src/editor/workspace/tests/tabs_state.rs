use super::super::Editor;
use super::tabs_sessions::{init_review_workspace_app, review_workspace_fixture};
use gpui::{TestAppContext, px};
use std::fs;

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
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
    }
    editor.update(cx, |editor, cx| {
        editor.set_vertical_scroll_offset(px(-700.0), cx)
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear());
    }
    let offset = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta, window, cx);
            editor.open_workspace_file(alpha, window, cx);
        })
    });
    for frame in 0..8 {
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
    }
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(beta, window, cx);
            editor.open_workspace_file(alpha, window, cx);
        })
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
            cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
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
            cx.update(|window, cx| window.draw(cx).clear());
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
            cx.update(|window, cx| window.draw(cx).clear());
        }
    }
}
