use super::super::{
    Editor, SearchMatcher, SearchOptions, TreeSortPreference, WorkspaceTab, has_utf16_bom,
    is_likely_text_file,
    scan_workspace_dir, search_utf8_to_utf16, search_utf16_to_utf8,
    search_workspace_files,
};
use crate::components::UndoCaptureKind;
use gpui::{
    ClipboardItem, EntityInputHandler, Modifiers,
    TestAppContext, px,
};
use std::fs;
use std::time::Duration;


#[test]
fn search_cache_detects_a_rewrite_with_a_restored_mtime() {
    // 审查发现：搜索内容缓存只比 mtime。粗粒度文件系统（秒级/更粗）里
    // 内容改了但 mtime 没变时，搜索会一直读到旧内容。
    let path = std::env::temp_dir().join(format!(
        "velora-search-cache-len-{}.md",
        uuid::Uuid::new_v4()
    ));
    fs::write(&path, "alpha").unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(
        &*super::super::cached_file_source(&path).expect("first read"),
        "alpha"
    );
    fs::write(&path, "beta beta").unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(before).unwrap();
    assert_eq!(
        &*super::super::cached_file_source(&path).expect("second read"),
        "beta beta",
        "mtime 未变但长度变了，缓存必须失效"
    );
    let _ = fs::remove_file(&path);
}

#[test]
fn search_offsets_keep_cjk_and_emoji_boundaries() {
    let text = "中😀a";
    assert_eq!(search_utf16_to_utf8(text, 1), "中".len());
    assert_eq!(search_utf16_to_utf8(text, 2), "中".len());
    assert_eq!(search_utf16_to_utf8(text, 3), "中😀".len());
    assert_eq!(search_utf8_to_utf16(text, "中😀".len()), 3);
}

#[test]
fn utf16_bom_detection() {
    let root = std::env::temp_dir().join(format!("velora-bom-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("create root");

    let le = root.join("le.txt");
    std::fs::write(&le, [0xFF, 0xFE, b'a', 0x00]).expect("write LE");
    assert!(has_utf16_bom(&le));

    let be = root.join("be.txt");
    std::fs::write(&be, [0xFE, 0xFF, 0x00, b'a']).expect("write BE");
    assert!(has_utf16_bom(&be));

    let utf8 = root.join("utf8.txt");
    std::fs::write(&utf8, "plain utf-8 text").expect("write utf8");
    assert!(!has_utf16_bom(&utf8));

    let _ = std::fs::remove_dir_all(root);
}

pub(super) fn plain_matcher(query: &str) -> SearchMatcher {
    SearchMatcher::new(query, SearchOptions::default())
}

#[gpui::test]
async fn workspace_search_accepts_unicode_platform_input(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            // 侧栏默认关闭（用户需求）：搜索输入框在抽屉里，先展开再聚焦。
            editor.workspace.is_open = true;
            editor.workspace.active_tab = super::super::WorkspaceTab::Search;
            let focus = editor
                .workspace
                .search_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            window.focus(&focus);
            cx.notify();
        });
        window.draw(cx).clear();
    });
    cx.simulate_input("你好");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_query, "你好");
        assert_eq!(
            editor.workspace.search_selected_range,
            "你好".len().."你好".len()
        );
    });
    cx.simulate_keystrokes("backspace");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_query, "你");
    });
    cx.update(|_window, cx| cx.write_to_clipboard(ClipboardItem::new_string("世界".into())));
    cx.simulate_keystrokes("cmd-v");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(editor.workspace.search_query, "你世界");
    });
}

#[gpui::test]
async fn workspace_search_waits_for_ime_commit_before_searching(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.workspace.active_tab = super::super::WorkspaceTab::Search;
            let generation = editor.workspace.search_generation;
            editor.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
            assert_eq!(editor.workspace.search_query, "ni");
            assert_eq!(editor.workspace.search_generation, generation);
            editor.replace_and_mark_text_in_range(None, "你", Some(1..1), window, cx);
            assert_eq!(editor.workspace.search_query, "你");
            assert_eq!(editor.workspace.search_generation, generation);
            editor.replace_text_in_range(None, "你", window, cx);
            assert!(editor.workspace.search_marked_range.is_none());
            assert_eq!(editor.workspace.search_generation, generation + 1);
        });
    });
}

