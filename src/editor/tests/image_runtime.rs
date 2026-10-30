use super::common::*;

#[gpui::test]
async fn mermaid_preview_opens_zooms_and_closes_without_editing_document(cx: &mut TestAppContext) {
    use gpui::{MouseButton, point};
    init_editor_test_app(cx);
    let body = std::iter::once("flowchart LR".to_string())
        .chain((0..10).map(|index| {
            format!(
                "N{index}[中文处理阶段{index}] --> N{}[中文处理阶段{}]",
                index + 1,
                index + 1
            )
        }))
        .collect::<Vec<_>>()
        .join("\n");
    let markdown = format!("intro\n\n```mermaid\n{body}\n```\n\ntail");
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
    for _ in 0..3 {
        redraw(cx);
    }
    let diagram = cx
        .debug_bounds("mermaid-diagram-image")
        .expect("正文图表应挂载");
    assert!(cx.debug_bounds("mermaid-diagram-scroll").is_none(), "正文不应增加横向滚动条");
    cx.simulate_mouse_down(diagram.center(), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(diagram.center(), MouseButton::Left, Modifiers::none());
    redraw(cx);
    redraw(cx);
    assert!(
        cx.debug_bounds("mermaid-preview").is_some(),
        "点击图表应打开放大预览"
    );
    let before = cx.debug_bounds("mermaid-preview-image").expect("预览图像");
    let zoom_in = cx
        .debug_bounds("mermaid-preview-zoom-in")
        .expect("放大按钮");
    cx.simulate_mouse_down(zoom_in.center(), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(zoom_in.center(), MouseButton::Left, Modifiers::none());
    redraw(cx);
    let after = cx
        .debug_bounds("mermaid-preview-image")
        .expect("放大后的预览图像");
    assert!(
        after.size.width > before.size.width,
        "预览中的放大按钮应增大图像"
    );
    let original_button = cx.debug_bounds("mermaid-preview-original").expect("原尺寸按钮");
    cx.simulate_mouse_down(original_button.center(), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(original_button.center(), MouseButton::Left, Modifiers::none());
    redraw(cx);
    let original = cx.debug_bounds("mermaid-preview-image").expect("原尺寸图像");
    let viewport = cx.debug_bounds("mermaid-preview-viewport").expect("预览视口");
    assert!(original.size.width > viewport.size.width, "原尺寸图像应保留可拖动的完整内容");
    let start = viewport.center();
    let end = start - point(px(60.0), px(0.0));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    redraw(cx);
    let dragged = cx.debug_bounds("mermaid-preview-image").expect("拖动后的图像");
    assert!(dragged.left() < original.left(), "放大后应能拖动查看被遮住的部分");
    let reset = cx.debug_bounds("mermaid-preview-fit").expect("重置缩放按钮");
    cx.simulate_mouse_down(reset.center(), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(reset.center(), MouseButton::Left, Modifiers::none());
    redraw(cx);
    let fitted = cx.debug_bounds("mermaid-preview-image").expect("适配窗口的图像");
    assert!((f32::from(fitted.size.width) - f32::from(before.size.width)).abs() < 1.0);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text().to_string()),
        markdown
    );
    cx.simulate_keystrokes("escape");
    redraw(cx);
    assert!(
        cx.debug_bounds("mermaid-preview").is_none(),
        "Esc 应关闭预览并返回正文"
    );
    let edit = cx.debug_bounds("mermaid-edit-source").expect("图表源码编辑入口");
    cx.simulate_mouse_down(edit.center(), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(edit.center(), MouseButton::Left, Modifiers::none());
    redraw(cx);
    redraw(cx);
    editor.read_with(cx, |editor, cx| {
        let active = editor.active_entity_id.expect("源码编辑应聚焦图表");
        let block = editor.document.block_entity_by_id(active).expect("活动图表");
        assert_eq!(block.read(cx).kind(), BlockKind::MermaidBlock);
        assert!(block.read(cx).has_text_layout(), "源码编辑应显示可输入的文本布局");
        assert_eq!(editor.buffer.text(), markdown);
    });
}

/// 预览工具条上的每个控件都必须是标准按钮：只画一个带 hover 的 div，键盘用户和
/// 辅助技术都发现不了「放大」「关闭」这些动作，也无法知道自己在按什么。
#[gpui::test]
fn mermaid_preview_controls_expose_native_button_semantics(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    init_editor_test_app(cx);
    let markdown = "```mermaid\nflowchart LR\nA[起点] --> B[终点]\n```".to_string();
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
    editor.update(cx, |editor, cx| editor.open_mermaid_preview(&markdown, cx));
    redraw(cx);
    redraw(cx);
    cx.update(|window, _| {
        for id in [
            "mermaid-preview-zoom-out",
            "mermaid-preview-zoom-in",
            "mermaid-preview-original",
            "mermaid-preview-fit",
            "mermaid-preview-close",
        ] {
            let item = window
                .try_find(id)
                .unwrap_or_else(|| panic!("{id} 应挂载在预览工具条上"));
            assert_eq!(item.role(), Some(gpui::Role::Button), "{id} 必须是标准按钮");
            assert!(
                item.label().is_some_and(|label| !label.is_empty()),
                "{id} 必须有可访问名称"
            );
        }
    });
}

/// 预览浮层是一个独立焦点范围：打开后焦点要进来，Tab 落到工具条上并能用 Enter
/// 按下去，按键不进正文，Esc 关掉并把焦点还给原来的块。
///
/// 这里必须走 `crate::ui::open_window` 开窗：Tab 与 Esc 的绑定挂在 Kit `Root` 上，
/// 直接 `add_window_view` 造出来的裸窗口没有 Root，Tab 根本不会派发。
#[gpui::test]
fn mermaid_preview_owns_keyboard_focus_while_open(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let fence = "```mermaid\nflowchart LR\nA[起点] --> B[终点]\n```".to_string();
    let markdown = format!("正文段落\n\n{fence}");
    let handle = cx
        .update(|cx| {
            crate::ui::open_window(gpui::WindowOptions::default(), cx, |_, cx| {
                cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), None))
            })
        })
        .unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    redraw(&mut visual);
    let block = handle
        .read_with(&visual, |editor, _| {
            editor.document.visible_blocks()[0].entity.clone()
        })
        .unwrap();
    handle
        .update(&mut visual, |editor, _, _| {
            editor.focus_block(block.entity_id())
        })
        .unwrap();
    redraw(&mut visual);
    redraw(&mut visual);
    let before = visual
        .update(|window, cx| window.focused(cx))
        .expect("正文块应持有焦点");
    handle
        .update(&mut visual, |editor, _, cx| {
            editor.open_mermaid_preview(&fence, cx)
        })
        .unwrap();
    redraw(&mut visual);
    redraw(&mut visual);
    let overlay_focus = handle
        .read_with(&visual, |editor, _| {
            editor
                .mermaid_preview
                .as_ref()
                .expect("预览应已打开")
                .focus
                .clone()
        })
        .unwrap();
    visual.update(|window, cx| {
        assert!(
            overlay_focus.contains_focused(window, cx),
            "预览打开后焦点应落在浮层里，而不是留在正文"
        );
    });
    visual.simulate_keystrokes("a");
    redraw(&mut visual);
    handle
        .read_with(&visual, |editor, _| {
            assert_eq!(
                editor.buffer.text(),
                markdown,
                "浮层开着时按键不能写进正文"
            )
        })
        .unwrap();
    // 浮层自己接手缩放键：焦点在陷阱里时按键捕获钩子已经让位。
    let fitted = visual
        .debug_bounds("mermaid-preview-image")
        .expect("预览图像")
        .size
        .width;
    visual.simulate_keystrokes("=");
    redraw(&mut visual);
    let zoomed = visual
        .debug_bounds("mermaid-preview-image")
        .expect("放大后的预览图像")
        .size
        .width;
    assert!(zoomed > fitted, "浮层里的 = 应放大图像");
    // Tab 落到第一颗控件（缩小），Enter 走 Kit Button 自己的键盘激活。
    visual.simulate_keystrokes("tab");
    redraw(&mut visual);
    visual.update(|window, cx| {
        assert!(
            overlay_focus.contains_focused(window, cx),
            "Tab 之后焦点仍在浮层里"
        );
    });
    // `simulate_keystrokes` 只派发按下，而 GPUI 的键盘激活发生在松开那一下，
    // 所以这里像真实平台那样同时发 keydown / keyup。
    let enter = gpui::Keystroke::parse("enter").expect("enter");
    visual.simulate_event(gpui::KeyDownEvent {
        keystroke: enter.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    visual.simulate_event(gpui::KeyUpEvent { keystroke: enter });
    redraw(&mut visual);
    let shrunk = visual
        .debug_bounds("mermaid-preview-image")
        .expect("缩小后的预览图像")
        .size
        .width;
    assert!(shrunk < zoomed, "Enter 应按下当前焦点按钮（缩小）");
    visual.simulate_keystrokes("escape");
    redraw(&mut visual);
    assert!(
        visual.debug_bounds("mermaid-preview").is_none(),
        "Esc 应关闭预览"
    );
    let after = visual
        .update(|window, cx| window.focused(cx))
        .expect("关闭后焦点不能丢");
    assert_eq!(after, before, "关闭后焦点要回到打开前的块");
}

#[gpui::test]
fn mermaid_preview_zoom_reaches_two_hundred_percent(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "```mermaid\nflowchart LR\nA[起点] --> B[终点]\n```".to_string();
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
    cx.update(|window, _| window.activate_window());
    editor.update(cx, |editor, cx| editor.open_mermaid_preview(&markdown, cx));
    redraw(cx);
    redraw(cx);
    let mut capped = Vec::new();
    for method in ["button", "wheel", "keyboard"] {
        let original = cx
            .debug_bounds("mermaid-preview-original")
            .expect("原始大小按钮");
        cx.simulate_click(original.center(), Modifiers::none());
        redraw(cx);
        let before = cx.debug_bounds("mermaid-preview-image").expect("原始图像");
        for _ in 0..8 {
            match method {
                "button" => {
                    let button = cx
                        .debug_bounds("mermaid-preview-zoom-in")
                        .expect("放大按钮");
                    cx.simulate_click(button.center(), Modifiers::none());
                }
                "wheel" => {
                    let viewport = cx
                        .debug_bounds("mermaid-preview-viewport")
                        .expect("预览视口");
                    cx.simulate_event(gpui::ScrollWheelEvent {
                        position: viewport.center(),
                        delta: gpui::ScrollDelta::Lines(gpui::point(0.0, 3.0)),
                        modifiers: Modifiers::none(),
                        touch_phase: gpui::TouchPhase::default(),
                    });
                }
                _ => cx.simulate_keystrokes("="),
            }
            redraw(cx);
        }
        let after = cx.debug_bounds("mermaid-preview-image").expect("放大图像");
        if (f32::from(after.size.width) / f32::from(before.size.width) - 2.0).abs() > 0.01 {
            capped.push(method);
        }
    }
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.buffer.text()),
        markdown
    );
    assert!(capped.is_empty(), "这些入口仍无法放大到 200%：{capped:?}");
}

