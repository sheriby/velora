mod tests {
    use super::reveal_command_spec;

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

    use super::super::{ContextMenuState, Editor, TableInsertTarget};
    use gpui::{AppContext, Point, TestAppContext, px};

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

            editor.set_context_menu_hover_state(true, false, cx);
            let Some(ContextMenuState::Insert { submenu_open, .. }) = editor.context_menu.as_ref()
            else {
                panic!("expected insert context menu");
            };
            assert!(*submenu_open);
            assert!(editor.context_menu_submenu_close_task.is_none());

            editor.set_context_menu_hover_state(false, false, cx);
            let Some(ContextMenuState::Insert { submenu_open, .. }) = editor.context_menu.as_ref()
            else {
                panic!("expected insert context menu");
            };
            assert!(*submenu_open);
            assert!(editor.context_menu_submenu_close_task.is_some());

            editor.set_context_menu_hover_state(true, true, cx);
            let Some(ContextMenuState::Insert { submenu_open, .. }) = editor.context_menu.as_ref()
            else {
                panic!("expected insert context menu");
            };
            assert!(*submenu_open);
            assert!(editor.context_menu_submenu_close_task.is_none());
        });
    }
}

/// 「在文件管理器中显示」的平台命令：程序名 + 参数（纯数据便于测试）。
pub(crate) fn reveal_command_spec(path: &std::path::Path) -> (&'static str, Vec<String>) {
    #[cfg(target_os = "macos")]
    {
        ("open", vec!["-R".to_string(), path.display().to_string()])
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = path
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| path.to_path_buf());
        ("xdg-open", vec![dir.display().to_string()])
    }
    #[cfg(windows)]
    {
        (
            "explorer",
            vec![format!("/select,{}", path.display())],
        )
    }
}
