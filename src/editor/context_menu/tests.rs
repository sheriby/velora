mod tests {
    use super::super::{
        ContextMenuState, DocumentSubmenu, Editor, TableInsertTarget, reveal_command_spec,
    };
    use gpui::{AppContext, Point, TestAppContext, px};

    #[test]
    fn reveal_command_targets_the_file_on_macos_and_windows_dir_on_linux() {
        let path = std::path::Path::new("/tmp/docs/pic.png");
        let (program, args) = reveal_command_spec(path);
        #[cfg(target_os = "macos")]
        {
            assert_eq!(program, "open");
            assert_eq!(args, vec!["-R".to_string(), "/tmp/docs/pic.png".to_string()]);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert_eq!(program, "xdg-open");
            assert_eq!(args, vec!["/tmp/docs".to_string()]);
        }
        #[cfg(windows)]
        {
            assert_eq!(program, "explorer");
            assert_eq!(args, vec!["/select,/tmp/docs/pic.png".to_string()]);
        }
    }

    /// 悬停展开二级菜单：从父行移向二级面板要穿过一段空隙，这期间两处都不悬停，
    /// 但面板得等到定时器到点才收；期间换悬停另一行，展开的那块跟着换。
    #[gpui::test]
    async fn context_submenu_stays_open_while_crossing_hover_gap(cx: &mut TestAppContext) {
        let editor = cx.new(|cx| Editor::from_markdown(cx, "alpha".to_string(), None));
        editor.update(cx, |editor, cx| {
            editor.open_insert_context_menu(
                Point {
                    x: px(24.0),
                    y: px(24.0),
                },
                TableInsertTarget::Append,
                cx,
            );
        });

        let opened = |cx: &mut TestAppContext| -> Option<DocumentSubmenu> {
            editor.read_with(cx, |editor, _| {
                let Some(ContextMenuState::Document {
                    open_submenu, ..
                }) = editor.context_menu.as_ref()
                else {
                    panic!("应当正开着正文右键菜单");
                };
                *open_submenu
            })
        };
        let hover = |hovered: bool, submenu: DocumentSubmenu, cx: &mut TestAppContext| {
            editor.update(cx, |editor, cx| {
                editor.set_document_menu_hover(hovered, Some(submenu), cx)
            });
        };

        hover(true, DocumentSubmenu::Insert, cx);
        assert_eq!(opened(cx), Some(DocumentSubmenu::Insert));
        assert!(
            editor.read_with(cx, |editor, _| editor.context_menu_submenu_close_task.is_none()),
            "刚悬停上就挂上了关闭定时器"
        );

        // 离开父行、还没进二级面板：空隙里保持展开，只挂定时器。
        hover(false, DocumentSubmenu::Insert, cx);
        assert_eq!(opened(cx), Some(DocumentSubmenu::Insert));
        assert!(
            editor.read_with(cx, |editor, _| editor.context_menu_submenu_close_task.is_some()),
            "移入空隙时该挂上延时关闭的定时器"
        );

        hover(true, DocumentSubmenu::Insert, cx);
        assert_eq!(opened(cx), Some(DocumentSubmenu::Insert));
        assert!(
            editor.read_with(cx, |editor, _| editor.context_menu_submenu_close_task.is_none()),
            "回到面板上却没撤掉定时器"
        );

        hover(false, DocumentSubmenu::Insert, cx);
        cx.executor().advance_clock(std::time::Duration::from_millis(150));
        cx.run_until_parked();
        assert_eq!(opened(cx), None, "两处都离开满 120ms 之后二级面板该收掉");

        // 换一行悬停：展开的那块跟着换，不会两块同时开着。
        hover(true, DocumentSubmenu::Format, cx);
        hover(true, DocumentSubmenu::Paragraph, cx);
        assert_eq!(opened(cx), Some(DocumentSubmenu::Paragraph));
    }
}