#[gpui::test]
async fn standalone_root_image_installs_runtime_and_resolves_relative_path(
    cx: &mut TestAppContext,
) {
    let markdown = "![diagram](./assets/diagram.png \"System diagram\")".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.title.as_deref(), Some("System diagram"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn standalone_root_image_with_underscores_installs_runtime(cx: &mut TestAppContext) {
    let markdown =
        "![1.1_进制转换例子](./NetworkEngineerSummer.assets/1.1_进制转换例子.jpg)".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "1.1_进制转换例子");
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("NetworkEngineerSummer.assets/1.1_进制转换例子.jpg")
            )
        );
        assert_eq!(editor.document.markdown_text(cx), markdown);
    });
}

#[gpui::test]
async fn indented_root_images_install_runtime_before_indented_code(cx: &mut TestAppContext) {
    let url1 = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508201435626.png";
    let url2 = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508201438742.png";
    let url3 = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508201439288.png";
    let url4 = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508201419865.png";
    let markdown = [
        format!("![image-1]({})", url1.replace("_", "\\_")),
        String::new(),
        format!("   ![image-2]({})", url2.replace("_", "\\_")),
        String::new(),
        format!("        ![image-3]({})", url3.replace("_", "\\_")),
        String::new(),
        "   所有组或用户名均对**Anaconda安装目录**的权限设置为**完全控制**后，如下图所示："
            .to_string(),
        String::new(),
        format!("![image-4]({})", url4.replace("_", "\\_")),
        String::new(),
        "    plain indented code".to_string(),
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        let image_sources = roots
            .iter()
            .filter_map(|block| {
                block
                    .read(cx)
                    .image_runtime()
                    .map(|runtime| runtime.src.clone())
            })
            .collect::<Vec<_>>();
        assert_eq!(image_sources, vec![url1, url2, url3, url4]);
        assert!(
            roots
                .iter()
                .any(|block| matches!(block.read(cx).kind(), BlockKind::CodeBlock { .. }))
        );
    });
}

