use super::super::Editor;
use gpui::{TestAppContext, px};
use std::fs;
use std::time::Duration;

/// 撑起可滚动的正文：每段一行，段间空行分开。给同级的标签用例复用。
pub(super) fn long_markdown(paragraphs: usize) -> String {
    (1..=paragraphs)
        .map(|index| format!("第 {index} 段：一行用来把正文撑出视口的中文内容。"))
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}

/// 外部改动判定看的是**内容版本**，不是时间戳：同长度、同 mtime 的一次改写
/// 也必须刷进界面。
///
/// 「文档在外部修改后没有刷新」这条报修（Windows 验收）最像时间戳判据的失效形状：
/// 快速保存只落在一个 mtime 刻度里，或者同步盘把时间戳原样贴回去。这里的夹具把两种
/// 情况一起造出来——替换文本与原文**逐字节等长**、写完再把 mtime 设回原来那一纳秒——
/// 只有按内容比对的实现才认得出这是一次改动。
#[gpui::test]
async fn an_external_edit_lands_in_the_view_with_the_same_size_and_mtime(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-external-same-size-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("note.md");
    // 9 字节；替换文本 `# 改文\n` 同样 9 字节（每个汉字 3 字节）。
    fs::write(&path, "# 原文\n").unwrap();
    let stamp = fs::metadata(&path).expect("metadata").modified().expect("mtime");
    let size_before = fs::metadata(&path).expect("metadata").len();
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
            editor.open_workspace_file(path.clone(), window, cx);
        })
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "# 原文");
    });

    fs::write(&path, "# 改文\n").unwrap();
    let handle = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open to stamp the mtime");
    handle.set_modified(stamp).expect("set_modified");
    let after = fs::metadata(&path).expect("metadata after");
    assert_eq!(after.len(), size_before, "夹具前提：替换必须等字节数");
    assert_eq!(
        after.modified().expect("mtime after"),
        stamp,
        "夹具前提：mtime 与改前同一刻度"
    );

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.reload_externally_changed_document(&path, cx)
        })
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx),
            "# 改文",
            "等长、同 mtime 的外部改动没刷进界面"
        );
    });
}

/// 文件被删掉又重建（换了一个 inode，尺寸可能一模一样）也要认出来。
///
/// 同步盘、`git checkout`、编辑器的「写临时文件再改名」都是这个形状：旧的那个文件
/// 没了，新文件的时间戳还可能被贴回旧的。删除那一次事件读不到内容，必须让后面的
/// 重建事件把新内容接上。
#[gpui::test]
async fn a_deleted_then_recreated_file_refreshes_the_view(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-external-recreate-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("note.md");
    fs::write(&path, "# 原文\n").unwrap();
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
            editor.open_workspace_file(path.clone(), window, cx);
        })
    });
    cx.run_until_parked();

    fs::remove_file(&path).unwrap();
    // 删除事件：这一趟读不到内容，界面保持最后一版（不能把正文清空）。
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.reload_externally_changed_document(&path, cx)
        })
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.markdown_text(cx), "# 原文");
    });

    fs::write(&path, "# 重建后的内容\n").unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.reload_externally_changed_document(&path, cx)
        })
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx),
            "# 重建后的内容",
            "重建后的内容没刷进界面"
        );
    });
}

/// 有未保存编辑时，外部改动**不能**被静默写进界面（那是丢用户的字），也**不能**
/// 被反向盖回磁盘：本地内容留在屏幕上，盘上那一版保持不动，冲突由保存路径处理。
#[gpui::test]
async fn an_external_edit_does_not_clobber_unsaved_local_edits(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-external-dirty-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("note.md");
    fs::write(&path, "# 原文\n").unwrap();
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
            editor.open_workspace_file(path.clone(), window, cx);
        })
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        let first = editor.document.first_root().expect("first block").clone();
        first.update(cx, |block, _cx| {
            block
                .record
                .set_title(crate::components::InlineTextTree::plain(
                    "本地未保存的改动".to_string(),
                ));
            block.sync_render_cache();
        });
        editor.mark_dirty(cx);
    });
    fs::write(&path, "# 外部改动\n").unwrap();

    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.reload_externally_changed_document(&path, cx)
        })
    });
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.document.markdown_text(cx).contains("本地未保存的改动"),
            "脏文档被外部内容顶掉了"
        );
    });
    assert_eq!(
        fs::read_to_string(&path).expect("read file"),
        "# 外部改动\n",
        "重载判定也不许顺手写盘"
    );
}

