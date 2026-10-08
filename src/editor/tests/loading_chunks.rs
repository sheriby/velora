use super::common::*;

#[gpui::test]
#[ignore = "手动大文件诊断；设置 VELORA_PERF_FILE 后单独运行"]
async fn manual_markdown_load_probe(cx: &mut TestAppContext) {
    let path = std::env::var("VELORA_PERF_FILE").expect("需要设置 VELORA_PERF_FILE");
    let markdown = fs::read_to_string(path).expect("性能样本必须是 UTF-8 文本");
    let bytes = markdown.len();
    init_editor_test_app(cx);

    let start = Instant::now();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        let start = Instant::now();
        let editor = Editor::from_markdown(cx, markdown, None);
        println!(
            "editor_constructor_ms={:.1}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        editor
    });
    let construct = start.elapsed();
    let (mode, rows) = editor.read_with(cx, |editor, _cx| {
        (editor.view_mode, editor.document.visible_blocks().len())
    });
    assert!(matches!(mode, ViewMode::Rendered));

    let start = Instant::now();
    redraw(cx);
    let first_draw = start.elapsed();
    let steady_count = std::env::var("VELORA_STEADY_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let mut steady_draws = Vec::with_capacity(steady_count);
    for _ in 0..steady_count {
        let start = Instant::now();
        redraw(cx);
        steady_draws.push(start.elapsed().as_secs_f64() * 1000.0);
        if steady_count > 100 {
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }
    steady_draws.sort_by(f64::total_cmp);
    let p95 = steady_draws[steady_draws.len() - 1];
    println!(
        "bytes={bytes} rows={rows} construct_ms={:.1} first_draw_ms={:.1} steady_p95_ms={p95:.1}",
        construct.as_secs_f64() * 1000.0,
        first_draw.as_secs_f64() * 1000.0
    );

    let start = Instant::now();
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        editor.active_entity_id = Some(first.entity_id());
        first.update(cx, |block, cx| {
            block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(0..0, "测", None, false, cx);
        });
    });
    let edit_update = start.elapsed();
    let start = Instant::now();
    redraw(cx);
    let edit_draw = start.elapsed();
    println!(
        "edit_update_ms={:.1} edit_draw_ms={:.1}",
        edit_update.as_secs_f64() * 1000.0,
        edit_draw.as_secs_f64() * 1000.0
    );
    let start = Instant::now();
    let source_bytes = editor.read_with(cx, |editor, cx| editor.current_document_source(cx).len());
    println!(
        "serialized_bytes={source_bytes} serialize_ms={:.1}",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

#[gpui::test]
#[ignore = "手动大文件诊断；设置 VELORA_PERF_FILE 后单独运行"]
async fn manual_code_load_probe(cx: &mut TestAppContext) {
    let path = std::env::var("VELORA_PERF_FILE").expect("需要设置 VELORA_PERF_FILE");
    let source_path = PathBuf::from(&path);
    let source = fs::read_to_string(&source_path).expect("性能样本必须是 UTF-8 文本");
    let bytes = source.len();
    init_editor_test_app(cx);

    let start = Instant::now();
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        let start = Instant::now();
        let editor = Editor::from_file_source(cx, source, Some(source_path));
        println!(
            "editor_constructor_ms={:.1}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        editor
    });
    let construct = start.elapsed();
    let (mode, rows) = editor.read_with(cx, |editor, _cx| {
        (
            editor.view_mode,
            editor.document.visible_blocks().len(),
        )
    });
    assert!(matches!(mode, ViewMode::Source), "代码文件应进入 Source 模式");

    let start = Instant::now();
    redraw(cx);
    let first_draw = start.elapsed();
    let steady_count = std::env::var("VELORA_STEADY_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let mut steady_draws = Vec::with_capacity(steady_count);
    for _ in 0..steady_count {
        let start = Instant::now();
        redraw(cx);
        steady_draws.push(start.elapsed().as_secs_f64() * 1000.0);
        if steady_count > 100 {
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
    }
    steady_draws.sort_by(f64::total_cmp);
    let p95 = steady_draws[steady_draws.len() - 1];
    println!(
        "bytes={bytes} rows={rows} construct_ms={:.1} first_draw_ms={:.1} steady_p95_ms={p95:.1}",
        construct.as_secs_f64() * 1000.0,
        first_draw.as_secs_f64() * 1000.0,
    );

    let start = Instant::now();
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        editor.active_entity_id = Some(first.entity_id());
        first.update(cx, |block, cx| {
            block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(0..0, "x", None, false, cx);
        });
    });
    let edit_update = start.elapsed();
    let start = Instant::now();
    redraw(cx);
    let edit_draw = start.elapsed();
    println!(
        "edit_update_ms={:.1} edit_draw_ms={:.1}",
        edit_update.as_secs_f64() * 1000.0,
        edit_draw.as_secs_f64() * 1000.0
    );
    let start = Instant::now();
    let source_bytes = editor.read_with(cx, |editor, cx| editor.current_document_source(cx).len());
    println!(
        "serialized_bytes={source_bytes} serialize_ms={:.1}",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

#[gpui::test]
async fn code_source_chunks_round_trip_and_continue_line_numbers(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let mut source = String::new();
    for index in 0..1_200 {
        source.push_str(&format!("line-{index}\n"));
    }
    let path = temp_fixture_dir().join(format!("velora-chunk-roundtrip-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");
    let expected_source = source.clone();

    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_file_source(cx, source.clone(), Some(path.clone()))
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert!(matches!(editor.view_mode, ViewMode::Source));
        let blocks = editor.document.visible_blocks();
        assert_eq!(blocks.len(), 3, "1200 行 + 行尾换行应切成 512/512/177 三块");

        let mut expected_start = 1usize;
        for visible in blocks {
            visible.entity.read_with(cx, |block, _cx| {
                assert_eq!(block.source_line_start(), expected_start);
            });
            expected_start += visible.entity.read_with(cx, |block, _cx| {
                block.display_text().split('\n').count()
            });
        }

        let serialized = editor.current_document_source(cx);
        assert_eq!(serialized, expected_source, "分块序列化必须逐字节还原源码");
    });
}

#[gpui::test]
async fn code_source_with_crlf_round_trips_through_chunks(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 1200 行 > 一个源码块（512 行）：这份文档走的是分块续建这条路。
    let mut crlf_source = String::new();
    for index in 0..1200 {
        crlf_source.push_str(&format!("alpha {index}\r\n"));
    }
    let expected_bytes = crlf_source.clone().into_bytes();
    let path = temp_fixture_dir().join(format!("velora-chunk-crlf-{}.txt", std::process::id()));
    std::fs::write(&path, &crlf_source).expect("seed CRLF source");
    let cleanup = path.clone();
    cx.on_quit(move || {
        let _ = std::fs::remove_file(&cleanup);
    });
    let document = crate::editor::encoding::load_document(&path).expect("read fixture");

    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(path.clone()))
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.visible_blocks().len(),
            3,
            "1200 行源码应分成 512/512/177 三块再续建"
        );
        // 保存的字节来自缓冲区：没改过的 CRLF 代码文档，落盘就该是原样那串字节。
        assert_eq!(editor.buffer.file_bytes(), expected_bytes);
        // 投影本身也要完整：兜底重投影那一档拿它当内容源，缺行就是丢内容。
        assert_eq!(
            editor.document.raw_source_text(cx).replace('\n', "\r\n"),
            crlf_source
        );
    });
}

