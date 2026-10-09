use super::common::*;
use gpui::{Entity, MouseButton, point};

fn open_image_menu(cx: &mut VisualTestContext) {
    let image = cx.debug_bounds("image-content").expect("图片或占位框");
    let position = image.center();
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
async fn image_scale_is_available_from_the_menu_without_a_resize_handle(cx: &mut TestAppContext) {
    // 用户报修：常驻缩放圆点破坏图片观感，去掉手柄后仍需支持右键缩放。
    init_editor_test_app(cx);
    let (_editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_markdown(cx, "![图例](./missing.png)".to_string(), None)
    });
    redraw(cx);
    redraw(cx);
    assert!(
        cx.debug_bounds("image-resize-handle").is_none(),
        "图片不应显示缩放圆点"
    );
    open_image_menu(cx);
    assert!(cx.debug_bounds("menu-item-image-scale-75").is_some());
}

#[gpui::test]
async fn image_scale_baseline_follows_the_text_column_and_keeps_the_aspect_ratio(
    cx: &mut TestAppContext,
) {
    // 用户要求：100% 对应正文容器宽度的 95%，图片左右边界绝不越过正文。
    // 侧栏会挤窄正文；必须核对实际块边界与滚动视口，不能拿整窗宽度当正文宽度。
    init_editor_test_app(cx);
    let image_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("docs/验收记录/2026-09-30-velora-macos-原生界面.png");
    let markdown = format!("![图例]({})", image_path.display());
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
    for (width, sidebar_open) in [
        (1200.0, true),
        (1200.0, false),
        (2400.0, true),
        (3840.0, true),
        (600.0, true),
    ] {
        editor.update(cx, |editor, cx| {
            editor.workspace.is_open = sidebar_open;
            cx.notify();
        });
        cx.simulate_resize(gpui::size(px(width), px(2160.0)));
        redraw(cx);
        redraw(cx);
        let block_bounds = cx.debug_bounds("block-shell").expect("正文图片块");
        let padding = cx.update(|_, cx| {
            cx.global::<ThemeManager>().current().dimensions.block_padding_x
        });
        let content_left = block_bounds.left() + px(padding);
        let content_right = block_bounds.right() - px(padding);
        let available_width = f32::from(content_right - content_left);
        for percent in [100, 75, 25, 100] {
            editor.update(cx, |editor, cx| {
                let image = editor.document.first_root().expect("图片块").clone();
                image.update(cx, |block, cx| {
                    block.image_width_factor = percent as f32 / 100.0;
                    block.write_image_width_back_to_source(cx);
                });
            });
            redraw(cx);
            let image_bounds = cx.debug_bounds("image-content").expect("图片尺寸");
            let viewport_bounds = editor.read_with(cx, |editor, _| editor.scroll_handle.bounds());
            assert!(
                image_bounds.left() >= content_left - px(1.0)
                    && image_bounds.right() <= content_right + px(1.0)
                    && image_bounds.left() >= viewport_bounds.left()
                    && image_bounds.right() <= viewport_bounds.right(),
                "窗口 {width}、侧栏 {sidebar_open}、{percent}% 的图片必须完整落在正文内：图片 {image_bounds:?}，正文 {block_bounds:?}，视口 {viewport_bounds:?}"
            );
            let scaled = image_bounds.size;
            let expected_width = available_width * 0.95 * percent as f32 / 100.0;
            assert!(
                (f32::from(scaled.width) - expected_width).abs() < 1.0,
                "窗口 {width} 的 {percent}% 应按正文宽度计算：期望 {expected_width}，实际 {scaled:?}"
            );
            assert!(
                (f32::from(scaled.height) - expected_width * 2136.0 / 2720.0).abs() < 1.0,
                "图片高度应随宽度等比变化，实际 {scaled:?}"
            );
        }
    }
    assert_eq!(source(&editor, cx), markdown, "恢复 100% 后源码不留缩放属性");
}