/// 外部改动读不出来时不能什么都不做（静默失效），要让用户看见并补一次复查。
///
/// 「文档在外部修改后没有刷新」的另一条可复现形状：监听事件到得很及时，但那一瞬间
/// 盘上的内容读不出来——别的程序正在写（半截的 UTF-16、读到 NUL 的字节流）、文件被
/// 同步盘换成了占位符、或者句柄还被上一个写者独占。`reload_externally_changed_document`
/// 以前把 `load_document` 的错误直接 `let Ok(..) else { return }` 吞掉：界面停在旧内容、
/// 没有任何提示，而且**再也不会有第二次机会**（很多平台这一次事件就是最后一次）。
/// 现在：这一趟先给一条侧栏错误说明为什么没刷，再补一次短延时的复查，写完了就接上。
#[gpui::test]
async fn an_unreadable_external_change_is_reported_and_rechecked(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-external-unreadable-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("note.md");
    fs::write(&path, "# 原文\n").unwrap();
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
            editor.open_workspace_file(path.clone(), window, cx);
        })
    });
    cx.run_until_parked();

    // 半截/不可无损表示的字节：读盘漏斗会拒绝（UTF-16 BOM + NUL + 孤立单元）。
    fs::write(&path, [0xFF, 0xFE, 0x00, 0x00, 0xFF, 0xD8]).expect("write junk");
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.reload_externally_changed_document(&path, cx)
        })
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.workspace.file_error.is_some(),
            "读不出外部改动也要说得出为什么，不能安静地留着旧正文"
        );
        assert!(
            !editor.document_dirty,
            "读不出来不是用户的编辑，不该把文档置脏"
        );
    });

    // 写完了：补的那一次复查要把新内容接上。
    fs::write(&path, "# 外部改完的内容\n").expect("write the finished file");
    cx.executor().advance_clock(Duration::from_millis(1_000));
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor.document.markdown_text(cx),
            "# 外部改完的内容",
            "补一次复查之后仍要能刷进界面"
        );
    });
    editor.read_with(cx, |editor, _cx| {
        assert!(
            editor.workspace.file_error.is_none(),
            "接上新内容之后那条提示该收回去"
        );
    });
}

#[gpui::test]
async fn external_file_events_refresh_the_workspace_tree(cx: &mut TestAppContext) {
    // 审查发现：watcher 只转发 Modify/Create 且从不刷新文件树，外部新建/
    // 删除/改名的文件在树、⌘P、工作区搜索的文件列表里永远是旧的。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-watcher-tree-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    // macOS 上 /var 是 /private/var 的符号链接：set_workspace_root 会把根
    // canonicalize，树里的路径全是 /private/var/...；测试断言用的路径必须
    // 与之同源，否则 path == 断言永远失败。
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let existing = root.join("a.md");
    fs::write(&existing, "# a\n").unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.workspace.is_open = true;
    });
    cx.run_until_parked();

    // 外部新建：树/文件列表要出现它。
    let added = root.join("b.md");
    fs::write(&added, "# b\n").unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&added, cx));
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.workspace_text_files().iter().any(|path| path == &added),
            "外部新建的文件应出现在工作区文件列表"
        );
    });

    // 外部删除：树/文件列表不能再列出它。
    fs::remove_file(&existing).unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&existing, cx));
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(
            !editor.workspace_text_files().iter().any(|path| path == &existing),
            "外部删除的文件不应再出现在工作区文件列表"
        );
    });
}