#[gpui::test]
async fn mixed_text_does_not_activate_image_runtime(cx: &mut TestAppContext) {
    let markdown = "before ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        assert!(block.read(cx).image_runtime().is_none());
    });
}

#[gpui::test]
async fn ordinary_edit_skips_global_context_but_image_edits_refresh_it(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| {
        Editor::from_markdown(
            cx,
            "# Heading\n\n![old](https://example.com/old.png)".into(),
            None,
        )
    });
    let (heading, image, definitions) = editor.read_with(cx, |editor, _cx| {
        (
            editor.document.root_blocks()[0].clone(),
            editor.document.root_blocks()[1].clone(),
            editor.image_reference_definitions.clone(),
        )
    });

    heading.update(cx, |heading, cx| {
        heading.record.set_title(InlineTextTree::plain("Updated"));
        heading.sync_render_cache();
        cx.emit(BlockEvent::Changed);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert!(Arc::ptr_eq(
            &definitions,
            &editor.image_reference_definitions
        ));
    });

    image.update(cx, |image, cx| {
        image.record.set_title(InlineTextTree::plain("plain"));
        image.sync_render_cache();
        cx.emit(BlockEvent::Changed);
    });
    cx.run_until_parked();
    assert!(image.read_with(cx, |image, _cx| image.image_runtime().is_none()));

    image.update(cx, |image, cx| {
        image
            .record
            .set_title(InlineTextTree::plain("![new](https://example.com/new.png)"));
        image.sync_render_cache();
        cx.emit(BlockEvent::Changed);
    });
    cx.run_until_parked();
    image.read_with(cx, |image, _cx| {
        assert_eq!(
            image
                .image_runtime()
                .as_ref()
                .map(|runtime| runtime.src.as_str()),
            Some("https://example.com/new.png")
        );
    });
}

#[gpui::test]
async fn editing_image_reference_definition_refreshes_existing_image(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| {
        Editor::from_markdown(
            cx,
            "![photo][asset]\n\n[asset]: https://example.com/old.png".into(),
            None,
        )
    });
    let (image, definition) = editor.read_with(cx, |editor, _cx| {
        (
            editor.document.root_blocks()[0].clone(),
            editor.document.root_blocks()[1].clone(),
        )
    });
    assert_eq!(
        image.read_with(cx, |image, _cx| image
            .image_runtime()
            .map(|runtime| runtime.src.clone())),
        Some("https://example.com/old.png".into())
    );

    definition.update(cx, |definition, cx| {
        definition.record.set_title(InlineTextTree::plain(
            "[asset]: https://example.com/new.png",
        ));
        definition.sync_render_cache();
        cx.emit(BlockEvent::Changed);
    });
    cx.run_until_parked();
    assert_eq!(
        image.read_with(cx, |image, _cx| image
            .image_runtime()
            .map(|runtime| runtime.src.clone())),
        Some("https://example.com/new.png".into())
    );
}

#[gpui::test]
async fn reference_style_root_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown =
        "![reference image][ref-image]\n\n[ref-image]: ./assets/ref-image.png \"Caption\""
            .to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "reference image");
        assert_eq!(runtime.src, "./assets/ref-image.png");
        assert_eq!(runtime.title.as_deref(), Some("Caption"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/ref-image.png")
            )
        );
    });
}

