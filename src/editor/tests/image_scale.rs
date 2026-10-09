use super::common::*;
use gpui::{Entity, MouseButton, point};

fn open_image_menu(cx: &mut VisualTestContext) {
    let handle = cx
        .debug_bounds("image-resize-handle")
        .expect("图片缩放手柄");
    let position = point(handle.left() - px(12.0), handle.top() - px(12.0));
    cx.simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
    redraw(cx);
}

fn click_scale_row(name: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(name);
    assert!(bounds.is_some(), "右键菜单应提供图片缩放项 {name}");
    let bounds = bounds.expect("已验证菜单项存在");
    cx.simulate_click(bounds.center(), Modifiers::none());
    redraw(cx);
}

fn source(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.buffer.text())
}

#[gpui::test]
async fn image_scale_menu_stays_inside_the_viewport_when_opened_at_the_bottom_right(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "![图例](./missing.png)".to_string(), None)
    });
    redraw(cx);
    let viewport = cx.update(|window, _| window.viewport_size());
    editor.update_in(cx, |editor, _, cx| {
        let target = editor.document.first_root().expect("图片块").entity_id();
        editor.open_image_context_menu(
            point(viewport.width - px(1.0), viewport.height - px(1.0)),
            target,
            None,
            "./missing.png".to_string(),
            cx,
        );
    });
    redraw(cx);
    let panel = cx
        .debug_bounds("image-context-menu-panel")
        .expect("图片菜单面板");
    assert!(panel.left() >= px(0.0) && panel.top() >= px(0.0));
    assert!(panel.right() <= viewport.width && panel.bottom() <= viewport.height);
    let input = cx.debug_bounds("image-scale-input").expect("缩放输入框");
    cx.simulate_click(input.center(), Modifiers::none());
    redraw(cx);
    cx.simulate_input("999");
    click_scale_row("menu-item-image-scale-apply", cx);
    let panel = cx
        .debug_bounds("image-context-menu-panel")
        .expect("带错误提示的菜单面板");
    assert!(panel.right() <= viewport.width && panel.bottom() <= viewport.height);
}

#[gpui::test]
async fn scaling_a_small_image_changes_both_dimensions_proportionally(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let document_path = temp_markdown_path("image-scale-small");
    let image_path = document_path.with_extension("bmp");
    let mut bitmap = vec![80; 54 + 120 * 80 * 3];
    bitmap[..54].fill(0);
    bitmap[..2].copy_from_slice(b"BM");
    bitmap[2..6].copy_from_slice(&28854u32.to_le_bytes());
    bitmap[10..14].copy_from_slice(&54u32.to_le_bytes());
    bitmap[14..18].copy_from_slice(&40u32.to_le_bytes());
    bitmap[18..22].copy_from_slice(&120u32.to_le_bytes());
    bitmap[22..26].copy_from_slice(&80u32.to_le_bytes());
    bitmap[26..28].copy_from_slice(&1u16.to_le_bytes());
    bitmap[28..30].copy_from_slice(&24u16.to_le_bytes());
    fs::write(&image_path, bitmap).expect("写入小图片夹具");
    let markdown = format!(
        "![small]({})",
        image_path
            .file_name()
            .expect("图片文件名")
            .to_string_lossy()
    );
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown, Some(document_path)));
    redraw(cx);
    redraw(cx);
    let before = cx.debug_bounds("image-content").expect("原图片尺寸");
    assert_eq!(before.size, gpui::size(px(120.0), px(80.0)));
    editor.update(cx, |editor, cx| {
        let image = editor.document.first_root().expect("图片块").clone();
        image.update(cx, |block, cx| {
            block.image_width_factor = 0.5;
            block.write_image_width_back_to_source(cx);
        });
    });
    redraw(cx);
    let after = cx.debug_bounds("image-content").expect("缩放后的图片尺寸");
    assert_eq!(
        after.size,
        gpui::size(px(60.0), px(40.0)),
        "50% 缩放应同时减半宽高，小图不能只改变上限"
    );
    fs::remove_file(image_path).expect("清理图片夹具");
}

#[gpui::test]
async fn right_click_image_can_set_scale_and_undo_without_changing_other_blocks(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let original = "__前文__\n\n![图例](./missing.png \"说明\")\n\n尾文\n";
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.to_string(), None));
    redraw(cx);
    redraw(cx);
    open_image_menu(cx);
    click_scale_row("menu-item-image-scale-50", cx);
    let resized = original.replace("\")", "\"){width=50%}");
    assert_eq!(source(&editor, cx), resized);
    editor.read_with(cx, |editor, cx| {
        assert!(editor.document_dirty);
        let image = editor.document.root_blocks().get(1).expect("图片块");
        assert_eq!(image.read(cx).image_width_factor, 0.5);
    });
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    redraw(cx);
    assert_eq!(source(&editor, cx), original);
    editor.update(cx, |editor, cx| editor.redo_document(cx));
    redraw(cx);
    assert_eq!(source(&editor, cx), resized);
}