#[gpui::test]
async fn small_code_files_stay_single_chunk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = "one\ntwo\nthree\n".to_string();
    let path = temp_fixture_dir().join(format!("velora-chunk-small-{}.toml", std::process::id()));
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_file_source(cx, source, Some(path)));
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.document.visible_blocks().len(), 1);
    });
}

#[gpui::test]
async fn progressive_import_blocks_render_after_streaming(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    // 12000 块 > FIRST_CHUNK_ROOTS(2000)：首帧只建 2000，其余流式续建。
    let mut markdown = String::new();
    for index in 0..6_000 {
        markdown.push_str(&format!("# Heading {index}\n\nParagraph {index} body text.\n\n"));
    }
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));

    // 流式续建排空后，行计划必须反映全部块（此前 plan 键不含块数，
    // 续建只 append_roots + notify，渲染停留在首帧的 2000 行）。
    cx.run_until_parked();
    redraw(cx);
    redraw(cx);

    editor.read_with(cx, |editor, _cx| {
        let visible = editor.document.visible_blocks().len();
        assert!(
            (11_900..=12_100).contains(&visible),
            "流式续建完成后可见块应全部就位，实际 {}",
            visible
        );
        let plan_rows = editor
            .rendered_row_plan
            .as_ref()
            .expect("行计划应在渲染后存在")
            .rows
            .len();
        assert_eq!(
            plan_rows, visible,
            "行计划必须随流式续建刷新，否则新块不渲染（用户可见大片空白）"
        );
    });
}

/// 700 行 + 行尾换行 → 701 个行片段 → 512/189 两块。
pub(super) fn chunk_boundary_source() -> String {
    let mut source = String::new();
    for index in 0..700 {
        source.push_str(&format!("line-{index}\n"));
    }
    source
}