#[gpui::test]
async fn quote_child_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = ">     ![diagram](./assets/diagram.png \"Caption\")".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let quote = editor.document.first_root().expect("quote root").clone();
        let image_block = quote
            .read(cx)
            .children
            .first()
            .expect("quote image child")
            .clone();
        let runtime = image_block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Caption"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn bulleted_list_item_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = "-     ![diagram](./assets/diagram.png \"Caption\")".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Caption"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn html_fallback_before_image_does_not_swallow_standalone_image(cx: &mut TestAppContext) {
    let image_url = "https://gitee.com/jikeyang/typera_picgo/raw/master/sias/202508200941158.png";
    let markdown = format!(
        "<span style='color:blue;'>Anaconda下载地址</span>：https://mirrors.tuna.tsinghua.edu.cn/anaconda/archive/\n\n![image-20250820094109009]({image_url})"
    );
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.root_count(), 2);
        {
            let html = editor.document.root_blocks()[0].read(cx);
            assert_eq!(html.kind(), BlockKind::HtmlBlock);
            assert!(
                html.display_text()
                    .starts_with("<span style='color:blue;'>")
            );
            assert!(
                html.record
                    .html
                    .as_ref()
                    .is_some_and(|html| html.is_semantic())
            );
        }

        let image = editor.document.root_blocks()[1].read(cx);
        let runtime = image.image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "image-20250820094109009");
        assert_eq!(runtime.src, image_url);
        match &runtime.resolved_source {
            ImageResolvedSource::Remote(uri) => assert_eq!(uri.to_string(), image_url),
            other => panic!("expected remote image, got {other:?}"),
        }
    });
}

#[gpui::test]
async fn unclosed_html_block_stops_before_standalone_image_without_blank(cx: &mut TestAppContext) {
    let image_url = "https://example.com/image.png";
    let markdown = format!("<span>unclosed html\n![image]({image_url})");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.root_count(), 2);
        // html5ever recovers the unclosed tag like a browser does; the block is
        // still an HTML block, not raw Markdown.
        let html = editor.document.root_blocks()[0].read(cx);
        assert_eq!(html.kind(), BlockKind::HtmlBlock);
        assert!(
            html.record
                .html
                .as_ref()
                .is_some_and(|html| html.is_semantic())
        );
        let image = editor.document.root_blocks()[1].read(cx);
        let runtime = image.image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "image");
        assert_eq!(runtime.src, image_url);
    });
}

#[gpui::test]
async fn numbered_list_item_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = "1. ![diagram](https://example.com/diagram.gif \"Caption\")".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.title.as_deref(), Some("Caption"));
        match &runtime.resolved_source {
            ImageResolvedSource::Remote(uri) => {
                assert_eq!(uri.to_string(), "https://example.com/diagram.gif");
            }
            other => panic!("expected remote source, got {other:?}"),
        }
    });
}

#[gpui::test]
async fn task_list_item_reference_style_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = "- [ ] ![diagram][cover]\n\n[cover]: ./assets/diagram.png \"Cover\"".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("task list item root")
            .clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Cover"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn mixed_list_item_title_does_not_activate_image_runtime(cx: &mut TestAppContext) {
    let markdown = "- text ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        assert!(block.read(cx).image_runtime().is_none());
    });
}

#[gpui::test]
async fn list_child_reference_style_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = [
        "- item",
        "  ![diagram][cover]",
        "",
        "[cover]: ./assets/diagram.png \"Cover\"",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let list_item = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let image_block = list_item
            .read(cx)
            .children
            .first()
            .expect("list child image")
            .clone();
        let runtime = image_block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Cover"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn list_scoped_reference_definition_supports_list_item_image_runtime(
    cx: &mut TestAppContext,
) {
    let markdown = [
        "- ![diagram][cover]",
        "  [cover]: ./assets/diagram.png \"Cover\"",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown.clone(), Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let list_item = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let runtime = list_item.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Cover"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
        assert_eq!(
            list_item
                .read(cx)
                .children
                .first()
                .expect("reference definition child")
                .read(cx)
                .kind(),
            BlockKind::RawMarkdown
        );
    });
}

#[gpui::test]
async fn quote_list_item_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = "> - ![diagram](./assets/diagram.png)".to_string();
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let quote = editor.document.first_root().expect("quote root").clone();
        let list_item = quote
            .read(cx)
            .children
            .first()
            .expect("quote list child")
            .clone();
        let runtime = list_item.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn callout_task_list_reference_style_image_uses_container_scoped_definition(
    cx: &mut TestAppContext,
) {
    let markdown = [
        "> [!NOTE]",
        "> - [ ] ![diagram][cover]",
        ">",
        "> [cover]: ./assets/diagram.png \"Cover\"",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let callout = editor.document.first_root().expect("callout root").clone();
        let list_item = callout
            .read(cx)
            .children
            .first()
            .expect("callout list child")
            .clone();
        let runtime = list_item.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Cover"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn callout_list_child_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = [
        "> [!NOTE]",
        "> - item",
        ">   ![diagram](./assets/diagram.png)",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let callout = editor.document.first_root().expect("callout root").clone();
        let list_item = callout
            .read(cx)
            .children
            .first()
            .expect("callout list child")
            .clone();
        let image_block = list_item
            .read(cx)
            .children
            .first()
            .expect("list child image")
            .clone();
        let runtime = image_block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn callout_child_reference_style_image_uses_container_scoped_definition(
    cx: &mut TestAppContext,
) {
    let markdown = [
        "> [!NOTE]",
        ">     ![diagram][anim]",
        ">",
        "> [anim]: ./assets/diagram.png \"Animated\"",
    ]
    .join("\n");
    let file_path = PathBuf::from("D:/workspace/docs/note.md");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, Some(file_path.clone())));

    editor.read_with(cx, |editor, cx| {
        let callout = editor.document.first_root().expect("callout root").clone();
        let image_block = callout
            .read(cx)
            .children
            .iter()
            .find(|child| {
                child.read(cx).kind() == BlockKind::Paragraph
                    && child.read(cx).image_runtime().is_some()
            })
            .expect("callout image child")
            .clone();
        let runtime = image_block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.alt, "diagram");
        assert_eq!(runtime.src, "./assets/diagram.png");
        assert_eq!(runtime.title.as_deref(), Some("Animated"));
        assert_eq!(
            runtime.resolved_source,
            ImageResolvedSource::Local(
                file_path
                    .parent()
                    .expect("file parent")
                    .join("assets/diagram.png")
            )
        );
    });
}