/// 一份长于 8 KiB、且第 8192 个字节正好落在三字节汉字中间的 Markdown。
fn long_cjk_markdown() -> String {
    let mut content = String::from("# 长篇中文文档\n\n");
    while content.len() + "中文内容。".len() <= 8191 {
        content.push_str("中文内容。");
    }
    while content.len() < 8191 {
        content.push('a');
    }
    assert_eq!(content.len(), 8191);
    content.push('中');
    content.push_str("\n\n结尾段落\n");
    content
}

#[test]
fn text_sniffing_accepts_a_prefix_cut_mid_character() {
    // 用户报修：8 KiB 读取窗切在多字节字符中间时，窗口内不是合法 UTF-8，
    // 于是整篇中文文档被判成二进制，只显示「无法使用文本编辑器预览该文件」。
    let root = std::env::temp_dir().join(format!("velora-sniff-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    let path = root.join("长篇.md");
    fs::write(&path, long_cjk_markdown()).expect("write markdown");

    let bytes = fs::read(&path).expect("read back");
    assert!(bytes.len() > 8192, "用例前提：文件要长于读取窗");
    assert!(
        std::str::from_utf8(&bytes[..8192]).is_err(),
        "用例前提：8192 字节必须切在多字节字符中间"
    );
    assert!(is_likely_text_file(&path), "切在多字节字符中间的文本前缀仍是文本");

    let binary = root.join("blob.bin");
    fs::write(&binary, [0u8, 1, 2, 3, 0, 5]).expect("write binary");
    assert!(!is_likely_text_file(&binary), "含 NUL 的文件不该被当成文本");
    let invalid = root.join("invalid.md");
    fs::write(&invalid, [b'a', 0xE5, 0x20, 0x20, 0x20]).expect("write invalid utf8");
    assert!(
        !is_likely_text_file(&invalid),
        "窗口内的非法 UTF-8（非截断）仍应是二进制"
    );
    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn opening_a_large_cjk_markdown_file_shows_the_editor(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-cjk-open-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    let path = root.join("长篇.md");
    fs::write(&path, long_cjk_markdown()).expect("write markdown");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx);
        });
    });
    editor.read_with(cx, |editor, cx| {
        assert!(
            editor.unsupported_preview_path.is_none(),
            "长中文 md 文件应正常打开，不该显示「无法预览」占位"
        );
        assert_eq!(editor.file_path.as_ref(), Some(&path));
        assert!(editor.document.markdown_text(cx).contains("结尾段落"));
    });
    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn workspace_search_matches_file_names_and_contents(cx: &mut TestAppContext) {
    let background = cx.executor();
    let root =
        std::env::temp_dir().join(format!("velora-workspace-search-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("src")).expect("create source dir");
    fs::write(
        root.join("README.md"),
        "search term is only in file content",
    )
    .expect("write md");
    fs::write(root.join("src").join("main.rs"), "fn main() {}").expect("write code");
    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");

    let matches = search_workspace_files(&tree, &SearchMatcher::new("MAIN", SearchOptions::default()), 200, &background).await;
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].label, "src/main.rs");
    assert_eq!(matches[0].line, None);
    assert_eq!(matches[1].line, Some(1));

    let matches = search_workspace_files(&tree, &SearchMatcher::new("readme", SearchOptions::default()), 200, &background).await;
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].label, "README.md");

    let matches = search_workspace_files(&tree, &SearchMatcher::new("content", SearchOptions::default()), 200, &background).await;
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].label, "README.md");
    assert_eq!(matches[0].line, Some(1));
    assert!(matches[0].preview.contains("content"));
    assert!(search_workspace_files(&tree, &SearchMatcher::new("absent", SearchOptions::default()), 200, &background).await.is_empty());

    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn clicking_a_search_hit_in_a_dirty_file_lands_on_the_match(cx: &mut TestAppContext) {
    // 审查发现：工作区搜索读磁盘快照，跳转偏移却套在未保存的内存文本上，
    // 脏文件点搜索结果会跳错位置或静默落到空区间。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-dirty-search-hit-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    // 与树/搜索产出的 canonical 路径对齐（macOS /var → /private/var）。
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let path = root.join("note.md");
    fs::write(&path, "first para\n\nbeta target\n").unwrap();
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
            editor.open_workspace_file(path.clone(), window, cx);
        });
    });
    cx.run_until_parked();
    // 未保存的编辑：在开头插入两行，磁盘上的 line = 3 在内存里已推到 line = 5。
    editor.update(cx, |editor, cx| {
        let block = editor.document.root_blocks()[0].clone();
        block.update(cx, |block, cx| {
            block.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            block.replace_text_in_visible_range(0..0, "inserted line\n\n", None, false, cx);
        });
    });
    cx.run_until_parked();

    editor.update(cx, |editor, cx| {
        editor.workspace.search_query = "beta".into();
        editor.workspace.search_scope = super::super::WorkspaceSearchScope::Workspace;
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(400));
    cx.run_until_parked();
    let index = editor.read_with(cx, |editor, _| {
        editor
            .workspace
            .search_results
            .iter()
            .position(|hit| hit.path == path && hit.line == Some(3))
            .expect("磁盘快照应把命中记在第 3 行")
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.open_search_hit(index, window, cx));
    });
    editor.read_with(cx, |editor, cx| {
        let range = editor
            .workspace
            .document_active_range
            .clone()
            .expect("点击结果应跳到命中");
        let source = editor.current_document_source(cx);
        assert_eq!(
            &source[range],
            "beta",
            "脏文件里的跳转必须落在命中文本上"
        );
    });
}

