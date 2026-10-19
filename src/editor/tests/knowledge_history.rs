use super::common::*;

#[gpui::test]
async fn review_history_snapshots_keep_crlf_bytes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-history-shape-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    cx.on_quit({ let root = root.clone(); move || fs::remove_dir_all(root).unwrap() });
    let _config_guard = crate::config::override_test_config_root(root.join("config"));
    let path = root.join("notes.md"); fs::write(&path, "当前\r\n正文\r\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "当前\n正文\n".into(), Some(path.clone())));
    cx.update(|window, cx| editor.update(cx, |editor, cx| {
        editor.attach_file_origin(fs::read(&path).unwrap());
        editor.mark_dirty(cx);
        assert!(editor.save_to_existing_path(&path, window, cx));
    }));
    cx.run_until_parked();
    let version = crate::config::list_file_history(&path).remove(0);
    assert_eq!(fs::read(version).unwrap(), fs::read(&path).unwrap());
}

#[gpui::test]
async fn review_history_restore_keeps_crlf_shape(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-history-restore-shape-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    cx.on_quit({ let root = root.clone(); move || fs::remove_dir_all(root).unwrap() });
    let path = root.join("notes.md"); fs::write(&path, "当前\r\n正文\r\n").unwrap();
    let _config_guard = crate::config::override_test_config_root(root.join("config"));
    crate::config::record_file_history(&path, "旧\r\n正文\r\n").unwrap();
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "当前\n正文\n".into(), Some(path.clone())));
    editor.update(cx, |editor, cx| {
        editor.attach_file_origin(fs::read(&path).unwrap());
        editor.open_file_history(cx);
        editor.restore_file_history_version(0, cx);
    });
    editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.buffer.file_bytes(), "旧\r\n正文\r\n".as_bytes());
        assert_eq!(editor.buffer.text(), "旧\n正文\n");
    });
}