#[gpui::test]
async fn opening_a_single_file_starts_the_workspace_watcher(cx: &mut TestAppContext) {
    // 审查发现：只有打开文件夹才会启动 watcher；只打开一个文件时外部修改
    // 永远不会重载（D3 静默失效）。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-single-file-watch-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("note.md");
    fs::write(&path, "# note\n").unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown("# note\n".into(), Some(path.clone()), cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.root.is_some(), "打开单文件应隐含工作区根");
        // 测试里不起真实 OS watcher（fd 限制），断言“决定监听哪根”的接缝；
        // 实际的 notify 监听由真实运行验证。
        assert_eq!(
            editor.watched_workspace_root.as_deref(),
            editor.workspace.root.as_deref(),
            "隐含根也必须进入监听状态"
        );
    });
}

#[gpui::test]
async fn backlinks_panel_picks_up_an_external_link_to_the_active_document(
    cx: &mut TestAppContext,
) {
    // 审查发现：反链面板只按 document_revision 失效，别的文件在外部
    // 新增 [[链接]] 时面板一直显示旧结果。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-backlinks-external-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    // 与索引/树产出的 canonical 路径对齐（macOS /var → /private/var）。
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let active = root.join("a.md");
    let other = root.join("b.md");
    fs::write(&active, "# A\n").unwrap();
    fs::write(&other, "# B\n").unwrap();
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
        assert!(editor.link_panels.backlinks.is_empty(), "前置：还没有反链");
    });

    fs::write(&other, "# B\n\n[[a]]\n").unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&other, cx));
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    editor.update(cx, |editor, cx| editor.refresh_link_panels(cx));
    editor.read_with(cx, |editor, _| {
        assert!(
            editor
                .link_panels
                .backlinks
                .iter()
                .any(|path| path == &other),
            "外部新增的 [[a]] 必须出现在反链面板"
        );
    });
}

/// 外部改动重载只换内容，不换阅读现场：视图模式与视口位置都不许被重置
/// （用户报修：重载之后源码模式自动跳回所见即所得，且页面弹回文档顶部）。
#[gpui::test]
async fn reloading_an_externally_changed_document_keeps_source_mode_and_viewport(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-reload-source-view-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("long.md");
    fs::write(&path, long_markdown(400)).unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.workspace.is_open = false;
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());

    // 用户现场：源码模式 + 视口停在文档中段。
    editor.update(cx, |editor, cx| editor.toggle_view_mode(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.update(cx, |editor, cx| {
        editor.set_vertical_scroll_offset(px(-3000.0), cx);
    });
    let (mode_before, offset_before) = editor.read_with(cx, |editor, _| {
        (editor.view_mode, editor.scroll_handle.offset().y)
    });
    assert_eq!(
        mode_before,
        crate::editor::ViewMode::Source,
        "前置：应已切到源码模式"
    );
    assert!(
        offset_before < px(0.0),
        "前置：视口应已离开文档顶部，实测 {offset_before:?}"
    );

    // 外部改动：末尾追加一段，内容版本变了才会触发重载。走 watcher 的真实入口。
    fs::write(&path, format!("{}\n\n新增的一段。\n", long_markdown(400))).unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&path, cx))
    });
    cx.run_until_parked();
    // 重载会挂上滚动校正帧；视口若被拽回顶部就发生在这些帧里。
    for _ in 0..8 {
        cx.update(|window, cx| window.draw(cx).clear());
    }

    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.view_mode,
            crate::editor::ViewMode::Source,
            "外部重载把源码模式跳回了所见即所得"
        );
        assert_eq!(
            editor.scroll_handle.offset().y,
            offset_before,
            "外部重载把视口弹回了文档顶部"
        );
        // 块树要跟着模式走：源码视图按 512 行切缓冲区（这篇 400 段只有两根），
        // 渲染态才是 markdown 解析出的 400 根。只对模式不对树时，位置换算会把块起点
        // 当行首去问行号，`TextBuffer::line_of` 直接断言失败。
        assert!(
            editor.document.root_count() < 20,
            "源码模式下块树没跟着模式建，实测 {} 根（渲染态解析会是 400 根）",
            editor.document.root_count()
        );
    });
}

