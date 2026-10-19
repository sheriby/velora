use super::common::*;

fn exercise_pending_decision_cancel(kind: &str, cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let root =
        std::env::temp_dir().join(format!("velora-pending-decision-{}", uuid::Uuid::new_v4()));
    let _config = crate::config::override_test_config_root(root.join("profile"));
    std::fs::create_dir_all(root.join("folder")).unwrap();
    std::fs::write(root.join("incoming.md"), "传入内容\n").unwrap();
    cx.on_quit({
        let root = root.clone();
        move || std::fs::remove_dir_all(root).unwrap()
    });
    init_editor_test_app(cx);
    let handle = cx
        .update(|cx| {
            crate::ui::open_window(gpui::WindowOptions::default(), cx, |_, cx| {
                cx.new(|cx| Editor::from_markdown(cx, "未保存的内容".into(), None))
            })
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    redraw(&mut visual);
    let focus = handle
        .read_with(&visual, |editor, cx| {
            editor
                .document
                .first_root()
                .unwrap()
                .read(cx)
                .focus_handle
                .clone()
        })
        .unwrap();
    visual.update(|window, cx| focus.focus(window, cx));
    handle
        .update(&mut visual, |editor, window, cx| {
            editor.mark_dirty(cx);
            match kind {
                "close" => assert!(!editor.on_window_should_close(window, cx)),
                "drop" => {
                    editor.request_dropped_markdown_replace(root.join("incoming.md"), window, cx)
                }
                "folder" => {
                    editor.pending_folder_choice = Some(root.join("folder"));
                    cx.notify();
                }
                _ => unreachable!(),
            }
        })
        .unwrap();
    redraw(&mut visual);
    assert!(
        visual.update(|window, cx| gpui_kit::base::active_focus_trap(window, cx).is_some()),
        "{kind} 确认需要可操作的模态焦点范围"
    );
    if kind == "drop" {
        handle
            .update(&mut visual, |editor, _, cx| {
                editor.show_message_modal("读取失败", "请重试", cx)
            })
            .unwrap();
        redraw(&mut visual);
        visual.update(|window, cx| window.click(("editor-modal-button", 0usize), cx));
        redraw(&mut visual);
        assert!(
            handle
                .read_with(&visual, |editor, _| editor.show_drop_replace_dialog)
                .unwrap(),
            "错误提示关闭后仍须能处理原决定"
        );
    }
    visual.simulate_keystrokes("tab");
    let cancel = match kind {
        "close" => "cancel-close-dialog",
        "drop" => "cancel-drop-replace-dialog",
        "folder" => "folder-choice-cancel",
        _ => unreachable!(),
    };
    visual.update(|window, _| {
        let button = window.find(cancel);
        assert_eq!(button.role(), Some(gpui::Role::Button));
        assert_eq!(button.focused(), Some(true));
    });
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    handle
        .read_with(&visual, |editor, _| {
            assert_eq!(editor.buffer.text(), "未保存的内容");
            assert!(editor.document_dirty);
            assert!(!editor.show_unsaved_changes_dialog);
            assert!(!editor.show_drop_replace_dialog);
            assert!(editor.pending_folder_choice.is_none());
            assert!(editor.file_path.is_none());
        })
        .unwrap();
    assert!(visual.update(|window, _| focus.is_focused(window)));
    assert_eq!(
        std::fs::read_to_string(root.join("incoming.md")).unwrap(),
        "传入内容\n"
    );
    assert_eq!(visual.cx.windows().len(), 1);
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn pending_decision_close_supports_keyboard_cancel(cx: &mut TestAppContext) {
    exercise_pending_decision_cancel("close", cx);
}

#[gpui::test]
fn pending_decision_drop_supports_keyboard_cancel(cx: &mut TestAppContext) {
    exercise_pending_decision_cancel("drop", cx);
}

#[gpui::test]
fn pending_decision_folder_supports_keyboard_cancel(cx: &mut TestAppContext) {
    exercise_pending_decision_cancel("folder", cx);
}

#[gpui::test]
fn pending_decision_save_and_replace_preserves_disk_shape(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let root = std::env::temp_dir().join(format!("velora-decision-save-{}", uuid::Uuid::new_v4()));
    let _config = crate::config::override_test_config_root(root.join("profile"));
    std::fs::create_dir_all(&root).unwrap();
    let current = root.join("current.md");
    let incoming = root.join("incoming.md");
    std::fs::write(&current, b"original\r\n").unwrap();
    std::fs::write(&incoming, b"incoming\r\n").unwrap();
    cx.on_quit({
        let root = root.clone();
        move || std::fs::remove_dir_all(root).unwrap()
    });
    init_editor_test_app(cx);
    let loaded = crate::editor::encoding::load_document(&current).unwrap();
    let handle = cx
        .update(|cx| {
            crate::ui::open_window(gpui::WindowOptions::default(), cx, {
                let current = current.clone();
                move |_, cx| {
                    cx.new(move |cx| Editor::from_loaded_document(cx, loaded, Some(current)))
                }
            })
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    redraw(&mut visual);
    let focus = handle
        .read_with(&visual, |editor, cx| {
            editor
                .document
                .first_root()
                .unwrap()
                .read(cx)
                .focus_handle
                .clone()
        })
        .unwrap();
    visual.update(|window, cx| {
        window.activate_window();
        focus.focus(window, cx);
    });
    visual.simulate_input("修改 ");
    redraw(&mut visual);
    handle
        .update(&mut visual, |editor, window, cx| {
            editor.request_dropped_markdown_replace(incoming.clone(), window, cx)
        })
        .unwrap();
    redraw(&mut visual);
    visual.update(|window, cx| window.click("save-and-replace-drop-dialog", cx));
    redraw(&mut visual);
    assert_eq!(
        std::fs::read(&current).unwrap(),
        "修改 original\r\n".as_bytes()
    );
    assert_eq!(std::fs::read(&incoming).unwrap(), b"incoming\r\n");
    handle
        .read_with(&visual, |editor, _| {
            assert_eq!(editor.buffer.text(), "incoming\n");
            assert_eq!(
                crate::file_url::canonical_path(editor.file_path.as_ref().unwrap()),
                crate::file_url::canonical_path(&incoming)
            );
            assert!(!editor.document_dirty);
            assert!(!editor.show_drop_replace_dialog);
        })
        .unwrap();
    visual.update(|window, _| window.remove_window());
}

#[gpui::test]
fn pending_decision_folder_actions_preserve_the_document_and_window_target(
    cx: &mut TestAppContext,
) {
    use gpui_kit::test::TestWindowExt as _;
    let root =
        std::env::temp_dir().join(format!("velora-decision-folder-{}", uuid::Uuid::new_v4()));
    let _config = crate::config::override_test_config_root(root.join("profile"));
    std::fs::create_dir_all(root.join("folder")).unwrap();
    let folder = crate::file_url::canonical_path(&root.join("folder"));
    cx.on_quit({
        let root = root.clone();
        move || std::fs::remove_dir_all(root).unwrap()
    });
    init_editor_test_app(cx);
    for new_window in [false, true] {
        let handle = cx
            .update(|cx| {
                crate::ui::open_window(gpui::WindowOptions::default(), cx, |_, cx| {
                    cx.new(|cx| Editor::from_markdown(cx, "保留原文".into(), None))
                })
            })
            .unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        redraw(&mut visual);
        handle
            .update(&mut visual, |editor, _, cx| {
                editor.mark_dirty(cx);
                editor.pending_folder_choice = Some(folder.clone());
                cx.notify();
            })
            .unwrap();
        redraw(&mut visual);
        visual.update(|window, cx| {
            window.click(
                if new_window {
                    "folder-choice-new-window"
                } else {
                    "folder-choice-replace"
                },
                cx,
            )
        });
        redraw(&mut visual);
        handle
            .read_with(&visual, |editor, _| {
                assert_eq!(editor.buffer.text(), "保留原文");
                assert!(editor.document_dirty);
                assert!(editor.pending_folder_choice.is_none());
                assert_eq!(
                    editor
                        .workspace_root_path()
                        .map(std::path::Path::to_path_buf),
                    (!new_window).then(|| folder.clone())
                );
            })
            .unwrap();
        let windows = visual.cx.windows();
        assert_eq!(windows.len(), if new_window { 2 } else { 1 });
        if new_window {
            let other = *windows
                .iter()
                .find(|window| window.window_id() != handle.window_id())
                .unwrap();
            let content = visual
                .update(|_, cx| crate::ui::window_content::<Editor>(other, cx))
                .unwrap();
            assert_eq!(
                content
                    .read_with(&visual, |editor, _| editor
                        .workspace_root_path()
                        .map(std::path::Path::to_path_buf))
                    .unwrap(),
                Some(folder.clone())
            );
        }
        for window in windows {
            visual
                .cx
                .update_window(window, |_, window, _| window.remove_window())
                .unwrap();
        }
    }
}

#[gpui::test]
async fn export_html_writes_rendered_document_without_changing_editor_state(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("rendered-export-html", "html");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# Title\n\nbody".to_string(), None)
    });

    editor.update(cx, |editor, cx| {
        editor.mark_dirty(cx);
        assert!(editor.document_dirty);
        assert!(editor.file_path.is_none());
        editor
            .export_document_to_path(ExportFormat::Html, &export_path, cx)
            .expect("html export should write");
        assert!(editor.document_dirty);
        assert!(editor.file_path.is_none());
    });

    let html = fs::read_to_string(&export_path).expect("read exported html");
    // 标题带 id：正文里的 `#锚点` 链接要在浏览器里跳得动（报修第 14 条）。
    assert!(html.contains("<h1 id=\"title\">Title</h1>"), "导出内容缺失：{html}");
    assert!(html.contains("<p>body</p>"));
}

/// 导出是「读文档」，不该为了渲染 HTML 先把整篇投影重拼一遍。
///
/// 大文档上这一遍是一次全文分配加几百毫秒，而它产出的 markdown 与缓冲区只差在写法
/// （下划线变星号、表格重新补白），HTML 里根本看不出来——纯付钱不拿货。
#[gpui::test]
async fn exporting_a_document_does_not_rereserialize_the_projection(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("export-reads-buffer", "html");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# Title\n\n_下划线_ body".to_string(), None)
    });
    redraw(cx);

    let before = editor.read_with(cx, |editor, _| editor.document.whole_document_renders.get());
    editor
        .update(cx, |editor, cx| {
            editor.export_document_to_path(ExportFormat::Html, &export_path, cx)
        })
        .expect("html export should write");
    let after = editor.read_with(cx, |editor, _| editor.document.whole_document_renders.get());
    assert_eq!(
        before, after,
        "导出又把整篇投影序列化了一遍（{before} → {after}）"
    );

    let html = fs::read_to_string(&export_path).expect("read exported html");
    assert!(
        html.contains("<h1 id=\"title\">Title</h1>"),
        "导出内容缺失：{html}"
    );
    assert!(
        html.contains("<em>下划线</em>"),
        "导出的正文不是文档本身：{html}"
    );
}

#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
async fn export_png_writes_long_image_without_changing_editor_state(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("rendered-export-png", "png");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# Title\n\nbody".to_string(), None)
    });

    let export_result = editor.update(cx, |editor, cx| {
        editor.mark_dirty(cx);
        let result = editor.export_document_to_path(ExportFormat::Png, &export_path, cx);
        assert!(editor.document_dirty);
        assert!(editor.file_path.is_none());
        result
    });

    match export_result {
        Ok(()) => {
            let png = fs::read(&export_path).expect("read exported png");
            assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        }
        // Chrome 缺失时只要求给出可行动的错误，磁盘上不产生半成品。
        Err(err) => {
            let message = err.to_string();
            assert!(
                message.contains("Chromium") || message.contains("Chrome"),
                "unexpected PNG export error: {message}"
            );
            assert!(!export_path.exists());
        }
    }
}