#[gpui::test]
async fn modal_blocks_document_input_and_keyboard_can_choose_cancel(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let choice = std::rc::Rc::new(std::cell::Cell::new(None));
    init_editor_test_app(cx);
    let handle = cx
        .update(|cx| {
            crate::ui::open_window(gpui::WindowOptions::default(), cx, |_, cx| {
                cx.new(|cx| Editor::from_markdown(cx, "正文保持原样".into(), None))
            })
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    redraw(&mut visual);
    let document_focus = handle
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
    visual.update(|window, cx| document_focus.focus(window, cx));
    handle
        .update(&mut visual, |editor, _, cx| {
            let choice = choice.clone();
            editor.show_modal(
                crate::editor::modal::ModalSpec {
                    title: "确认修改".into(),
                    detail: Some("文档正文".into()),
                    buttons: vec!["应用".into(), "取消".into(), "查看详情".into()],
                    default_index: 0,
                    cancel_index: 1,
                },
                move |index, _, _, _| choice.set(Some(index)),
                cx,
            );
        })
        .unwrap();
    redraw(&mut visual);
    assert!(
        !visual.update(|window, _| document_focus.is_focused(window)),
        "打开模态后不能继续聚焦正文"
    );
    visual.simulate_input("误改");
    assert_eq!(
        handle
            .read_with(&visual, |editor, _| editor.buffer.text())
            .unwrap(),
        "正文保持原样"
    );
    visual.simulate_keystrokes("tab");
    visual.update(|window, _| {
        let button = window.find(("editor-modal-button", 1usize));
        assert_eq!(button.role(), Some(gpui::Role::Button));
        assert_eq!(button.focused(), Some(true), "Tab 应能选择取消按钮");
    });
    visual.simulate_keystrokes("shift-tab");
    visual.update(|window, _| {
        assert_eq!(
            window.find(("editor-modal-button", 0usize)).focused(),
            Some(true)
        );
    });
    visual.simulate_keystrokes("tab");
    visual.simulate_keystrokes("enter");
    redraw(&mut visual);
    assert_eq!(
        choice.get(),
        Some(1),
        "回车应激活聚焦按钮而非总是执行默认按钮"
    );
    assert!(visual.update(|window, _| document_focus.is_focused(window)));
    handle
        .update(&mut visual, |editor, _, cx| {
            editor.show_message_modal("第一次提示", "", cx)
        })
        .unwrap();
    redraw(&mut visual);
    handle
        .update(&mut visual, |editor, _, cx| {
            editor.show_message_modal("替换后的提示", "", cx)
        })
        .unwrap();
    redraw(&mut visual);
    visual.simulate_keystrokes("escape");
    redraw(&mut visual);
    assert!(
        visual.update(|window, _| document_focus.is_focused(window)),
        "替换模态应保留最初的返回焦点"
    );
}

#[gpui::test]
async fn modal_keeps_long_details_and_actions_inside_a_small_window(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    cx.simulate_resize(gpui::size(px(640.0), px(360.0)));
    editor.update(cx, |editor, cx| {
        editor.show_message_modal("操作失败", "错误详情\n".repeat(200), cx)
    });
    redraw(cx);
    let panel = cx.debug_bounds("editor-modal").unwrap();
    let button = cx.debug_bounds("editor-modal-button-0").unwrap();
    assert!(
        panel.top() >= px(0.0) && panel.bottom() <= px(360.0),
        "长详情不得把弹窗撑出窗口：{panel:?}"
    );
    assert!(button.top() >= panel.top() && button.bottom() <= panel.bottom());
}



#[gpui::test]
async fn modal_in_a_split_keeps_window_geometry_and_the_owning_editor(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    init_editor_test_app(cx);
    let path = temp_markdown_path("modal-split");
    std::fs::write(&path, "分屏正文").unwrap();
    cx.on_quit({
        let path = path.clone();
        move || std::fs::remove_file(path).unwrap()
    });
    let handle = cx
        .update(|cx| {
            crate::ui::open_window(gpui::WindowOptions::default(), cx, |_, cx| {
                cx.new(|cx| Editor::from_markdown(cx, String::new(), None))
            })
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    handle
        .update(&mut visual, |editor, window, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        })
        .unwrap();
    redraw(&mut visual);
    handle
        .update(&mut visual, |editor, window, cx| {
            editor.split_active_document(
                crate::editor::workspace::split::SplitDirection::Right,
                window,
                cx,
            );
        })
        .unwrap();
    redraw(&mut visual);
    redraw(&mut visual);
    let groups = handle
        .read_with(&visual, |editor, _| editor.split_groups())
        .unwrap();
    assert_eq!(groups.len(), 2);
    let active = handle
        .read_with(&visual, |editor, _| editor.active_split_editor().unwrap())
        .unwrap();
    let focus = active.read_with(&visual, |editor, cx| {
        editor
            .document
            .first_root()
            .unwrap()
            .read(cx)
            .focus_handle
            .clone()
    });
    visual.update(|window, cx| focus.focus(window, cx));
    let completed = std::rc::Rc::new(std::cell::Cell::new(None));
    active.update(&mut visual, |editor, cx| {
        let completed = completed.clone();
        editor.show_modal(
            crate::editor::modal::ModalSpec {
                title: "确认分屏文档".into(),
                detail: None,
                buttons: vec!["应用".into(), "取消".into()],
                default_index: 0,
                cancel_index: 1,
            },
            move |_, _, _, cx| completed.set(Some(cx.entity_id())),
            cx,
        );
    });
    redraw(&mut visual);
    let panel = visual.debug_bounds("editor-modal").unwrap();
    let width = visual.update(|window, _| window.viewport_size().width);
    assert!(
        (panel.center().x - width / 2.0).abs() <= px(1.0),
        "分屏弹窗应以整窗居中：{panel:?}"
    );
    visual.simulate_input("误改");
    for group in &groups {
        assert_eq!(
            group.read_with(&visual, |editor, _| editor.buffer.text()),
            "分屏正文"
        );
    }
    visual.update(|window, cx| window.click(("editor-modal-button", 0usize), cx));
    redraw(&mut visual);
    assert_eq!(completed.get(), Some(active.entity_id()));
    assert!(visual.update(|window, _| focus.is_focused(window)));

    completed.set(None);
    let left = groups.first().unwrap();
    for group in [&active, left] {
        group.update(&mut visual, |editor, cx| {
            let completed = completed.clone();
            editor.show_modal(
                crate::editor::modal::ModalSpec {
                    title: "确认文档".into(),
                    detail: None,
                    buttons: vec!["应用".into(), "取消".into()],
                    default_index: 0,
                    cancel_index: 1,
                },
                move |_, _, _, cx| completed.set(Some(cx.entity_id())),
                cx,
            );
        });
        redraw(&mut visual);
    }
    let latest_button = visual.debug_bounds("editor-modal-button-0").unwrap();
    visual.simulate_click(latest_button.center(), Modifiers::none());
    redraw(&mut visual);
    assert_eq!(
        completed.get(),
        Some(left.entity_id()),
        "后出现的确认必须同时接收指针和键盘焦点"
    );
    assert!(active.read_with(&visual, |editor, _| editor.modal_is_open()));
    visual.simulate_keystrokes("escape");
    redraw(&mut visual);
    assert_eq!(completed.get(), Some(active.entity_id()));
    assert!(!active.read_with(&visual, |editor, _| editor.modal_is_open()));
    assert!(visual.update(|window, _| focus.is_focused(window)));
}

#[gpui::test]
async fn in_app_modal_buttons_close_it_and_run_the_callback(cx: &mut TestAppContext) {
    // 用户要求：全软件不用系统原生弹窗。模态必须可点、可关、回调拿到正确序号。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# 标题\n".into(), None));
    editor.update(cx, |editor, cx| {
        editor.show_modal(
            crate::editor::modal::ModalSpec {
                title: "确认删除".into(),
                detail: Some("note.md".into()),
                buttons: vec!["删除".into(), "取消".into()],
                default_index: 0,
                cancel_index: 1,
            },
            |choice, editor, _window, cx| {
                if choice == 0 {
                    // 用一个只有单个按钮的模态标记「回调确实按 choice 跑了」。
                    editor.show_message_modal("已删除", "", cx);
                }
            },
            cx,
        );
    });
    redraw(cx);
    assert!(cx.debug_bounds("editor-modal-button-0").is_some(), "模态应渲染第一个按钮");
    assert!(cx.debug_bounds("editor-modal-button-1").is_some(), "模态应渲染取消按钮");

    let first = cx.debug_bounds("editor-modal-button-0").expect("button 0");
    cx.simulate_click(first.center(), Modifiers::none());
    redraw(cx);
    assert!(
        cx.debug_bounds("editor-modal-button-1").is_none(),
        "点「删除」后旧模态应关闭，回调里新开的模态只有一个按钮"
    );
    assert!(cx.debug_bounds("editor-modal-button-0").is_some());

    let second = cx.debug_bounds("editor-modal-button-0").expect("button of second modal");
    cx.simulate_click(second.center(), Modifiers::none());
    redraw(cx);
    assert!(cx.debug_bounds("editor-modal-button-0").is_none());
    editor.read_with(cx, |editor, _| assert!(!editor.modal_is_open()));
}

#[gpui::test]
async fn in_app_modal_backdrop_click_cancels(cx: &mut TestAppContext) {
    // 取消语义：点遮罩 = 按取消位（键盘 Esc/Enter 另计，见 roadmap 后续项）。
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "# 标题\n".into(), None));
    editor.update(cx, |editor, cx| {
        editor.show_modal(
            crate::editor::modal::ModalSpec {
                title: "确认".into(),
                detail: None,
                buttons: vec!["确定".into(), "取消".into()],
                default_index: 0,
                cancel_index: 1,
            },
            |choice, editor, _window, cx| {
                if choice != 1 {
                    editor.show_message_modal("不该发生", "", cx);
                }
            },
            cx,
        );
    });
    redraw(cx);
    assert!(cx.debug_bounds("editor-modal-button-1").is_some());

    // 遮罩左上角（面板之外）按下 = 取消。
    cx.simulate_click(gpui::point(px(6.0), px(6.0)), Modifiers::none());
    redraw(cx);
    assert!(
        cx.debug_bounds("editor-modal-button-0").is_none(),
        "点遮罩应关闭模态，且回调按取消位走（不再开新模态）"
    );
    editor.read_with(cx, |editor, _| assert!(!editor.modal_is_open()));
}

#[gpui::test]
async fn modal_enter_triggers_default_and_escape_cancels(cx: &mut TestAppContext) {
    // C13：模态支持键盘。Enter = 默认按钮，Esc = 取消位。
    // 实现挂在编辑器按键捕获钩子上（模态不抢焦点），所以先聚焦一个块
    // 让按键有派发路径。
    use std::cell::RefCell;
    use std::rc::Rc;

    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "alpha".to_string(), None)
    });
    editor.update(cx, |editor, _cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
    });
    redraw(cx);

    let choice: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

    // Enter = 默认按钮（删除）。
    let sink = choice.clone();
    editor.update(cx, |editor, cx| {
        editor.show_modal(
            crate::editor::modal::ModalSpec {
                title: "确认删除".into(),
                detail: None,
                buttons: vec!["删除".into(), "取消".into()],
                default_index: 0,
                cancel_index: 1,
            },
            move |index, _editor, _window, _cx| {
                *sink.borrow_mut() = Some(index);
            },
            cx,
        );
    });
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(editor.modal_is_open(), "前置：模态应已打开");
    });
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(*choice.borrow(), Some(0), "Enter 应触发默认按钮");
    editor.read_with(cx, |editor, _| {
        assert!(!editor.modal_is_open(), "Enter 后模态应关闭");
    });

    // Esc = 取消位（取消）。
    let sink = choice.clone();
    editor.update(cx, |editor, cx| {
        editor.show_modal(
            crate::editor::modal::ModalSpec {
                title: "确认删除".into(),
                detail: None,
                buttons: vec!["删除".into(), "取消".into()],
                default_index: 0,
                cancel_index: 1,
            },
            move |index, _editor, _window, _cx| {
                *sink.borrow_mut() = Some(index);
            },
            cx,
        );
    });
    redraw(cx);
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert_eq!(*choice.borrow(), Some(1), "Esc 应触发取消位");
    editor.read_with(cx, |editor, _| {
        assert!(!editor.modal_is_open(), "Esc 后模态应关闭");
    });
}

