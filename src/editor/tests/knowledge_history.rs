use super::common::*;

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
async fn knowledge_panels_list_backlinks_and_tags_end_to_end(cx: &mut TestAppContext) {
    // 反链 + 标签面板端到端：树落地后索引重建，面板列条目，点击生效。
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-km-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("a.md"), "见 [[target]] #rust\n").expect("write a");
    std::fs::write(root.join("b.md"), "#rust 和 #gpui，与目标无关\n").expect("write b");
    std::fs::write(root.join("target.md"), "# target\n").expect("write target");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace_link_index.tracked_file_count(),
            3,
            "树落地后索引应包含全部 Markdown 文件"
        );
    });

    let target_path = root.join("target.md");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(target_path, window, cx);
        });
    });

    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.set_workspace_tab(crate::editor::workspace::WorkspaceTab::Backlinks, cx);
    });
    redraw(cx);
    assert!(
        cx.debug_bounds("backlink-entry-0").is_some(),
        "反链面板应列出 a.md"
    );
    assert!(
        cx.debug_bounds("backlink-entry-1").is_none(),
        "无关文件不应出现"
    );
    let row = cx.debug_bounds("backlink-entry-0").expect("row");
    cx.simulate_click(row.center(), gpui::Modifiers::none());
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor
                .file_path
                .as_ref()
                .map(|path| path.file_name().unwrap().to_string_lossy().to_string()),
            Some("a.md".to_string()),
            "点击反链条目应打开对应笔记"
        );
    });

    editor.update(cx, |editor, cx| {
        editor.set_workspace_tab(crate::editor::workspace::WorkspaceTab::Tags, cx);
    });
    redraw(cx);
    let first = cx.debug_bounds("tag-entry-0").expect("tag row 0");
    let second = cx.debug_bounds("tag-entry-1").expect("tag row 1");
    assert!(first.origin.y <= second.origin.y, "#rust 应排在 #gpui 前");
    cx.simulate_click(first.center(), gpui::Modifiers::none());
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.active_tab,
            crate::editor::workspace::WorkspaceTab::Search
        );
        assert_eq!(editor.workspace.search_query, "#rust");
    });
    let _ = std::fs::remove_dir_all(root);
}

#[gpui::test]
async fn tags_panel_lists_workspace_tags_and_click_starts_search(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = std::env::temp_dir().join(format!("velora-tags-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::write(root.join("a.md"), "#rust 笔记\n").expect("write a");
    std::fs::write(root.join("b.md"), "也是 #rust 和 #gpui\n").expect("write b");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();
    cx.run_until_parked();

    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.set_workspace_tab(crate::editor::workspace::WorkspaceTab::Tags, cx);
    });
    redraw(cx);

    // #rust 两个文件引用排第一，#gpui 一个排第二。
    let first = cx.debug_bounds("tag-entry-0").expect("tag row 0");
    let second = cx.debug_bounds("tag-entry-1").expect("tag row 1");
    assert!(first.origin.y <= second.origin.y, "#rust 应排在 #gpui 前");

    // 点击标签 → 进入搜索 tab，query 已填 #rust。
    cx.simulate_click(first.center(), gpui::Modifiers::none());
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.workspace.active_tab, crate::editor::workspace::WorkspaceTab::Search);
        assert_eq!(editor.workspace.search_query, "#rust");
    });
    let _ = std::fs::remove_dir_all(root);
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

#[test]
fn file_history_records_dedupes_and_prunes() {
    // 存储约定：时间戳命名、同内容去重、每文件保留最近 20 条。
    let root = std::env::temp_dir().join(format!("velora-fhist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let file = root.join("doc.md");
    std::fs::create_dir_all(&root).expect("create root");

    // 记录走的是 config 全局目录；测试构建用进程级临时配置根
    // （VeloraConfigDirs::from_system 的 test 分支），不能直接指定 root，
    // 所以这里只验证去重与上限行为，目录隔离交给测试根。
    for index in 0..25 {
        let content = format!("版本 {index}");
        crate::config::record_file_history(&file, &content).expect("record");
    }
    let versions = crate::config::list_file_history(&file);
    assert_eq!(versions.len(), 20, "超出上限应裁剪到 20 条");
    let newest = std::fs::read_to_string(&versions[0]).expect("read newest");
    assert_eq!(newest, "版本 24", "最新一条应是最后一次保存的内容");
    let oldest = std::fs::read_to_string(&versions.last().unwrap()).expect("read oldest");
    assert_eq!(oldest, "版本 5", "最老的 0..=4 应被裁掉");

    // 同内容再保存不重复落盘。
    crate::config::record_file_history(&file, "版本 24").expect("record dup");
    assert_eq!(crate::config::list_file_history(&file).len(), 20);
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
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.command_palette.is_some(), "前置：面板已打开");
    });
    cx.simulate_input("中文");
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.command_palette.as_ref().expect("面板").query,
            "中文",
            "命令面板必须能吃输入法/非 ASCII 文本"
        );
    });
}

#[gpui::test]
async fn editing_a_code_file_updates_the_link_index(cx: &mut TestAppContext) {
    // 审查发现：全量索引含代码文件（collect_workspace_files），增量重扫却只收
    // Markdown，代码文件里的 [[链接]] 外部改动后永远停在旧状态。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-code-index-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    // 与索引产出的 canonical 路径对齐（macOS /var → /private/var）。
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let active = root.join("a.md");
    let code = root.join("notes.rs");
    fs::write(&active, "# A\n").unwrap();
    fs::write(&code, "// [[a]]\n").unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(root.clone(), cx);
            editor.workspace.is_open = true;
            editor.open_workspace_file(active.clone(), window, cx);
        });
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| editor.refresh_link_panels(cx));
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.link_panels.backlinks.iter().any(|path| path == &code),
            "前置：代码文件里的 [[a]] 已被全量索引"
        );
    });

    fs::write(&code, "// 链接已删\n").unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&code, cx));
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| editor.refresh_link_panels(cx));
    editor.read_with(cx, |editor, _| {
        assert!(
            !editor.link_panels.backlinks.iter().any(|path| path == &code),
            "代码文件的外部改动也要更新索引，不能一直显示旧反链"
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
    cx.update(|window, cx| window.draw(cx).clear());
    editor.update(cx, |editor, cx| {
        editor.show_info_dialog(crate::editor::InfoDialogKind::About, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.info_dialog.is_some(), "前置：弹窗已打开");
    });
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear());
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
    cx.update(|window, cx| window.draw(cx).clear());
    editor.update(cx, |editor, _cx| {
        editor.menu_bar_open = Some(0);
    });
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.menu_bar_open.is_some(), "前置：菜单已打开");
    });
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear());
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
    cx.update(|window, cx| window.draw(cx).clear());
    let block_id = editor.read_with(cx, |editor, _| {
        editor.document.root_blocks()[1].entity_id()
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let block = editor
                .document
                .block_entity_by_id(block_id)
                .expect("目标块");
            window.focus(&block.read(cx).focus_handle);
            editor.toggle_quick_open(window, cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.quick_open.is_some(), "前置：⌘P 已打开");
    });
    // Esc 关闭（走全局 DismissTransientUi 路径）。
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| window.draw(cx).clear());
    cx.update(|window, cx| window.draw(cx).clear());
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
    // 用户报修（审查发现）：恢复历史版本会清空 undo 栈，模块注释承诺的
    // 「可撤销」是假的——误按 Enter 就丢掉当前未保存内容且无法撤回。
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

    // ↓ 选上一版，Enter 恢复为未保存修改。
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
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