#[gpui::test]
async fn backspace_at_chunk_start_merges_previous_chunk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = chunk_boundary_source();
    let path = temp_fixture_dir().join(format!("velora-chunk-bs-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");
    let expected_source = source.clone();
    let expected_merged = source.replace("line-511\nline-512", "line-511line-512");

    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    cx.run_until_parked();
    editor
        .update(cx, |editor, window, cx| {
            let blocks = editor.document.flatten_visible_blocks();
            assert_eq!(blocks.len(), 2);
            let second = blocks[1].entity.clone();
            second.update(cx, |block, cx| {
                block.selected_range = 0..0;
                block.on_delete_back(&DeleteBack, window, cx);
            });
        })
        .expect("editor window should be open");
    cx.run_until_parked();

    editor
        .read_with(cx, |editor, cx| {
            assert_eq!(
                editor.document.visible_blocks().len(),
                1,
                "块首退格应并入前块"
            );
            assert_eq!(editor.current_document_source(cx), expected_merged);
        })
        .expect("editor window should be open");

    editor
        .update(cx, |editor, _window, cx| editor.undo_document(cx))
        .expect("editor window should be open");
    cx.run_until_parked();
    editor
        .read_with(cx, |editor, cx| {
            assert_eq!(editor.document.visible_blocks().len(), 2, "undo 恢复分块");
            assert_eq!(editor.current_document_source(cx), expected_source);
        })
        .expect("editor window should be open");
}

#[gpui::test]
async fn enter_at_chunk_end_inserts_boundary_chunk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = chunk_boundary_source();
    let path = temp_fixture_dir().join(format!("velora-chunk-enter-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");
    let expected_source = source.replace("line-511\nline-512", "line-511\n\nline-512");

    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    cx.run_until_parked();
    editor
        .update(cx, |editor, window, cx| {
            let blocks = editor.document.flatten_visible_blocks();
            assert_eq!(blocks.len(), 2);
            let first = blocks[0].entity.clone();
            let end = first.read(cx).display_text().len();
            first.update(cx, |block, cx| {
                block.selected_range = end..end;
                block.on_newline(&Newline, window, cx);
            });
        })
        .expect("editor window should be open");
    cx.run_until_parked();

    editor
        .read_with(cx, |editor, cx| {
            let blocks = editor.document.visible_blocks();
            assert_eq!(blocks.len(), 3, "块尾回车应插入一个空块");
            let starts: Vec<usize> = blocks
                .iter()
                .map(|visible| visible.entity.read(cx).source_line_start())
                .collect();
            assert_eq!(starts, vec![1, 513, 514], "行号续号应随插入刷新");
            assert_eq!(editor.current_document_source(cx), expected_source);
        })
        .expect("editor window should be open");
}

#[gpui::test]
async fn delete_at_chunk_end_merges_next_chunk(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let source = chunk_boundary_source();
    let path = temp_fixture_dir().join(format!("velora-chunk-del-{}.log", std::process::id()));
    fs::write(&path, &source).expect("write chunk fixture");
    let expected_merged = source.replace("line-511\nline-512", "line-511line-512");

    let editor =
        cx.update(|cx| crate::app_menu::open_editor_window(cx, source.clone(), Some(path)));
    cx.run_until_parked();
    editor
        .update(cx, |editor, window, cx| {
            let blocks = editor.document.flatten_visible_blocks();
            assert_eq!(blocks.len(), 2);
            let first = blocks[0].entity.clone();
            let end = first.read(cx).display_text().len();
            first.update(cx, |block, cx| {
                block.selected_range = end..end;
                block.on_delete(&Delete, window, cx);
            });
        })
        .expect("editor window should be open");
    cx.run_until_parked();

    editor
        .read_with(cx, |editor, cx| {
            assert_eq!(
                editor.document.visible_blocks().len(),
                1,
                "块尾前删应并入后块"
            );
            assert_eq!(editor.current_document_source(cx), expected_merged);
        })
        .expect("editor window should be open");
}

#[gpui::test]
async fn targeted_source_mapping_matches_later_blocks_and_table_cells(cx: &mut TestAppContext) {
    init_editor_test_app(cx);
    let markdown = "intro\n\n## heading\n\n| Name | Value |\n| --- | --- |\n| A | B |".into();
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_markdown(cx, markdown, None));
    editor.read_with(cx, |editor, cx| {
        let all = editor.build_source_target_mappings(cx);
        assert!(all.len() >= 4);
        for expected in all.iter().skip(1) {
            let actual = editor
                .source_mapping_for_entity(expected.entity.entity_id(), cx)
                .expect("later block or table cell should have a source mapping");
            assert_eq!(actual.full_source_range, expected.full_source_range);
            assert_eq!(actual.content_to_source, expected.content_to_source);
            assert_eq!(actual.source_to_content, expected.source_to_content);
        }
    });
}