#[gpui::test]
async fn typing_wikilink_opens_completion_and_enter_inserts_target(cx: &mut TestAppContext) {
    // [[ 补全端到端：输入 [[ 弹浮层 → 查询过滤 → Enter 插入 stem+]]。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-wlc-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("alpha.md"), "").expect("write alpha");
    std::fs::write(root.join("beta.md"), "# beta\n").expect("write beta");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();
    let alpha = root.join("alpha.md");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(alpha, window, cx);
        });
    });

    // 聚焦首块后输入 "a[[be"：浮层出现且过滤到 beta。
    editor.update(cx, |editor, _cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        editor.focus_block(block.entity_id());
    });
    redraw(cx);
    cx.simulate_input("a[[be");
    redraw(cx);
    assert!(
        cx.debug_bounds("wikilink-completion").is_some(),
        "输入 [[查询 后应出现补全浮层"
    );
    assert!(
        cx.debug_bounds("wikilink-entry-0").is_some(),
        "应过滤出 beta"
    );
    assert!(
        cx.debug_bounds("wikilink-entry-1").is_none(),
        "alpha 不匹配 be 查询"
    );

    // Enter 插入 "beta]]" 且不换行。
    cx.simulate_keystrokes("enter");
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.read(cx);
        assert_eq!(block.display_text(), "a[[beta]]", "Enter 应插入 stem 与收尾");
        assert!(block.cursor_offset() >= 9, "光标应停在 ]] 之后");
    });
    assert!(
        cx.debug_bounds("wikilink-completion").is_none(),
        "插入后浮层应关闭"
    );

    // Esc 关闭：重新输入 [[ 后按 Esc。
    cx.simulate_input(" [[");
    redraw(cx);
    assert!(cx.debug_bounds("wikilink-completion").is_some());
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(
        cx.debug_bounds("wikilink-completion").is_none(),
        "Esc 应关闭浮层"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// 插入表格对话框的步进器与 TableData::new_empty 都要有规模上限：
/// 每格是一个块实体，无上限的 `+` 连点到 200×200 会同步建 4 万个块，
/// 前台长时间冻结（评审 v0.2.6 维度二建议 22）。
#[test]
fn table_insert_dialog_steppers_saturate_at_safe_bounds() {
    use crate::components::markdown::table::TableData;
    let mut dialog = crate::editor::context_menu::TableInsertDialogState {
        target: crate::editor::context_menu::TableInsertTarget::Append,
        body_rows: TableData::MAX_CREATED_ROWS - 1,
        columns: TableData::MAX_CREATED_COLUMNS - 1,
    };
    dialog.increment_rows();
    dialog.increment_rows();
    assert_eq!(
        dialog.body_rows,
        TableData::MAX_CREATED_ROWS,
        "行步进应停在上限"
    );
    dialog.increment_columns();
    dialog.increment_columns();
    assert_eq!(
        dialog.columns,
        TableData::MAX_CREATED_COLUMNS,
        "列步进应停在上限"
    );

    // new_empty 兜同一层：超大入参不再建巨表。
    let table = TableData::new_empty(200, 200);
    assert_eq!(table.rows.len(), TableData::MAX_CREATED_ROWS);
    assert_eq!(table.header.len(), TableData::MAX_CREATED_COLUMNS);
}

/// 历史版本文件名是纳秒时间戳（记录端为防同毫秒覆盖用 `as_nanos()` 命名），
/// 浮层换算成 Unix 秒再格式化；曾按毫秒换算，`1.7e18 / 1000` 秒被当日期，
/// 浮层每行日期都是垃圾，无法按时间挑版本。
#[test]
fn file_history_version_label_parses_nanosecond_stems() {
    // 1791676800 秒 = 2026-10-11 00:00 UTC。
    let label = super::super::file_history::file_history_version_label("1791676800000000000")
        .expect("纳秒文件名应能换算出标签");
    let timestamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_791_676_800);
    let local: chrono::DateTime<chrono::Local> = timestamp.into();
    assert_eq!(label, local.format("%Y-%m-%d %H:%M").to_string());

    // 非数字文件名（人工放置的文件）回退原样显示，不参与换算。
    assert_eq!(super::super::file_history::file_history_version_label("notes"), None);
}