#[gpui::test]
async fn export_html_uses_source_mode_raw_text(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let export_path = temp_export_path("source-export-html", "html");
    let cleanup_path = export_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "rendered".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        let source_block = editor
            .document
            .first_root()
            .expect("source mode should keep one root block")
            .clone();
        source_block.update(cx, |block, _cx| {
            block.record.set_title(InlineTextTree::plain(
                "# Source\n\n<!--\n<strong>visible</strong>\n-->".to_string(),
            ));
            block.sync_render_cache();
        });
        // 源码模式的一次改动都要落到缓冲区（这一档没有区间可声明，由重投影落笔），
        // 导出读的就是缓冲区。只改投影不写回，等于伪造一个产品里不存在的状态。
        editor.mark_dirty(cx);
        editor
            .export_document_to_path(ExportFormat::Html, &export_path, cx)
            .expect("source html export should write");
    });

    let html = fs::read_to_string(&export_path).expect("read exported html");
    assert!(
        html.contains("<h1 id=\"source\">Source</h1>"),
        "源码模式的标题缺失：{html}"
    );
    assert!(html.contains("class=\"vlt-comment\""));
    assert!(html.contains("&lt;strong&gt;visible&lt;/strong&gt;"));
}

#[gpui::test]
async fn dropped_markdown_replaces_clean_editor_in_current_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let dropped_path = temp_markdown_path("drop-clean-replace");
    fs::write(
        &dropped_path,
        "# Dropped\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n",
    )
    .expect("write dropped markdown");
    let cleanup_path = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "old".to_string(), None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(editor.view_mode == ViewMode::Source);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path.clone(), window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&dropped_path));
        assert!(editor.view_mode == ViewMode::Rendered);
        assert!(!editor.document_dirty);
        assert!(!editor.show_drop_replace_dialog);
        assert_eq!(editor.document.root_count(), 2);
        assert_eq!(
            editor
                .document
                .root_blocks()
                .last()
                .expect("table block")
                .read(cx)
                .kind(),
            BlockKind::Table
        );
        assert!(editor.document.markdown_text(cx).contains("# Dropped"));
    });

    let fallback_source = "!!! note\n  keep this text";
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown(fallback_source.into(), None, cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        assert!(editor.source_mode_fallback_required);
        assert_eq!(editor.document.raw_source_text(cx), fallback_source);
    });
    redraw(cx);
    assert_eq!(cx.cx.windows().len(), 1);
}