#[gpui::test]
async fn image_scale_layout_keeps_the_aspect_ratio_with_a_container_width(cx: &mut TestAppContext) {
    // 列表图片按容器比例限宽，也不能保留未缩放的原图高度。
    init_editor_test_app(cx);
    let image_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("docs/验收记录/2026-09-30-velora-macos-原生界面.png");
    let markdown = format!("![图例]({})", image_path.display());
    let (_block, cx) = cx.add_window_view(|_, cx| {
        let mut block = Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::BulletedListItem,
                InlineTextTree::from_markdown(&markdown),
            ),
        );
        block.sync_render_cache();
        block
    });
    cx.simulate_resize(gpui::size(px(1400.0), px(2000.0)));
    redraw(cx);
    redraw(cx);
    let size = cx
        .debug_bounds("image-content")
        .expect("列表图片尺寸")
        .size;
    assert!(
        (f32::from(size.height) - f32::from(size.width) * 2136.0 / 2720.0).abs() < 1.0,
        "图片布局框应贴合实际图片，不保留多余高度，实际 {size:?}"
    );
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
    for row in [
        "menu-item-image-scale-25",
        "menu-item-image-scale-50",
        "menu-item-image-scale-75",
        "menu-item-image-scale-100",
    ] {
        let bounds = cx.debug_bounds(row).expect("图片缩放预设");
        assert!(bounds.top() >= panel.top() && bounds.bottom() <= panel.bottom());
    }
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
    let block_bounds = cx.debug_bounds("block-shell").expect("小图片所在正文块");
    let padding = cx.update(|_, cx| {
        cx.global::<ThemeManager>().current().dimensions.block_padding_x
    });
    let expected_width = (f32::from(block_bounds.size.width) - padding * 2.0) * 0.95;
    assert!(
        (f32::from(before.size.width) - expected_width).abs() < 1.0,
        "100% 的小图片也应采用正文容器 95% 的宽度，实际 {before:?}"
    );
    assert!((f32::from(before.size.height) - expected_width * 80.0 / 120.0).abs() < 1.0);
    editor.update(cx, |editor, cx| {
        let image = editor.document.first_root().expect("图片块").clone();
        image.update(cx, |block, cx| {
            block.image_width_factor = 0.5;
            block.write_image_width_back_to_source(cx);
        });
    });
    redraw(cx);
    let after = cx.debug_bounds("image-content").expect("缩放后的图片尺寸");
    assert!(
        (f32::from(after.size.width) - f32::from(before.size.width) * 0.5).abs() < 1.0
            && (f32::from(after.size.height) - f32::from(before.size.height) * 0.5).abs() < 1.0,
        "50% 缩放应同时减半宽高，原尺寸 {before:?}，缩放后 {after:?}"
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
async fn image_scale_menu_only_offers_presets_and_persists_after_reopening(cx: &mut TestAppContext) {
    // 用户要求删除手动百分比输入，只保留四个缩放预设；预设仍需写回源码。
    init_editor_test_app(cx);
    let original = "![图例](./missing.png)\n";
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, original.to_string(), None));
    redraw(cx);
    redraw(cx);
    open_image_menu(cx);
    assert!(
        cx.debug_bounds("image-scale-input").is_none(),
        "菜单不应再有手动百分比输入"
    );
    assert!(
        cx.debug_bounds("menu-item-image-scale-apply").is_none(),
        "菜单不应再有应用按钮"
    );
    for row in [
        "menu-item-image-scale-25",
        "menu-item-image-scale-50",
        "menu-item-image-scale-75",
        "menu-item-image-scale-100",
    ] {
        assert!(cx.debug_bounds(row).is_some(), "应保留缩放预设 {row}");
    }
    click_scale_row("menu-item-image-scale-75", cx);
    let resized = "![图例](./missing.png){width=75%}\n";
    assert_eq!(source(&editor, cx), resized);
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            String::from_utf8(editor.buffer.file_bytes()).expect("UTF-8 保存字节"),
            resized
        );
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
            0.75
        );
    });
    open_image_menu(cx);
    click_scale_row("menu-item-image-scale-100", cx);
    assert_eq!(source(&editor, cx), original, "恢复 100% 应删除宽度属性");
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
    cx.simulate_click(point(px(4.0), px(100.0)), Modifiers::none());
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
