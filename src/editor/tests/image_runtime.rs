use super::common::*;

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
    let root = fs::canonicalize(root).expect("规范化夹具目录");
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
            window.draw(cx).clear();
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
            .find_map(|handle| handle.downcast::<Editor>())
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