#[gpui::test]
async fn custom_image_scale_validates_input_and_persists_after_reopening(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let original = "![图例](./missing.png)\n";
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.to_string(), None));
    redraw(cx);
    redraw(cx);
    open_image_menu(cx);
    let input = cx
        .debug_bounds("image-scale-input")
        .expect("自定义缩放输入框");
    cx.simulate_click(input.center(), Modifiers::none());
    redraw(cx);
    let select_all = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };
    for invalid in ["0", "19", "101", "200", "50.5", "abc", ""] {
        cx.simulate_keystrokes(select_all);
        cx.simulate_input(invalid);
        click_scale_row("menu-item-image-scale-apply", cx);
        assert_eq!(source(&editor, cx), original);
        assert!(editor.read_with(cx, |editor, _| {
            editor.image_scale_input().expect("无效输入保留菜单").error
        }));
        assert!(editor.read_with(cx, |editor, _| editor.undo_history.is_empty()));
    }
    cx.simulate_keystrokes(select_all);
    cx.update(|_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("62%".to_string())));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    });
    assert_eq!(source(&editor, cx), original, "草稿输入不能提前修改文档");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    let resized = "![图例](./missing.png){width=62%}\n";
    assert_eq!(source(&editor, cx), resized);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            String::from_utf8(editor.buffer.file_bytes()).expect("UTF-8 保存字节"),
            resized
        )
    });
    let reopened = cx.new(|cx| Editor::from_markdown(cx, resized.to_string(), None));
    reopened.read_with(cx, |editor, cx| {
        assert_eq!(
            editor
                .document
                .first_root()
                .expect("重新打开的图片")
                .read(cx)
                .image_width_factor,
            0.62
        );
    });
    open_image_menu(cx);
    click_scale_row("menu-item-image-scale-100", cx);
    assert_eq!(source(&editor, cx), original, "恢复 100% 应删除宽度属性");
    open_image_menu(cx);
    let input = cx.debug_bounds("image-scale-input").expect("缩放输入框");
    cx.simulate_click(input.center(), Modifiers::none());
    redraw(cx);
    cx.simulate_input("20");
    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(source(&editor, cx), "![图例](./missing.png){width=20%}\n");
}

#[gpui::test]
async fn cancelling_image_scale_and_choosing_the_current_scale_do_not_edit_the_document(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let original = "![图例](./missing.png)\n";
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.to_string(), None));
    redraw(cx);
    redraw(cx);
    open_image_menu(cx);
    let input = cx.debug_bounds("image-scale-input").expect("缩放输入框");
    cx.simulate_click(input.center(), Modifiers::none());
    redraw(cx);
    cx.simulate_input("35");
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert_eq!(source(&editor, cx), original);
    assert!(editor.read_with(cx, |editor, _| editor.context_menu.is_none()));
    open_image_menu(cx);
    click_scale_row("menu-item-image-scale-100", cx);
    assert_eq!(source(&editor, cx), original);
    assert!(editor.read_with(cx, |editor, _| editor.undo_history.is_empty()));
}

#[gpui::test]
async fn image_scale_uses_the_right_clicked_target_and_keeps_reference_and_container_syntax(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    for original in [
        "![first](./first.png)\n\n> ![甲乙][hero]{width=75%}\n\n[hero]: ./second.png \"说明\"\n\n__尾文__\n",
        "![first](./first.png)\n\n- ![甲乙][hero]{width=75%}\n\n[hero]: https://example.com/second.png \"说明\"\n\n__尾文__\n",
    ] {
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.to_string(), None));
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            let first = editor
                .document
                .first_root()
                .expect("第一张图片")
                .entity_id();
            editor.focus_block(first);
            let target = editor
                .document
                .visible_blocks()
                .iter()
                .find(|entry| {
                    entry
                        .entity
                        .read(cx)
                        .image_runtime()
                        .is_some_and(|runtime| runtime.alt == "甲乙")
                })
                .expect("引用或列表中的目标图片")
                .entity
                .entity_id();
            editor.on_block_context_menu_mouse_down(
                target,
                &gpui::MouseDownEvent {
                    button: MouseButton::Right,
                    position: point(px(40.0), px(40.0)),
                    ..Default::default()
                },
                window,
                cx,
            );
        });
        redraw(cx);
        click_scale_row("menu-item-image-scale-25", cx);
        let resized = original.replace("{width=75%}", "{width=25%}");
        assert_eq!(
            source(&editor, cx),
            resized,
            "只修改右键图片的比例，其它写法逐字保留"
        );
        editor.update(cx, |editor, cx| editor.undo_document(cx));
        redraw(cx);
        assert_eq!(source(&editor, cx), original);
    }
}