#[gpui::test]
async fn dirty_drop_waits_for_replace_decision_and_cancel_preserves_document(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);

    let dropped_path = temp_markdown_path("drop-dirty-cancel");
    fs::write(&dropped_path, "dropped").expect("write dropped markdown");
    let cleanup_path = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "current".to_string(), None));
    editor.update(cx, |editor, cx| editor.mark_dirty(cx));

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path, window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(editor.show_drop_replace_dialog);
        assert_eq!(editor.document.markdown_text(cx), "current");
        assert!(editor.pending_drop_replace_path.is_some());
    });

    editor.update(cx, |editor, cx| editor.cancel_drop_replace_dialog(cx));

    editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        assert!(!editor.show_drop_replace_dialog);
        assert!(editor.pending_drop_replace_path.is_none());
        assert_eq!(editor.document.markdown_text(cx), "current");
    });
}

#[gpui::test]
async fn dirty_drop_can_replace_without_saving(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let dropped_path = temp_markdown_path("drop-dirty-discard");
    fs::write(&dropped_path, "dropped").expect("write dropped markdown");
    let cleanup_path = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_path);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "current".to_string(), None));
    editor.update(cx, |editor, cx| editor.mark_dirty(cx));

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path.clone(), window, cx);
            editor.discard_pending_drop_replace(window, cx);
        });
    });
    redraw(cx);

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&dropped_path));
        assert_eq!(editor.document.markdown_text(cx), "dropped");
        assert!(!editor.document_dirty);
        assert!(!editor.show_drop_replace_dialog);
    });
}