#[gpui::test]
async fn table_cell_with_standalone_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = [
        "| Preview |",
        "| --- |",
        "|    ![diagram](https://example.com/diagram.gif \"Animated\") |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        let cell_runtime = runtime.rows[0][0]
            .read(cx)
            .image_runtime()
            .expect("cell image runtime");
        assert_eq!(cell_runtime.alt, "diagram");
        assert_eq!(cell_runtime.title.as_deref(), Some("Animated"));
        match &cell_runtime.resolved_source {
            ImageResolvedSource::Remote(uri) => {
                assert_eq!(uri.to_string(), "https://example.com/diagram.gif");
            }
            other => panic!("expected remote source, got {other:?}"),
        }
    });
}

#[gpui::test]
async fn table_cell_with_mixed_inline_image_uses_inline_image_segments(cx: &mut TestAppContext) {
    let markdown = [
        "| Preview |",
        "| --- |",
        "| image ![alt](https://example.com/x.png) |",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        let cell = runtime.rows[0][0].read(cx);
        assert!(cell.image_runtime().is_none());

        let segments = parse_table_cell_inline_images(&cell.record.title_markdown());
        assert_eq!(segments.len(), 2);
        assert_eq!(
            segments[0],
            TableCellInlineImageSegment::Text("image ".to_string())
        );
        assert!(matches!(
            &segments[1],
            TableCellInlineImageSegment::Image { syntax, .. }
                if syntax.alt == "alt"
                    && syntax
                        .resolve_target(&ImageReferenceDefinitions::default())
                        .is_some_and(|target| target.src == "https://example.com/x.png")
        ));
    });
}

#[gpui::test]
async fn table_cell_with_reference_style_image_installs_runtime(cx: &mut TestAppContext) {
    let markdown = [
        "| Preview |",
        "| --- |",
        "| ![diagram][anim] |",
        "",
        "[anim]: https://example.com/diagram.gif \"Animated\"",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        let cell_runtime = runtime.rows[0][0]
            .read(cx)
            .image_runtime()
            .expect("cell image runtime");
        assert_eq!(cell_runtime.alt, "diagram");
        assert_eq!(cell_runtime.title.as_deref(), Some("Animated"));
        match &cell_runtime.resolved_source {
            ImageResolvedSource::Remote(uri) => {
                assert_eq!(uri.to_string(), "https://example.com/diagram.gif");
            }
            other => panic!("expected remote source, got {other:?}"),
        }
    });
}

#[gpui::test]
async fn reference_style_link_in_root_paragraph_resolves_document_wide(cx: &mut TestAppContext) {
    let markdown = [
        "[reference link][ref-link]",
        "",
        "[ref-link]: https://example.com",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        assert_eq!(block.read(cx).display_text(), "reference link");
        assert_eq!(
            block.read(cx).inline_link_at(0),
            Some("https://example.com")
        );
    });
}

#[gpui::test]
async fn reference_style_link_in_table_cell_resolves_document_wide(cx: &mut TestAppContext) {
    let markdown = [
        "| Link |",
        "| --- |",
        "| [reference link][ref-link] |",
        "",
        "[ref-link]: https://example.com",
    ]
    .join("\n");
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.read_with(cx, |editor, cx| {
        let table = editor.document.first_root().expect("table root").clone();
        let runtime = table
            .read(cx)
            .table_runtime
            .as_ref()
            .expect("table runtime");
        let cell = runtime.rows[0][0].clone();
        assert_eq!(cell.read(cx).display_text(), "reference link");
        assert_eq!(cell.read(cx).inline_link_at(0), Some("https://example.com"));
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_root_image_runtime(cx: &mut TestAppContext) {
    let markdown = "![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        assert!(block.read(cx).image_runtime().is_some());
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_reference_style_root_image_runtime(
    cx: &mut TestAppContext,
) {
    let markdown = "![diagram][ref]\n\n[ref]: ./assets/diagram.png".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root block").clone();
        let runtime = block.read(cx).image_runtime().expect("image runtime");
        assert_eq!(runtime.src, "./assets/diagram.png");
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_quote_child_image_runtime(cx: &mut TestAppContext) {
    let markdown = "> ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let quote = editor.document.first_root().expect("quote root").clone();
        let image_block = quote
            .read(cx)
            .children
            .first()
            .expect("quote image child")
            .clone();
        assert!(image_block.read(cx).image_runtime().is_some());
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_list_item_image_runtime(cx: &mut TestAppContext) {
    let markdown = "- ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let block = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        assert!(block.read(cx).image_runtime().is_some());
    });
}

#[gpui::test]
async fn toggling_source_mode_preserves_list_child_image_runtime(cx: &mut TestAppContext) {
    let markdown = "- item\n  ![diagram](./assets/diagram.png)".to_string();
    let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

    editor.update(cx, |editor, cx| {
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Source));
        editor.toggle_view_mode(cx);
        assert!(matches!(editor.view_mode, ViewMode::Rendered));
    });

    editor.read_with(cx, |editor, cx| {
        let list_item = editor
            .document
            .first_root()
            .expect("list item root")
            .clone();
        let image_block = list_item
            .read(cx)
            .children
            .first()
            .expect("list child image")
            .clone();
        assert!(image_block.read(cx).image_runtime().is_some());
    });
}

const PNG_PREVIEW_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/images/preview.png"
));
const JPEG_PREVIEW_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/images/preview.jpg"
));
const WEBP_PREVIEW_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/images/preview.webp"
));
const ICO_PREVIEW_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/images/preview.ico"
));