#[gpui::test]
async fn workspace_search_cache_picks_up_modified_content(cx: &mut TestAppContext) {
    // 内容缓存：mtime 未变走内存；文件被改写后必须反映新内容（用户报修
    // 的性能优化不能牺牲正确性）。
    let background = cx.executor();
    let root =
        std::env::temp_dir().join(format!("velora-search-cache-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create dir");
    let note = root.join("note.md");
    fs::write(&note, "alpha only").expect("write");

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
    let matcher = SearchMatcher::new("alpha", SearchOptions::default());
    assert_eq!(search_workspace_files(&tree, &matcher, 200, &background).await.len(), 1);
    // 第二轮：命中缓存仍能找到。
    assert_eq!(search_workspace_files(&tree, &matcher, 200, &background).await.len(), 1);

    // 改写文件后缓存必须失效。
    fs::write(&note, "beta instead").expect("rewrite");
    // 缓存的失效依据是 mtime，而文件系统的 mtime 粒度可能是一秒甚至更粗：
    // 两次写在几十毫秒内发生时时间戳可能一模一样，缓存就会以为文件没变。
    // 这里显式把 mtime 往后推，让「文件已改」这件事与文件系统粒度无关。
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&note) {
        let _ = file.set_modified(
            std::time::SystemTime::now() + std::time::Duration::from_secs(2),
        );
    }
    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("rescan tree");
    let fresh = SearchMatcher::new("beta", SearchOptions::default());
    assert_eq!(
        search_workspace_files(&tree, &fresh, 200, &background).await.len(),
        1,
        "改写后应搜到新内容"
    );
    let stale = SearchMatcher::new("alpha", SearchOptions::default());
    assert!(
        search_workspace_files(&tree, &stale, 200, &background).await.is_empty(),
        "改写后不应再搜到旧内容"
    );

    let _ = fs::remove_dir_all(root);
}

#[gpui::test]
async fn workspace_search_returns_content_hits_after_typing(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root =
        std::env::temp_dir().join(format!("velora-content-search-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("notes.md"), "first line\n独特的内容在这里\n").unwrap();
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });
    let (editor, cx) =
        cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root, cx);
        editor.workspace.active_tab = super::super::WorkspaceTab::Search;
        editor.workspace.search_query = "独特".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.workspace.search_results.len(), 1);
        assert_eq!(editor.workspace.search_results[0].line, Some(2));
        assert_eq!(editor.workspace.search_results[0].label, "notes.md");
    });
}