#[gpui::test]
async fn dirty_drop_saves_existing_document_before_replace(cx: &mut TestAppContext) {
    init_editor_test_app(cx);

    let current_path = temp_markdown_path("drop-save-current");
    let dropped_path = temp_markdown_path("drop-save-replace");
    fs::write(&current_path, "original").expect("write current markdown");
    fs::write(&dropped_path, "dropped").expect("write dropped markdown");
    let cleanup_current = current_path.clone();
    let cleanup_dropped = dropped_path.clone();
    cx.on_quit(move || {
        let _ = fs::remove_file(&cleanup_current);
        let _ = fs::remove_file(&cleanup_dropped);
    });

    let (editor, cx) = cx.add_window_view({
        let current_path = current_path.clone();
        move |_window, cx| Editor::from_markdown(cx, "original".to_string(), Some(current_path))
    });

    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("current root").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(InlineTextTree::plain("edited".to_string()));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.request_dropped_markdown_replace(dropped_path.clone(), window, cx);
            editor.save_and_replace_pending_drop(window, cx);
        });
    });
    redraw(cx);

    assert_eq!(
        fs::read_to_string(&current_path).expect("read saved current"),
        "edited"
    );
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.file_path.as_ref(), Some(&dropped_path));
        assert_eq!(editor.document.markdown_text(cx), "dropped");
        assert!(!editor.document_dirty);
        assert!(!editor.pending_drop_replace_after_save);
    });
}