/// 渲染态同理：视口与光标都留在原处。
#[gpui::test]
async fn reloading_an_externally_changed_rendered_document_keeps_viewport_and_caret(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-reload-rendered-view-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("long.md");
    fs::write(&path, long_markdown(400)).unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.workspace.is_open = false;
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());

    editor.update(cx, |editor, cx| {
        editor.set_vertical_scroll_offset(px(-3000.0), cx);
        // 把光标放到文档中段的某一段上（第 150 段）。
        let block = editor.document.root_blocks()[150].clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, _cx| block.selected_range = 3..3);
    });
    let offset_before = editor.read_with(cx, |editor, _| editor.scroll_handle.offset().y);
    let caret_before = editor.read_with(cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx)
    });
    assert!(offset_before < px(0.0), "前置：视口应已离开顶部");
    assert!(
        caret_before.range.start > 0,
        "前置：光标应在文档中段，实测 {:?}",
        caret_before.range
    );

    fs::write(&path, format!("{}\n\n新增的一段。\n", long_markdown(400))).unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.reload_externally_changed_document(&path, cx)
        })
    });
    cx.run_until_parked();
    for _ in 0..8 {
        cx.update(|window, cx| window.draw(cx).clear());
    }

    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.view_mode,
            crate::editor::ViewMode::Rendered,
            "外部重载不该改动视图模式"
        );
        assert_eq!(
            editor.scroll_handle.offset().y,
            offset_before,
            "外部重载把视口弹回了文档顶部"
        );
    });
    let caret_after = editor.read_with(cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx)
    });
    assert_eq!(
        caret_after.range, caret_before.range,
        "外部重载把光标丢回了文档开头"
    );
}

/// 大代码文件（按 512 行切成投影块）外部重载后光标必须留在同一行：视口不滚，
/// 落点却跑回文件开头的话，下一次打字就打进了屏外的那一块。
#[gpui::test]
async fn reloading_a_chunked_code_document_keeps_the_caret(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-reload-chunked-code-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let root = fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("big.rs");
    let source = || {
        (1..=1100)
            .map(|index| format!("fn line_{index}() {{}}\n"))
            .collect::<String>()
    };
    fs::write(&path, source()).unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
        editor.workspace.is_open = false;
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.document.root_count(),
            3,
            "用例前提：1100 行按 512 行切成三根投影块"
        );
    });

    // 光标放进第 3 块（前两整块之外的落点，正是旧口径读错的地方）。
    editor.update(cx, |editor, cx| {
        let block = editor.document.root_blocks()[2].clone();
        editor.active_entity_id = Some(block.entity_id());
        block.update(cx, |block, _cx| block.selected_range = 5..5);
    });
    let caret_before = editor.read_with(cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx)
    });
    assert!(
        caret_before.range.start > 15_000,
        "前置：光标应在第 3 块（前两整块约 17 KB 之后），实测 {:?}",
        caret_before.range
    );

    fs::write(&path, format!("{}// 外部追加\n", source())).unwrap();
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.reload_externally_changed_document(&path, cx)
        })
    });
    cx.run_until_parked();

    let caret_after = editor.read_with(cx, |editor, cx| {
        editor.capture_source_selection_snapshot(cx)
    });
    assert_eq!(
        caret_after.range, caret_before.range,
        "外部重载把大代码文件的光标丢了"
    );
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.view_mode, crate::editor::ViewMode::Source);
    });
}