#[gpui::test]
async fn search_result_file_header_opens_the_file_and_has_no_empty_row(cx: &mut TestAppContext) {
    // 用户报修：搜索结果里文件名本身点不了，只有它下面一条没有内容的空行能点。
    // 原因是文件名命中（line=None）也渲染了一行（py(4) 且无内容），而文件头
    // 完全没有点击处理。现在文件头整行可点并代表该组第一条命中。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-search-header-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(root.join("assets")).unwrap();
    // 含 NUL 才能稳定判成不可预览文件（用户截图里的 png 场景）。
    fs::write(root.join("assets").join("velora-banner.png"), [0u8, 1, 2, 3]).unwrap();
    fs::write(root.join("velora-notes.md"), "开头\nvelora 命中行\n").unwrap();
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
        editor.workspace.active_tab = super::super::WorkspaceTab::Search;
        editor.workspace.search_query = "velora".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());

    let hits = editor.read_with(cx, |editor, _| {
        editor
            .workspace
            .search_results
            .iter()
            .map(|hit| (hit.label.clone(), hit.line))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        hits,
        vec![
            ("assets/velora-banner.png".to_string(), None),
            ("velora-notes.md".to_string(), None),
            ("velora-notes.md".to_string(), Some(2)),
        ],
        "目录在前，文件名命中在前，内容命中带行号"
    );

    // 文件名命中不再有独立空行；内容命中仍然渲染自己的行。
    assert!(
        cx.debug_bounds("workspace-search-hit-0").is_none(),
        "文件名命中不应再渲染一条看不见的空行"
    );
    assert!(
        cx.debug_bounds("workspace-search-hit-1").is_none(),
        "文件名命中不应再渲染一条看不见的空行"
    );
    assert!(cx.debug_bounds("workspace-search-hit-2").is_some());

    let header = cx
        .debug_bounds("workspace-search-file-0")
        .expect("首个文件头应渲染为可点击行");
    assert!(
        header.size.height > px(16.0),
        "点击区应覆盖整行文件头，实测高度 {:?}",
        header.size.height
    );
    assert!(header.size.width > px(60.0));

    cx.simulate_click(header.center(), Modifiers::none());
    cx.update(|window, cx| window.draw(cx).clear());
    // macOS 的 /var 会被打开流程规范化为 /private/var，断言前统一 canonicalize。
    let banner = fs::canonicalize(root.join("assets").join("velora-banner.png"))
        .expect("canonical banner path");
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor
                .unsupported_preview_path
                .as_ref()
                .and_then(|path| fs::canonicalize(path).ok()),
            Some(banner.clone()),
            "点文件名应打开该文件（png 走不可预览占位）"
        );
    });

    // 同一组里既有文件名命中又有内容命中时，文件头也负责打开文件。
    let notes_header = cx
        .debug_bounds("workspace-search-file-1")
        .expect("第二个文件头应渲染");
    cx.simulate_click(notes_header.center(), Modifiers::none());
    cx.update(|window, cx| window.draw(cx).clear());
    let notes = fs::canonicalize(root.join("velora-notes.md")).expect("canonical notes path");
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor
                .file_path
                .as_ref()
                .and_then(|path| fs::canonicalize(path).ok()),
            Some(notes.clone()),
            "点文件头应打开对应的 Markdown 文件"
        );
        assert!(editor.unsupported_preview_path.is_none());
    });
}

#[gpui::test]
async fn re_search_keeps_previous_results_visible(cx: &mut TestAppContext) {
    // 用户报修：点击搜索结果后侧栏闪一下——先空白再恢复。触发重新搜索的来源
    // 很多（watcher 刷新文件树、重新调度等），但闪空的根因是重新搜索一开始就
    // 清空 `search_results`，面板在 120ms 去抖窗口里只剩「…」。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!(
        "velora-search-keep-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::write(root.join("assets").join("velora-banner.png"), [0u8, 1, 2, 3]).unwrap();
    fs::write(root.join("velora-notes.md"), "开头\nvelora 命中行\n").unwrap();
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
        editor.workspace.active_tab = super::super::WorkspaceTab::Search;
        editor.workspace.search_query = "velora".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear());
    let baseline = editor.read_with(cx, |editor, _| editor.workspace.search_results.len());
    assert_eq!(baseline, 3);

    // 模拟「打开文件后 watcher 触发文件树刷新」：这会重新调度搜索。
    editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.search_pending, "重新搜索应处于进行中");
        assert_eq!(
            editor.workspace.search_results.len(),
            baseline,
            "重新搜索期间应继续显示旧结果，而不是先清空"
        );
    });
    cx.update(|window, cx| window.draw(cx).clear());
    assert!(
        cx.debug_bounds("workspace-search-file-0").is_some(),
        "重新搜索期间侧栏不应闪成空白"
    );
    assert!(cx.debug_bounds("workspace-search-hit-2").is_some());

    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(!editor.workspace.search_pending);
        assert_eq!(editor.workspace.search_results.len(), baseline);
    });

    // 查询被清空时必须立刻丢掉旧结果（面板回到空态），不能留着过期结果。
    editor.update(cx, |editor, cx| {
        editor.workspace.search_query.clear();
        editor.schedule_workspace_search(cx);
    });
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.search_results.is_empty());
        assert!(!editor.workspace.search_pending);
    });
}


use crate::components::BlockKind;