#[test]
fn file_history_records_dedupes_and_prunes() {
    // 存储约定：时间戳命名、同内容去重、每文件保留最近 30 条。
    let root = std::env::temp_dir().join(format!("velora-fhist-{}", temp_fixture_token()));
    let _ = std::fs::remove_dir_all(&root);
    let file = root.join("doc.md");
    std::fs::create_dir_all(&root).expect("create root");

    // 记录走的是 config 全局目录；测试构建用进程级临时配置根
    // （VeloraConfigDirs::from_system 的 test 分支），不能直接指定 root，
    // 所以这里只验证去重与上限行为，目录隔离交给测试根。
    for index in 0..35 {
        let content = format!("版本 {index}");
        crate::config::record_file_history(&file, &content).expect("record");
    }
    let versions = crate::config::list_file_history(&file);
    assert_eq!(versions.len(), 30, "超出上限应裁剪到 30 条");
    let newest = std::fs::read_to_string(&versions[0]).expect("read newest");
    assert_eq!(newest, "版本 34", "最新一条应是最后一次保存的内容");
    let oldest = std::fs::read_to_string(&versions.last().unwrap()).expect("read oldest");
    assert_eq!(oldest, "版本 5", "最老的 0..=4 应被裁掉");

    // 同内容再保存不重复落盘。
    crate::config::record_file_history(&file, "版本 34").expect("record dup");
    assert_eq!(crate::config::list_file_history(&file).len(), 30);
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
async fn command_palette_accepts_non_ascii_typing(cx: &mut TestAppContext) {
    // 审查发现：命令面板只接 on_key_down 且限定 is_ascii_graphic，中文用户
    // 在 ⇧⌘P 里打字没反应，退格删标量不删字素。
    init_editor_test_app(cx);
    cx.update(|cx| crate::app_menu::init(cx));
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# a\n".into(), None));
    cx.update(|window, cx| {
        window.activate_window();
        editor.update(cx, |editor, cx| editor.toggle_command_palette(window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.command_palette.is_some(), "前置：面板已打开");
    });
    cx.simulate_input("中文");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.command_palette.as_ref().expect("面板").input.read(cx).value().as_ref(),
            "中文",
            "命令面板必须能吃输入法/非 ASCII 文本"
        );
    });
}