fn image_preview_fixture_root(cx: &mut TestAppContext) -> PathBuf {
    let root = temp_fixture_dir().join(format!("image-preview-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("创建图片预览夹具目录");
    let root = crate::file_url::canonical_path(root);
    cx.on_quit({
        let root = root.clone();
        move || fs::remove_dir_all(root).expect("清理图片预览夹具")
    });
    root
}

#[gpui::test]
async fn image_file_preview_first_frame_does_not_flash_placeholder_text(cx: &mut TestAppContext) {
    use crate::editor::workspace::WorkspaceOpenMode;

    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let cases: &[(&str, &[u8])] = &[
        ("PNG", PNG_PREVIEW_BYTES),
        ("jpg", JPEG_PREVIEW_BYTES),
        ("webp", WEBP_PREVIEW_BYTES),
        ("ico", ICO_PREVIEW_BYTES),
    ];
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    for (extension, bytes) in cases {
        let path = root.join(format!("首次打开 [图片].{extension}"));
        fs::write(&path, bytes).expect("写入图片");
        // 文件树单击走预览模式；在调度解码任务之前画首帧，才能捕获冷缓存的闪字。
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file_in_mode(path, WorkspaceOpenMode::Preview, window, cx);
                editor.focus_workspace_tree(window, cx);
            });
            window.draw(cx).clear(cx);
        });
        assert!(cx.debug_bounds("image-file-preview").is_some());
        assert!(
            cx.debug_bounds("image-file-preview-message").is_none(),
            "{extension} 首次解码前不应闪出一行占位文字"
        );
        editor.read_with(cx, |editor, _| {
            assert!(
                editor
                    .image_preview
                    .as_ref()
                    .is_some_and(|preview| preview.image.is_none()),
                "应覆盖第一次解码还未完成的帧"
            );
            assert!(editor.unsupported_preview_path.is_none());
        });
        redraw(cx);
        redraw(cx);
        assert!(cx.debug_bounds("image-file-preview-image").is_some());
    }
}

#[gpui::test]
fn image_file_preview_zoom_responds_to_wheel_in_both_directions(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let path = root.join("滚轮缩放.svg");
    fs::write(&path, r##"<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 900"><rect width="1600" height="900" fill="#48a"/></svg>"##).expect("生成大图夹具");
    let original_bytes = fs::read(&path).expect("读取原图");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        window.resize(gpui::size(px(900.0), px(650.0)));
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        });
    });
    redraw(cx);
    redraw(cx);
    let before = cx
        .debug_bounds("image-file-preview-image")
        .expect("已加载的图片");
    let viewport = cx.debug_bounds("image-file-preview").expect("图片视口");
    assert!(before.size.width < px(1600.0), "大图初始应适应视口");
    let mut unchanged = Vec::new();
    for (label, delta) in [
        ("鼠标滚轮", gpui::ScrollDelta::Lines(gpui::point(0.0, 3.0))),
        (
            "触控板",
            gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(20.0))),
        ),
    ] {
        for _ in 0..3 {
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta,
                modifiers: Modifiers::none(),
                touch_phase: gpui::TouchPhase::default(),
            });
            redraw(cx);
        }
        let enlarged = cx
            .debug_bounds("image-file-preview-image")
            .expect("放大后的图片");
        if enlarged.size.width <= before.size.width || enlarged.size.width <= viewport.size.width {
            unchanged.push(label);
        }
        for _ in 0..3 {
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: match delta {
                    gpui::ScrollDelta::Lines(lines) => {
                        gpui::ScrollDelta::Lines(gpui::point(-lines.x, -lines.y))
                    }
                    gpui::ScrollDelta::Pixels(pixels) => {
                        gpui::ScrollDelta::Pixels(gpui::point(-pixels.x, -pixels.y))
                    }
                },
                modifiers: Modifiers::none(),
                touch_phase: gpui::TouchPhase::default(),
            });
            redraw(cx);
        }
        let reduced = cx
            .debug_bounds("image-file-preview-image")
            .expect("缩小后的图片");
        if reduced.size.width >= enlarged.size.width {
            unchanged.push("反向滚动未缩小");
        }
    }
    assert_eq!(fs::read(&path).expect("读取缩放后的原文件"), original_bytes);
    editor.read_with(cx, |editor, _| {
        assert!(!editor.document_dirty);
        assert!(editor.buffer.text().is_empty());
    });
    assert!(
        unchanged.is_empty(),
        "滚轮未改变图片显示大小：{unchanged:?}"
    );
}

#[gpui::test]
async fn image_file_preview_decodes_formats_and_never_saves_text_over_images(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let cases: &[(&str, &[u8])] = &[
        ("PNG", PNG_PREVIEW_BYTES),
        ("jpg", JPEG_PREVIEW_BYTES),
        ("jpeg", JPEG_PREVIEW_BYTES),
        ("webp", WEBP_PREVIEW_BYTES),
        ("ico", ICO_PREVIEW_BYTES),
    ];
    for (extension, bytes) in cases {
        let path = root.join(format!("图片 [1].{extension}"));
        fs::write(&path, bytes).expect("写入图片夹具");
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.open_workspace_file(path.clone(), window, cx)
            })
        });
        cx.run_until_parked();
        redraw(cx);
        editor.read_with(cx, |editor, cx| {
            let preview = editor.image_preview.as_ref().expect("进入图片预览");
            let image = preview.image.as_ref().unwrap_or_else(|| {
                panic!(
                    "{extension} 未完成实际解码，模态状态：{}",
                    editor.modal_is_open()
                )
            });
            assert_eq!(
                image.size(0),
                gpui::size(gpui::DevicePixels(16), gpui::DevicePixels(16)),
                "{extension} 的图片尺寸"
            );
            assert_eq!(editor.file_path.as_ref(), Some(&path));
            assert!(
                editor.buffer.text().is_empty(),
                "图片字节不能进入文本缓冲区"
            );
            assert!(!editor.document_dirty);
            assert!(
                editor.current_edit_target_from_state(cx).is_none(),
                "图片没有文本编辑目标"
            );
            assert!(!editor.modal_is_open());
        });
        let bounds = cx
            .debug_bounds("image-file-preview-image")
            .expect("图片元素应渲染");
        assert!(bounds.size.width > px(0.0) && bounds.size.height > px(0.0));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.replace_text_in_range(None, "不应写入", window, cx);
                editor.toggle_view_mode(cx);
                editor.format_document(cx);
                editor.save_document(window, cx);
                editor.save_document_as(window, cx);
                assert!(
                    editor
                        .export_document_to_path(ExportFormat::Png, &path, cx)
                        .is_err()
                );
            })
        });
        assert_eq!(
            fs::read(&path).expect("读取预览后的原文件"),
            *bytes,
            "预览及文本命令不能覆盖图片"
        );
        editor.read_with(cx, |editor, _| {
            assert!(editor.image_preview.is_some());
            assert!(!editor.document_dirty);
            assert!(editor.buffer.text().is_empty());
        });
    }
}