#[gpui::test]
async fn workspace_search_jump_scrolls_to_unpainted_matches(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    // 早命中在文档开头、晚命中在末尾：跳转必须双向跟随——向下跳进从未
    // 绘制过的区域（目标块没有布局边界，精确居中无从算起），向上跳反向
    // 滚回首屏（用户报修：浏览过一遍之前跳转不响应；浏览=全部画过=边界齐）。
    let mut big = String::from("## Top\n\n开头段落就有测试命中\n");
    for index in 0..200 {
        big.push_str(&format!(
            "\n段落 {}：需要一些正文文本把布局撑开，越远越不容易被绘制。\n",
            index
        ));
    }
    big.push_str("\n## 沟通风格\n\n- 专业、技术、简洁\n\n末尾再来一次性能测试，制造深处的命中。\n");
    let root = std::env::temp_dir().join(format!("velora-search-jump-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("mkdir");
    std::fs::write(root.join("big.md"), &big).expect("write big");
    std::fs::write(root.join("start.md"), "# start\n\nseed\n").expect("write start");
    let big_path = std::fs::canonicalize(root.join("big.md")).expect("canonicalize");

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(root.clone(), cx);
    });
    cx.run_until_parked();

    // 端到端复刻：搜索面板开在「所有文件」范围，查询后点击另一篇文档的命中
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(root.join("start.md"), window, cx);
            editor.workspace.is_open = true;
            editor.workspace.active_tab = WorkspaceTab::Search;
            editor.workspace.search_query = "测试".into();
            editor.schedule_workspace_search(cx);
        });
    });
    cx.run_until_parked();
    cx.executor().advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    let (early_index, later_index) = editor.read_with(cx, |editor, _cx| {
        let mut early = None;
        let mut later = None;
        for (index, hit) in editor.workspace.search_results.iter().enumerate() {
            if hit.path != big_path {
                continue;
            }
            if hit.line.is_some_and(|line| line < 100) {
                early = early.or(Some(index));
            } else {
                later = Some(index);
            }
        }
        (early.expect("early hit"), later.expect("late hit"))
    });

    // 1) 先跳文档末尾的命中：视口必须滚进从未绘制过的深处
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_search_hit(later_index, window, cx);
        });
    });
    for _ in 0..16 {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }
    let scroll_at_later = editor.read_with(cx, |editor, _cx| {
        f32::from(editor.scroll_handle.offset().y)
    });
    assert!(
        scroll_at_later < -2000.0,
        "向下跳转必须滚进深处：scroll_y={scroll_at_later}"
    );

    // 2) 再向上跳回开头的命中：视口必须反向滚回首屏
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_search_hit(early_index, window, cx);
        });
    });
    for _ in 0..16 {
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }
    let scroll_at_early = editor.read_with(cx, |editor, _cx| {
        f32::from(editor.scroll_handle.offset().y)
    });
    assert!(
        scroll_at_early.abs() < 500.0,
        "向上跳转必须反向滚回首屏：scroll_y={scroll_at_early}"
    );

    // 3) 活动块选区落在命中上、高亮与活动标记齐备
    editor.read_with(cx, |editor, cx| {
        let active = editor
            .active_entity_id
            .and_then(|id| {
                editor
                    .document
                    .visible_blocks()
                    .into_iter()
                    .find(|visible| visible.entity.entity_id() == id)
                    .map(|visible| visible.entity.clone())
            })
            .expect("active block");
        let block = active.read(cx);
        assert!(
            !block.selected_range.is_empty(),
            "活动块应有命中的选区"
        );
        assert!(
            block
                .search_highlight_ranges
                .iter()
                .any(|range| range == &block.selected_range),
            "选区应落在某个测试命中上"
        );
        assert_eq!(
            block.search_active_range,
            Some(block.selected_range.clone()),
            "活动命中应有更深的独立标记"
        );
        assert!(editor.cross_block_selection.is_none());
    });

    // 4) 工作区范围循环跳转（Enter / 上一个下一个小按钮）：此前是空操作
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let before = editor.workspace.search_active_index;
            editor.advance_search_match(false, window, cx);
            assert_ne!(
                editor.workspace.search_active_index, before,
                "工作区范围下 Enter 必须推进到下一个命中"
            );
            editor.advance_search_match(true, window, cx);
            assert_eq!(editor.workspace.search_active_index, before, "反向应绕回");
        });
    });
    let _ = std::fs::remove_dir_all(root);
}