#[gpui::test]
async fn inserting_a_table_through_the_dialog_can_be_undone(cx: &mut TestAppContext) {
    // 审查发现：表格插入对话框不进撤销栈（其它表格操作都进），Ctrl+Z 要么什么
    // 都不做，要么把之前的一次编辑一并撤掉。
    init_editor_test_app(cx);
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# a\n\nbody\n".into(), None));
    redraw(cx);
    let before = editor.read_with(cx, |editor, cx| editor.current_document_source(cx));
    editor.update(cx, |editor, cx| {
        editor.table_insert_dialog = Some(crate::editor::context_menu::TableInsertDialogState {
            target: crate::editor::context_menu::TableInsertTarget::Append,
            body_rows: 1,
            columns: 2,
        });
        assert!(editor.insert_table_from_dialog(cx), "对话框应插入表格");
    });
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.current_document_source(cx).contains('|'),
            "前置：表格已插入"
        );
    });
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.current_document_source(cx),
            before,
            "⌘Z 应撤销表格插入"
        );
    });
}

#[gpui::test]
async fn escape_dismisses_the_info_dialog(cx: &mut TestAppContext) {
    // 审查发现：信息弹窗（关于/检查更新）没有键盘路径也没有遮罩点击，Esc 关不掉。
    init_editor_test_app(cx);
    cx.update(|cx| crate::app_menu::init(cx));
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# a\n".into(), None));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.update(cx, |editor, cx| {
        editor.show_info_dialog(crate::editor::InfoDialogKind::About, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.info_dialog.is_some(), "前置：弹窗已打开");
    });
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.info_dialog.is_none(), "Esc 应关掉信息弹窗");
    });
}