#[gpui::test]
async fn image_file_preview_preserves_dirty_text_tabs_and_closes_normally(cx: &mut TestAppContext) {
    use crate::editor::workspace::WorkspaceOpenMode;
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let note = root.join("note.md");
    let picture = root.join("picture.png");
    let another = root.join("another.webp");
    fs::write(&another, WEBP_PREVIEW_BYTES).expect("写入另一张图片");
    fs::write(&note, "original").expect("写入文档");
    fs::write(&picture, PNG_PREVIEW_BYTES).expect("写入图片");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(note.clone(), window, cx)
        })
    });
    redraw(cx);
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().expect("文档块").clone()
    });
    block.update(cx, |block, cx| {
        block.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let end = block.visible_len();
        block.replace_text_in_visible_range(end..end, " draft", None, false, cx);
    });
    redraw(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(
                picture.clone(),
                WorkspaceOpenMode::Preview,
                window,
                cx,
            );
        })
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.cached_tab_content_for_path(&note, cx),
            Some(("original draft".into(), true)),
            "图片预览必须保留文档的未保存内容"
        );
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(
                picture.clone(),
                WorkspaceOpenMode::Pinned,
                window,
                cx,
            );
            editor.open_workspace_file_in_mode(another, WorkspaceOpenMode::Preview, window, cx);
            editor.close_active_tab(window, cx);
            assert_eq!(
                editor.file_path.as_ref(),
                Some(&picture),
                "固定图片标签不能被新的临时预览替换"
            );
            editor.close_active_tab(window, cx);
        })
    });
    editor.read_with(cx, |editor, _| {
        assert!(editor.image_preview.is_none());
        assert_eq!(editor.file_path.as_ref(), Some(&note));
        assert_eq!(editor.buffer.text(), "original draft");
        assert!(editor.document_dirty);
    });
}

#[gpui::test]
async fn image_file_preview_reports_decode_errors_in_an_app_modal(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let path = root.join("broken.png");
    fs::write(&path, [0, 1, 2, 3]).expect("写入损坏图片");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path, window, cx)
        })
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(editor.image_preview.is_some());
        assert!(editor.modal_is_open(), "异步解码失败必须显示应用内模态");
    });
}

#[gpui::test]
async fn image_file_preview_refreshes_after_external_changes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let path = root.join("picture.png");
    fs::write(&path, PNG_PREVIEW_BYTES).expect("写入图片");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    let before = editor.read_with(cx, |editor, _| {
        editor
            .image_preview
            .as_ref()
            .and_then(|preview| preview.image.as_ref())
            .expect("首次解码")
            .id
    });
    fs::write(&path, JPEG_PREVIEW_BYTES).expect("外部替换图片内容");
    editor.update(cx, |editor, cx| editor.on_watched_path_changed(&path, cx));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        let image = editor
            .image_preview
            .as_ref()
            .and_then(|preview| preview.image.as_ref())
            .expect("更新后的解码");
        assert_ne!(image.id, before, "外部更新必须使已解码图片失效");
        assert!(!editor.modal_is_open());
    });
}

/// 外部等长替换并保留 mtime 后，正文各入口必须显示新图，不能继续复用旧解码。
#[gpui::test]
fn embedded_images_refresh_after_external_changes(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let cases = [
        ("block", "![图](picture.svg)"),
        ("inline", "正文 ![图](./picture.svg) 结尾"),
        ("quote", "> ![图](picture.svg)"),
        ("table", "| 图片 |\n| --- |\n| ![图](picture.svg) |"),
        ("html", "<img src=\"picture.svg\" />"),
    ];
    let mut stale = Vec::new();
    for (name, markdown) in cases {
        let markdown = format!("说明\n\n{markdown}");
        let directory = root.join(name);
        fs::create_dir_all(&directory).expect("创建图片目录");
        let path = directory.join("picture.svg");
        let svg = |height| {
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"{height}\"><rect width=\"100\" height=\"{height}\" fill=\"red\"/></svg>"
            )
        };
        fs::write(&path, svg(20)).expect("写入原图");
        let stamp = fs::metadata(&path).unwrap().modified().unwrap();
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::from_markdown(cx, markdown.clone(), Some(directory.join("note.md")))
        });
        for _ in 0..4 {
            redraw(cx);
        }
        let before = cx
            .debug_bounds("image-content")
            .unwrap_or_else(|| panic!("{name} 原图已显示"));
        assert!(before.size.height > px(0.0), "{name} 原图应有高度");
        fs::write(&path, svg(40)).expect("外部替换图片");
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(stamp)
            .expect("保留原时间戳");
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&path, cx));
        for _ in 0..4 {
            redraw(cx);
        }
        let after = cx.debug_bounds("image-content").expect("替换后图片已显示");
        if (f32::from(after.size.height) / f32::from(before.size.height) - 2.0).abs() > 0.05 {
            stale.push((name, before.size, after.size));
        }
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.buffer.text(), markdown, "刷新图片不应改写文档");
            assert!(!editor.document_dirty);
        });
    }
    assert!(stale.is_empty(), "这些图片仍显示外部替换前的内容：{stale:?}");
}

