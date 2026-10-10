mod tests {
    use gpui::{AppContext, Bounds, Context, Modifiers, MouseButton, TestAppContext, point, px, size};

    use super::super::{CrossBlockSelection, CrossBlockSelectionEndpoint, Editor};
    use crate::components::{BlockKind, Cut, Undo, UndoCaptureKind};
    use crate::i18n::I18nManager;
    use crate::theme::ThemeManager;

    fn init_editor_test_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            I18nManager::init(cx);
            ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }

    fn redraw(cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }

    fn set_selection(
        editor: &mut Editor,
        start_index: usize,
        start_offset: usize,
        end_index: usize,
        end_offset: usize,
        cx: &mut Context<Editor>,
    ) {
        let visible = editor.document.visible_blocks().to_vec();
        let start = visible[start_index].entity.entity_id();
        let end = visible[end_index].entity.entity_id();
        editor.cross_block_selection = Some(CrossBlockSelection {
            anchor: CrossBlockSelectionEndpoint {
                entity_id: start,
                offset: start_offset,
            },
            focus: CrossBlockSelectionEndpoint {
                entity_id: end,
                offset: end_offset,
            },
        });
        editor.sync_cross_block_selection_visuals(cx);
    }

    fn assign_visible_block_bounds(editor: &mut Editor, cx: &mut Context<Editor>) {
        for (index, visible) in editor
            .document
            .visible_blocks()
            .to_vec()
            .into_iter()
            .enumerate()
        {
            visible.entity.update(cx, move |block, _cx| {
                block.last_bounds = Some(Bounds::new(
                    point(px(0.0), px(index as f32 * 32.0)),
                    size(px(400.0), px(24.0)),
                ));
            });
        }
    }

    #[test]
    fn mouse_down_starts_cross_block_drag_after_clearing_old_selection() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta\n\ngamma".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            assign_visible_block_bounds(editor, cx);
            set_selection(editor, 0, 0, 2, 2, cx);
            assert!(editor.cross_block_selection.is_some());
            assert!(
                editor
                    .document
                    .visible_blocks()
                    .iter()
                    .any(|visible| visible.entity.read(cx).editor_selection_range.is_some())
            );

            editor.begin_cross_block_drag_at_point(point(px(8.0), px(4.0)), cx);

            assert!(editor.cross_block_selection.is_none());
            assert!(editor.cross_block_drag.is_some());
            assert!(
                editor
                    .document
                    .visible_blocks()
                    .iter()
                    .all(|visible| visible.entity.read(cx).editor_selection_range.is_none())
            );
        });
        cx.quit();
    }

    #[test]
    fn typing_replaces_cross_block_selection_with_plain_text() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta\n\ngamma".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            set_selection(editor, 0, 2, 2, 2, cx);
            assert!(editor.replace_cross_block_selection_with_text(
                "X",
                None,
                false,
                UndoCaptureKind::CoalescibleText,
                cx
            ));

            assert_eq!(editor.document.markdown_text(cx), "alXmma");
            assert!(editor.cross_block_selection.is_none());
            assert!(editor.cross_block_drag.is_none());
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert_eq!(block.selected_range, 3..3);
            assert!(block.marked_range.is_none());
        });
        cx.quit();
    }

    /// 全选后粘纯文本：新首行不该继承旧标题的记号。
    ///
    /// 报修「使用体验」第 4 条：原文首段是 `# 标题` 时，选中全文再用纯文本替换，
    /// 结果第一行仍按标题保存——替换走的是「就地把字符换掉」，块记号没人重算。
    /// 口径：整篇重解析（`rebuild_document_from_buffer`），粘贴的内容里没有 `#`
    /// 就应当是段落。
    #[test]
    fn pasting_plain_text_over_the_whole_document_does_not_keep_the_heading() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "# 标题\n\n正文".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            set_selection(editor, 0, 0, 1, 2, cx);
            assert!(editor.replace_cross_block_selection_with_text(
                "第一行\n第二行",
                None,
                false,
                UndoCaptureKind::CoalescibleText,
                cx
            ));

            let markdown = editor.document.markdown_text(cx);
            assert!(!markdown.contains('#'), "旧标题记号留下来了：{markdown}");
            let visible = editor.document.visible_blocks();
            assert_eq!(
                visible[0].entity.read(cx).kind(),
                BlockKind::Paragraph,
                "粘贴的第一行仍按标题排版"
            );
        });
        cx.quit();
    }

    #[test]
    fn ime_composition_replaces_cross_block_selection_and_marks_inserted_text() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor =
            cx.new(|cx| Editor::from_markdown(cx, "alpha\n\nbeta\n\ngamma".to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            set_selection(editor, 0, 2, 2, 2, cx);
            assert!(editor.replace_cross_block_selection_with_text(
                "ni",
                Some(2..2),
                true,
                UndoCaptureKind::ImeComposition,
                cx
            ));

            assert_eq!(editor.document.markdown_text(cx), "alnimma");
            let block = editor.document.visible_blocks()[0].entity.read(cx);
            assert_eq!(block.selected_range, 4..4);
            assert_eq!(block.marked_range, Some(2..4));
            assert!(block.editor_selection_range.is_none());
        });
        cx.quit();
    }

    #[test]
    fn cross_block_selection_marks_visual_ranges_and_copies_markdown() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| {
            Editor::from_markdown(
                cx,
                "alpha **bold**\n\n- item\n\n![alt](image.png)".to_string(),
                None,
            )
        });

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let end_len = visible[2].entity.read(cx).visible_len();
            set_selection(editor, 0, 0, 2, end_len, cx);

            assert_eq!(
                editor.cross_block_selected_markdown(cx).as_deref(),
                Some("alpha **bold**\n\n- item\n\n![alt](image.png)")
            );
            for visible in visible {
                let block = visible.entity.read(cx);
                assert_eq!(block.editor_selection_range, Some(0..block.visible_len()));
            }
        });
        cx.quit();
    }

    #[test]
    fn cross_block_cut_writes_markdown_deletes_range_and_undo_restores() {
        let mut app_cx = TestAppContext::single();
        init_editor_test_app(&mut app_cx);
        let original = "alpha\n\nbeta\n\ngamma";
        let (editor, window_cx) = app_cx.add_window_view({
            let original = original.to_string();
            move |_window, cx| Editor::from_markdown(cx, original.clone(), None)
        });
        redraw(window_cx);

        editor.update(window_cx, |editor, cx| {
            set_selection(editor, 0, 2, 2, 2, cx);
            assert_eq!(
                editor.cross_block_selected_markdown(cx).as_deref(),
                Some("pha\n\nbeta\n\nga")
            );
        });
        redraw(window_cx);

        window_cx.dispatch_action(Cut);
        redraw(window_cx);

        assert_eq!(
            window_cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("pha\n\nbeta\n\nga")
        );
        assert_eq!(
            editor.read_with(window_cx, |editor, cx| editor.document.markdown_text(cx)),
            "almma"
        );

        window_cx.dispatch_action(Undo);
        redraw(window_cx);

        assert_eq!(
            editor.read_with(window_cx, |editor, cx| editor.document.markdown_text(cx)),
            original
        );
        editor.read_with(window_cx, |editor, cx| {
            assert_eq!(
                editor.cross_block_selected_markdown(cx).as_deref(),
                Some("pha\n\nbeta\n\nga")
            );
        });
        drop(editor);
        app_cx.quit();
    }

    /// README_CN.md 的「发布包」那两行，用户报修的那一段。
    const PACKAGE_SECTION: &str = "## 发布包\n\n- **macOS**——执行 `scripts/package-macos.sh`，生成 `dist/velora.app` 与 `dist/velora-0.1.0.pkg`。安装包未签名、未公证，仅用于本机与小范围内部试用。\n- **Windows**——`scripts/package-windows.sh` 在 macOS 上交叉构建 x64 安装器（需要 MinGW-w64 与 NSIS），生成 `dist/velora-0.1.0-windows-x64-setup.exe`。Windows 实机验收安排在下一版。\n\n## ⚙️ 配置\n\n正文。\n";

    /// 用户报修：两段全部选中之后按删除，文件里留下 `**`、`。` 这些半截记号。
    ///
    /// 根因是端点换算走了「插入点」那条口径：`**Windows**` 里第一个可见字符在文件里
    /// 停在第 2 个字节上，拿它当区间起点，开头那两个 `*` 就没被选走，删完留在文件里。
    #[test]
    fn cross_block_selection_over_two_list_items_keeps_the_whole_lines() {
        let mut app_cx = TestAppContext::single();
        init_editor_test_app(&mut app_cx);
        let (editor, window_cx) = app_cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, PACKAGE_SECTION.to_string(), None)
        });
        redraw(window_cx);

        editor.update(window_cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 5, "两个标题 + 两个列表项 + 正文");
            let item1 = visible[1].entity.clone();
            let item2 = visible[2].entity.clone();
            // 夹具里这两块都还没聚焦，显示文本就是干净文本，两种坐标同长。
            let item1_len = item1.read(cx).visible_len();
            let item2_len = item2.read(cx).visible_len();

            set_selection(editor, 1, 0, 2, item2_len, cx);

            // 高亮铺满两行：端点在干净坐标里，各块按自己的投影换成显示偏移。
            assert_eq!(item1.read(cx).editor_selection_range, Some(0..item1_len));
            assert_eq!(item2.read(cx).editor_selection_range, Some(0..item2_len));

            // 交出去的就是文件里那两行字节，连同行首的列表记号与 `**`。
            let copied = editor.cross_block_selected_markdown(cx).expect("选区文本");
            assert_eq!(
                copied,
                "- **macOS**——执行 `scripts/package-macos.sh`，生成 `dist/velora.app` 与 `dist/velora-0.1.0.pkg`。安装包未签名、未公证，仅用于本机与小范围内部试用。\n- **Windows**——`scripts/package-windows.sh` 在 macOS 上交叉构建 x64 安装器（需要 MinGW-w64 与 NSIS），生成 `dist/velora-0.1.0-windows-x64-setup.exe`。Windows 实机验收安排在下一版。"
            );

            assert!(editor.delete_cross_block_selection(cx));
            let text = editor.document.markdown_text(cx);
            assert!(!text.contains("macOS"), "第一行该整行没掉：{text:?}");
            assert!(!text.contains("Windows"), "第二行该整行没掉：{text:?}");
            assert!(!text.contains("下一版"), "尾巴该跟着走：{text:?}");
            assert!(!text.contains("- **"), "不该留下半截 `**`：{text:?}");
            assert!(text.contains("## 发布包"), "{text:?}");
            assert!(text.contains("## ⚙️ 配置"), "{text:?}");
        });
        drop(editor);
        app_cx.quit();
    }

    /// 用户报修的另一半：拖到行尾，最后两个字没高亮。
    ///
    /// 端点记的是拖动当刻那一段的显示偏移（`**` 还藏着，203 字节）；拖完之后光标落在
    /// 这一段里、`**` 显形，同一块变成 207。高亮照旧铺到 203，尾巴那 4 个字节就没铺
    /// 上（屏幕上就是最后两个字没高亮），按删除留下的字节也跟着对不上。
    #[test]
    fn cross_block_endpoints_survive_inline_delimiters_revealing() {
        let mut app_cx = TestAppContext::single();
        init_editor_test_app(&mut app_cx);
        let (editor, window_cx) = app_cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, PACKAGE_SECTION.to_string(), None)
        });
        redraw(window_cx);

        editor.update(window_cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            let item2 = visible[2].entity.clone();
            // 拖动当刻这一段的长度（`**` 还藏着，显示文本与干净文本同长）。
            let collapsed_len = item2.read(cx).visible_len();

            // 拖动当刻：光标还没进这一段，`**` 是藏着的。
            set_selection(editor, 1, 0, 2, collapsed_len, cx);

            // 拖完之后光标落在第二项里：`**` 显形，这一块的显示文本变长。
            item2.update(cx, |block, cx| {
                block.move_to(2, cx);
                block.sync_inline_projection_for_focus(true);
            });
            let revealed_len = item2.read(cx).visible_len();
            assert!(
                revealed_len > collapsed_len,
                "夹具前提：`**` 显形之后这一段更长（{collapsed_len} → {revealed_len}）"
            );
            editor.sync_cross_block_selection_visuals(cx);

            assert_eq!(
                item2.read(cx).editor_selection_range,
                Some(0..revealed_len),
                "高亮要铺到行尾：显形出来的记号也在选区内"
            );

            let copied = editor.cross_block_selected_markdown(cx).expect("选区文本");
            assert!(copied.ends_with("下一版。"), "选区末尾到内容末尾：{copied:?}");

            assert!(editor.delete_cross_block_selection(cx));
            let text = editor.document.markdown_text(cx);
            assert!(!text.contains("下一版"), "块尾那一段该被删掉：{text:?}");
            assert!(text.contains("## ⚙️ 配置"), "后面的块不该被吃掉：{text:?}");
        });
        drop(editor);
        app_cx.quit();
    }

    /// 鼠标那条路径：从第一项块首按住往下拖过两行。命中测试给的是显示偏移，
    /// 存下来之前换算成干净偏移，否则「谁显形了」一变，同一段选区就跟着漂。
    #[test]
    fn dragging_over_two_list_items_selects_whole_lines() {
        let mut app_cx = TestAppContext::single();
        init_editor_test_app(&mut app_cx);
        let (editor, window_cx) = app_cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, PACKAGE_SECTION.to_string(), None)
        });
        redraw(window_cx);
        redraw(window_cx);

        let (start, end) = editor.read_with(window_cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            let first = visible[1].entity.read(cx).last_bounds.expect("第一项有布局");
            let last = visible[2].entity.read(cx).last_bounds.expect("第二项有布局");
            (
                point(first.left() + px(2.0), first.top() + px(2.0)),
                point(last.right() - px(2.0), last.bottom() + px(6.0)),
            )
        });
        window_cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        redraw(window_cx);
        window_cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        redraw(window_cx);
        window_cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        redraw(window_cx);

        editor.update(window_cx, |editor, cx| {
            let copied = editor.cross_block_selected_markdown(cx).expect("拖动之后有选区");
            assert!(copied.starts_with("- **macOS**"), "选区从行首起：{copied:?}");
            assert!(copied.ends_with("下一版。"), "选区到行尾止：{copied:?}");

            assert!(editor.delete_cross_block_selection(cx));
            let text = editor.document.markdown_text(cx);
            assert!(!text.contains("macOS") && !text.contains("Windows"), "{text:?}");
            assert!(text.contains("## ⚙️ 配置"), "{text:?}");
        });
        drop(editor);
        app_cx.quit();
    }


    const TABLE_DOC: &str = "alpha\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\ngamma";

    #[test]
    fn delete_selection_spanning_table_removes_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, TABLE_DOC.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let end_len = visible[2].entity.read(cx).visible_len();
            // The table sits in the interior of the selection.
            set_selection(editor, 0, 0, 2, end_len, cx);
            assert!(editor.delete_cross_block_selection(cx));

            let text = editor.document.markdown_text(cx);
            assert!(!text.contains('|'), "table should be gone: {text:?}");
            assert!(!text.contains("alpha"));
            assert!(!text.contains("gamma"));
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_with_trailing_table_removes_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, TABLE_DOC.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            assert_eq!(editor.document.visible_blocks().len(), 3);
            // Selection ends at the start of the table block: the previously
            // broken case where the trailing atomic block was left behind.
            set_selection(editor, 0, 0, 1, 0, cx);
            assert!(editor.delete_cross_block_selection(cx));

            // The table is removed in full; only `gamma` survives (deleting from
            // the document start leaves the table's trailing blank line, which
            // reparses to leading empty paragraphs, harmless and trimmable).
            let text = editor.document.markdown_text(cx);
            assert!(
                !text.contains('|'),
                "trailing table should be gone: {text:?}"
            );
            assert_eq!(text.trim(), "gamma");
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_of_only_table_removes_just_the_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, TABLE_DOC.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let alpha_len = visible[0].entity.read(cx).visible_len();
            // Drag from the end of the paragraph above onto the table: only the
            // table is removed, and re-parse normalizes the spacing.
            set_selection(editor, 0, alpha_len, 1, 0, cx);
            assert!(editor.delete_cross_block_selection(cx));

            assert_eq!(editor.document.markdown_text(cx), "alpha\n\ngamma");
        });
        cx.quit();
    }

    #[test]
    fn cut_selection_including_table_serializes_and_deletes_it() {
        // Exercise cut's two halves directly (the clipboard markdown and the
        // deleted source range) rather than dispatching the action, keeping this
        // a focused unit test of the cut logic.
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let editor = cx.new(|cx| Editor::from_markdown(cx, TABLE_DOC.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let end_len = visible[2].entity.read(cx).visible_len();
            set_selection(editor, 0, 0, 2, end_len, cx);

            // The clipboard markdown serializes the full table, matching what
            // delete removes; otherwise cut would drop it from the clipboard.
            let markdown = editor.cross_block_selected_markdown(cx).unwrap();
            assert!(markdown.contains("| a | b |"), "clipboard: {markdown:?}");
            assert!(markdown.contains("| 1 | 2 |"), "clipboard: {markdown:?}");
            assert!(markdown.contains("alpha") && markdown.contains("gamma"));

            assert!(editor.delete_cross_block_selection(cx));
            assert!(
                !editor.document.markdown_text(cx).contains('|'),
                "document should no longer contain the table"
            );
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_spanning_code_block_removes_it() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        // Code blocks edit their raw text, so they are deletable as an ordinary
        // text range; this documents that visible_len-based behavior.
        let doc = "alpha\n\n```\ncode\n```\n\ngamma";
        let editor = cx.new(|cx| Editor::from_markdown(cx, doc.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let end_len = visible[2].entity.read(cx).visible_len();
            set_selection(editor, 0, 0, 2, end_len, cx);
            assert!(editor.delete_cross_block_selection(cx));

            let text = editor.document.markdown_text(cx);
            assert!(
                !text.contains("code"),
                "code block should be gone: {text:?}"
            );
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_ending_on_trailing_empty_paragraph_removes_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let doc = "alpha\n\n| a | b |\n| --- | --- |\n| 1 | 2 |";
        let editor = cx.new(|cx| Editor::from_markdown(cx, doc.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            // Append a trailing empty paragraph, exactly as inserting a table at
            // the end of a document does. Ending the selection on it used to
            // abort deletion because empty roots had no source span.
            let empty =
                Editor::new_block(cx, crate::components::BlockRecord::paragraph(String::new()));
            let index = editor.document.root_count();
            editor
                .document
                .insert_blocks_at(None, index, vec![empty], cx);

            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            let alpha_len = visible[0].entity.read(cx).visible_len();
            // From the end of `alpha` onto the trailing empty paragraph.
            set_selection(editor, 0, alpha_len, 2, 0, cx);
            assert!(editor.delete_cross_block_selection(cx));

            let text = editor.document.markdown_text(cx);
            assert!(!text.contains('|'), "table should be gone: {text:?}");
            assert_eq!(text.trim(), "alpha");
        });
        cx.quit();
    }

    #[test]
    fn delete_selection_starting_on_empty_paragraph_removes_table() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let doc = "| a | b |\n| --- | --- |\n| 1 | 2 |\n\ngamma";
        let editor = cx.new(|cx| Editor::from_markdown(cx, doc.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            // Prepend a leading empty paragraph; starting the highlight on it used
            // to abort deletion (the user's "drag up from the text below into an
            // empty block above the table" case).
            let empty =
                Editor::new_block(cx, crate::components::BlockRecord::paragraph(String::new()));
            editor.document.insert_blocks_at(None, 0, vec![empty], cx);

            let visible = editor.document.visible_blocks().to_vec();
            assert_eq!(visible.len(), 3);
            // From the empty paragraph (index 0) to the start of `gamma`.
            set_selection(editor, 0, 0, 2, 0, cx);
            assert!(editor.delete_cross_block_selection(cx));


            let text = editor.document.markdown_text(cx);
            assert!(!text.contains('|'), "table should be gone: {text:?}");
            assert_eq!(text.trim(), "gamma");
        });
        cx.quit();
    }

    /// 跨块删除只能改写选区自己那一段字节。
    ///
    /// 这条断言管的是「删两段正文，凭什么把不相干的表格列宽填充和下划线写法
    /// 一起洗掉」：旧路径把新整篇文本 `edit(0..全文)` 写回缓冲区，再 `mark_dirty`
    /// 让重同步从块树整篇重新序列化，于是未编辑的块也被规范化，撤销组里还存着
    /// 一份全文副本。
    #[test]
    fn cross_block_delete_rewrites_only_the_selected_bytes() {
        let mut cx = TestAppContext::single();
        init_editor_test_app(&mut cx);
        let source = concat!(
            "第一段文字\n",
            "\n",
            "第二段文字\n",
            "\n",
            "| 名称 | 数量 |\n| ---- | ---- |\n| 甲   | 1    |\n",
            "\n",
            "强调 __下划线__ 结尾\n",
        );
        let editor = cx.new(|cx| Editor::from_markdown(cx, source.to_string(), None));

        editor.update(&mut cx, |editor, cx| {
            let second_len = editor.document.visible_blocks()[1]
                .entity
                .read(cx)
                .visible_len();
            set_selection(editor, 0, 0, 1, second_len, cx);
            let selection = editor.normalized_cross_block_selection(cx).unwrap();
            let range = editor
                .cross_block_source_range_for_normalized(selection, cx)
                .unwrap();
            // 区间写回的定义就是这次拼接：选区之外一个字节都不许变。
            let expected = format!("{}{}", &source[..range.start], &source[range.end..]);

            assert!(editor.delete_cross_block_selection(cx));

            let text = editor.buffer.text();
            assert_eq!(text, expected, "跨块删除改写了选区之外的字节：{text:?}");
            assert!(
                text.contains("| 甲   | 1    |"),
                "表格列宽填充被洗掉了：{text:?}"
            );
            assert!(
                text.contains("强调 __下划线__ 结尾"),
                "下划线强调被规范化成了别的写法：{text:?}"
            );

            let deleted = range.end - range.start;
            let stored = editor.undo_history_byte_len();
            assert!(
                stored <= deleted + 32,
                "撤销组存了整篇副本：删了 {deleted} 字节却记了 {stored} 字节"
            );
        });
        cx.quit();
    }

    /// 源码模式下回车新建的块：跨块选区必须铺到中间每一根块。空块没有字形，
    /// 指针换算与逐块铺展都容易把它们当成"不存在"，用户实测从 138 行拖到
    /// 148 行只有最后一块亮着。
    #[gpui::test]
    async fn source_mode_selection_covers_every_block(cx: &mut TestAppContext) {
        use crate::components::Newline;

        init_editor_test_app(cx);
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "第一段文字\n\n第二段文字".into(), None)
        });
        redraw(cx);
        editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
        redraw(cx);

        for label in ["AAA", "BBB", "CCC"] {
            cx.update(|window, cx| {
                let last = editor.read_with(cx, |editor, _cx| {
                    editor.document.root_blocks().last().cloned().expect("有根块")
                });
                editor.update(cx, |editor, _cx| editor.focus_block(last.entity_id()));
                last.update(cx, |block, cx| {
                    let tail = block.visible_len();
                    block.move_to(tail, cx);
                    block.on_newline(&Newline, window, cx);
                });
            });
            redraw(cx);
            cx.simulate_input(label);
            redraw(cx);
        }

        let roots = editor.read_with(cx, |editor, _cx| editor.document.root_blocks().len());
        assert_eq!(roots, 4, "夹具该有 4 根块（1 块原文 + 回车 3 块）");

        editor.update(cx, |editor, cx| {
            set_selection(editor, 0, 0, 3, 3, cx);
        });
        let selected = editor.read_with(cx, |editor, cx| {
            editor
                .document
                .root_blocks()
                .iter()
                .map(|block| {
                    block
                        .read(cx)
                        .editor_selection_range
                        .clone()
                        .filter(|range| range.start != range.end)
                        .map(|range| range.len())
                })
                .collect::<Vec<_>>()
        });
        assert!(
            selected.iter().all(|length| matches!(length, Some(n) if *n > 0)),
            "跨块选区没铺到每一根块上，选中长度={selected:?}"
        );
    }
}