#[gpui::test]
async fn escape_closes_the_in_window_menu_bar(cx: &mut TestAppContext) {
    // 审查发现：标题栏菜单面板没有键盘路径，Esc 关不掉（只能等 hover 超时或点正文）。
    init_editor_test_app(cx);
    cx.update(|cx| crate::app_menu::init(cx));
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# a\n".into(), None));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.update(cx, |editor, _cx| {
        editor.menu_bar_open = Some(0);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.menu_bar_open.is_some(), "前置：菜单已打开");
    });
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.menu_bar_open.is_none(), "Esc 应关掉标题栏菜单");
    });
}

#[gpui::test]
async fn closing_quick_open_restores_focus_to_the_document(cx: &mut TestAppContext) {
    // 审查发现：关掉 ⌘P 只丢状态不还焦点，之后敲字全丢（要再手点正文）。
    init_editor_test_app(cx);
    cx.update(|cx| crate::app_menu::init(cx));
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# a\n\nbody\n".into(), None));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let block_id = editor.read_with(cx, |editor, _| {
        editor.document.root_blocks()[1].entity_id()
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let block = editor
                .document
                .block_entity_by_id(block_id)
                .expect("目标块");
            block.read(cx).focus_handle.clone().focus(window, cx);
            editor.toggle_quick_open(window, cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.quick_open.is_some(), "前置：⌘P 已打开");
    });
    // Esc 关闭（走全局 DismissTransientUi 路径）。
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        editor.read_with(cx, |editor, cx| {
            assert!(editor.quick_open.is_none(), "前置：⌘P 已关闭");
            let block = editor
                .document
                .block_entity_by_id(block_id)
                .expect("目标块");
            assert!(
                block.read(cx).focus_handle.is_focused(window),
                "关闭 ⌘P 后焦点应回到正文块"
            );
        });
    });
}