#[gpui::test]
async fn image_file_preview_prompts_before_discarding_an_untitled_draft(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let path = root.join("picture.png");
    fs::write(&path, PNG_PREVIEW_BYTES).expect("写入图片");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "draft".into(), None));
    editor.update(cx, |editor, _| editor.document_dirty = true);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        })
    });
    editor.read_with(cx, |editor, _| {
        assert!(editor.show_drop_replace_dialog);
        assert!(editor.image_preview.is_none());
        assert_eq!(editor.buffer.text(), "draft");
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.discard_pending_drop_replace(window, cx)
        })
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.file_path.as_ref(), Some(&path));
        assert!(
            editor
                .image_preview
                .as_ref()
                .is_some_and(|preview| preview.image.is_some())
        );
        assert!(!editor.document_dirty);
    });
}

#[gpui::test]
async fn image_file_preview_is_available_from_new_window_open(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let path = root.join("picture.ico");
    fs::write(&path, ICO_PREVIEW_BYTES).expect("写入图标");
    let handle = cx.update(|cx| {
        crate::app_menu::open_file_in_new_window(cx, &path).expect("二进制图片应正常打开");
        cx.windows()
            .into_iter()
            .find_map(|handle| crate::ui::window_content::<Editor>(handle, cx))
            .expect("图片窗口")
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let editor = handle.read(cx).expect("读取图片窗口");
        assert_eq!(editor.file_path.as_ref(), Some(&path));
        assert!(
            editor
                .image_preview
                .as_ref()
                .is_some_and(|preview| preview.image.is_some())
        );
        assert!(editor.buffer.text().is_empty());
    });
}

#[gpui::test]
async fn image_file_preview_keeps_quick_open_visible_and_lists_images(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let picture = root.join("picture.png");
    let another = root.join("another.webp");
    fs::write(&picture, PNG_PREVIEW_BYTES).expect("写入图片");
    fs::write(&another, WEBP_PREVIEW_BYTES).expect("写入图片");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root, cx));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(picture.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| editor.update(cx, |editor, cx| editor.toggle_quick_open(window, cx)));
    redraw(cx);
    assert!(
        cx.debug_bounds("quick-open-overlay").is_some(),
        "图片预览不能遮住快速打开"
    );
    editor.read_with(cx, |editor, _| {
        let entries = &editor.quick_open.as_ref().expect("快速打开").results;
        assert!(
            entries.contains(&picture) && entries.contains(&another),
            "快速打开应列出可预览图片"
        );
    });
}

#[gpui::test]
async fn printing_an_image_preview_shows_a_modal_instead_of_panicking(cx: &mut TestAppContext) {
    // print_document 的失败分支曾重入 editor.update（实体正在 update 栈上）：
    // GPUI 双重租借 panic。菜单派发路径必须弹应用内模态。
    use crate::editor::workspace::WorkspaceOpenMode;
    init_editor_test_app(cx);
    let root = image_preview_fixture_root(cx);
    let path = root.join("print.png");
    fs::write(&path, PNG_PREVIEW_BYTES).expect("写入图片");
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file_in_mode(path, WorkspaceOpenMode::Preview, window, cx);
        });
        window.draw(cx).clear(cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(editor.image_preview.is_some(), "夹具应处于图片预览");
    });

    cx.update(|window, cx| {
        let target = editor.downgrade();
        crate::app_menu::dispatch_menu_action_for_editor(
            &crate::components::PrintDocument,
            &target,
            window,
            cx,
        );
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(editor.modal.is_some(), "打印失败应弹应用内模态而不是重入 panic");
    });
}

/// 拦截器那条 Esc（焦点陷阱尚未建立的窗口期）也必须走正式关闭函数：旧实现
/// 直接把预览状态置空，焦点悬在已被丢弃的浮层上，必须用鼠标点回正文才能继续
/// 打字。主题重渲染失败那条路径同理。
#[gpui::test]
async fn mermaid_preview_escape_through_the_interceptor_restores_block_focus(
    cx: &mut TestAppContext,
) {
    init_editor_test_app(cx);
    let markdown = "```mermaid\nflowchart LR\nA-->B\n```\n\n正文\n".to_string();
    let (editor, cx) = cx.add_window_view({
        let markdown = markdown.clone();
        move |_, cx| Editor::from_markdown(cx, markdown, None)
    });
    redraw(cx);
    let fence = editor.read_with(cx, |editor, _| editor.document.root_blocks()[0].clone());
    editor.update(cx, |editor, _cx| editor.focus_block(fence.entity_id()));
    redraw(cx);
    editor.update(cx, |editor, cx| editor.open_mermaid_preview(
        "```mermaid\nflowchart LR\nA-->B\n```",
        cx,
    ));
    redraw(cx);
    redraw(cx);

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let escape = gpui::Keystroke::parse("escape").expect("escape");
            assert!(
                editor.mermaid_preview_key_down(&escape, window, cx),
                "Esc 应被预览拦下"
            );
        });
    });
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(editor.mermaid_preview.is_none(), "Esc 后预览应关闭");
    });

    // 焦点回到正文块：继续打字应落进文档。
    cx.simulate_input("x");
    redraw(cx);
    editor.read_with(cx, |editor, _| {
        assert_ne!(
            editor.buffer.text(),
            markdown,
            "Esc 之后焦点该回到正文块，打字仍应落进文档"
        );
    });
}