#[gpui::test]
async fn file_history_restore_can_be_undone(cx: &mut TestAppContext) {
    // 恢复历史版本曾清空 undo 栈，模块注释承诺的「可撤销」落了空——
    // 误按 Enter 就丢掉当前未保存内容且无法撤回。
    init_editor_test_app(cx);
    let path = temp_markdown_path("file-history-undo");
    std::fs::write(&path, "当前内容").expect("seed current");
    crate::config::record_file_history(&path, "旧版本内容").expect("record older");
    let history_files = crate::config::list_file_history(&path);
    let cleanup_path = path.clone();
    cx.on_quit(move || {
        let _ = std::fs::remove_file(cleanup_path);
        for file in history_files {
            let _ = std::fs::remove_file(file);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown("当前内容".to_string(), Some(path.clone()), cx);
    });
    redraw(cx);

    editor.update(cx, |editor, cx| {
        editor.open_file_history(cx);
        editor.restore_file_history_version(0, cx);
    });
    editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.current_document_source(cx),
            "旧版本内容",
            "恢复后正文应是历史内容"
        );
        assert!(editor.document_dirty, "恢复是未保存修改");
    });

    // ⌘Z 应回到恢复前的内容。
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.current_document_source(cx),
            "当前内容",
            "恢复历史版本必须可撤销，⌘Z 回到恢复前内容"
        );
    });
}

#[gpui::test]
async fn file_history_overlay_restores_version_as_unsaved_edit(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let path = temp_markdown_path("file-history");
    std::fs::write(&path, "第一版内容").expect("seed v1");

    // 直接落两条历史（记录路径已有单测），浮层走真实数据。
    crate::config::record_file_history(&path, "第一版内容").expect("record v1");
    crate::config::record_file_history(&path, "第二版内容").expect("record v2");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown("当前编辑内容".to_string(), Some(path.clone()), cx);
    });
    redraw(cx);

    // 打开历史浮层：两条版本，最新在前。
    editor.update(cx, |editor, cx| {
        editor.open_file_history(cx);
    });
    redraw(cx);
    assert!(cx.debug_bounds("file-history-entry-0").is_some(), "应有版本行");
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.file_history_overlay.as_ref().expect("浮层应打开").selected,
            0,
            "默认选中最新"
        );
    });

    // ↓ 选上一版，Enter 请求确认后再恢复为未保存修改。
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    assert!(editor.read_with(cx, |editor, _| editor.modal_is_open()));
    editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.current_document_source(cx).contains("第一版内容"),
            "恢复选中版本内容"
        );
        assert!(editor.document_dirty, "恢复后应为未保存状态");
        assert!(!editor.file_history_is_open(), "恢复后浮层关闭");
    });
    let _ = std::fs::remove_file(&path);
}
