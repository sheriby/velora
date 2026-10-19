use super::{workspace_menu_action_bounds, menu_label_bounds};
use super::super::{
    WorkspaceMenuAction,
    Editor, TreeSortPreference, WorkspaceSelection,
    WorkspaceState,
    WorkspaceTreeKind, WorkspaceTreeNode, build_outline_tree, clamp_workspace_panel_width,
    create_workspace_file, create_workspace_folder, is_code_file, tree_node_path,
    path_is_affected, prune_outline_state, remap_moved_path,
    collect_workspace_files_on_disk, file_node_id, find_workspace_node,
    scan_workspace_dir, scan_workspace_dir_level,
    scan_workspace_dir_recursive,
};
use crate::components::{Block, UndoCaptureKind};
use gpui::{
    AppContext, EntityInputHandler,
    TestAppContext, point, px,
};
use std::fs;
use std::path::{Path, PathBuf};

#[gpui::test]

async fn outline_rename_selects_heading_title(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown(
            "## Old Title\n\nbody".into(),
            None,
            cx,
        );
        editor.sync_workspace_outline(cx);
        editor.rename_outline_heading(0, cx);
    });
    editor.read_with(cx, |editor, cx| {
        let block = editor.document.first_root().expect("root");
        // 渲染模式标题块的内容坐标即整个标题文本。
        assert_eq!(
            block.read(cx).selected_range.clone(),
            0.."Old Title".len()
        );
    });
}

#[test]
fn workspace_scan_includes_markdown_and_code_files() {
    let root =
        std::env::temp_dir().join(format!("velora-workspace-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("nested")).expect("create dirs");
    fs::write(root.join("a.md"), "a").expect("write md");
    fs::write(root.join("a.txt"), "plain text").expect("write txt");
    fs::write(root.join("main.rs"), "fn main() {}").expect("write code");
    fs::write(root.join("nested").join("b.md"), "b").expect("write nested md");

    let tree = scan_workspace_dir_recursive(&root, TreeSortPreference::Name).expect("scan tree");
    let labels = tree
        .children
        .iter()
        .map(|node| node.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["nested", "a.md", "a.txt", "main.rs"]);
    assert!(matches!(
        tree.children[0].kind,
        WorkspaceTreeKind::Directory(_)
    ));
    assert!(matches!(
        tree.children[1].kind,
        WorkspaceTreeKind::MarkdownFile(_)
    ));
    assert!(matches!(
        tree.children[2].kind,
        WorkspaceTreeKind::CodeFile(_)
    ));
    assert!(matches!(
        tree.children[3].kind,
        WorkspaceTreeKind::CodeFile(_)
    ));

    // Type sort groups files by extension before name (roadmap D2).
    let typed = scan_workspace_dir_recursive(&root, TreeSortPreference::Type).expect("scan typed");
    let extension_at = |index: usize| {
        tree_node_path(&typed.children[index])
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned())
    };
    let first = extension_at(0);
    let second = extension_at(1);
    if let (Some(first), Some(second)) = (first.clone(), second) {
        assert!(
            first <= second,
            "extensions not ordered: {first} > {second}"
        );
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_tree_first_load_reads_only_root_children() {
    let root = std::env::temp_dir().join(format!("velora-wide-tree-{}", uuid::Uuid::new_v4()));
    for index in 0..32 {
        let directory = root.join(format!("folder-{index}"));
        fs::create_dir_all(directory.join("nested")).expect("创建目录");
        fs::write(directory.join("nested/note.md"), "正文").expect("写入嵌套文件");
    }
    fs::write(root.join("opened.md"), "当前文档").expect("写入当前文档");
    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("扫描根目录");
    assert_eq!(tree.children.len(), 33, "根目录的条目应完整显示");
    for directory in tree.children.iter().filter(|node| node.kind_dir()) {
        assert!(!directory.children_loaded, "未展开目录不能预读：{}", directory.label);
        assert!(directory.children.is_empty(), "首次加载不能扫描子目录内容");
    }
    fs::remove_dir_all(root).expect("清理夹具");
}

#[gpui::test]
async fn workspace_tree_opening_one_file_does_not_start_a_global_walk(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-implicit-root-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("nested/deeper")).expect("创建目录");
    let root = crate::file_url::canonical_path(&root);
    let path = root.join("opened.md");
    fs::write(&path, "当前文档").expect("写入当前文档");
    fs::write(root.join("nested/deeper/other.md"), "深层文档").expect("写入嵌套文件");
    let (editor, cx) = cx.add_window_view(|_, cx| {
        Editor::from_file_source(cx, "当前文档".into(), Some(path))
    });
    editor.update(cx, |editor, cx| {
        editor.workspace.is_open = true;
        editor.workspace.active_tab = super::super::WorkspaceTab::Files;
        editor.sync_workspace_models(cx);
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.file_tree.is_some(), "文件树应完成首层加载");
        assert!(editor.workspace.files_on_disk_root.is_none(), "打开文件不能递归建立全局名单");
        assert!(editor.workspace.files_on_disk_walk_root.is_none(), "不应启动后台全局遍历");
        assert!(editor.workspace.files_on_disk.is_empty(), "不能为当前文件预读其它目录");
    });
    editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.files_on_disk_root.is_none(), "刷新文件树也不能触发全局遍历");
    });
    editor.update(cx, |editor, cx| {
        editor.workspace.search_query = "当前".into();
        editor.schedule_workspace_search(cx);
    });
    cx.executor().advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.files_on_disk_root.is_none(), "当前文档搜索不需要全局名单");
    });
    editor.update(cx, |editor, cx| {
        editor.workspace.search_scope = super::super::WorkspaceSearchScope::Workspace;
        editor.workspace.search_query = "深层文档".into();
        editor.schedule_workspace_search(cx);
    });
    cx.run_until_parked();
    cx.executor().advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    let deep_path = crate::file_url::canonical_path(&root.join("nested/deeper/other.md"));
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.search_results.iter().any(|hit| hit.path == deep_path),
            "首次全局搜索应等待名单，并搜到未展开目录里的文件");
        assert!(!editor.workspace.search_pending, "全局搜索应完成");
    });
    fs::remove_dir_all(root).expect("清理夹具");
}

#[test]
fn workspace_tree_lists_build_directories_without_scanning_their_contents() {
    let root = std::env::temp_dir().join(format!("velora-build-dirs-{}", uuid::Uuid::new_v4()));
    for parent in [&root, &root.join("nested")] {
        for name in ["dist", "target"] {
            let directory = parent.join(name);
            fs::create_dir_all(directory.join("debug")).expect("创建构建目录");
            fs::write(directory.join("artifact.md"), "构建说明").expect("写入构建文件");
            fs::write(directory.join("debug/output.log"), "构建日志").expect("写入嵌套文件");
        }
    }
    fs::write(root.join("note.md"), "正文").expect("写入普通文件");
    for name in [".git", "node_modules", ".worktrees"] {
        fs::create_dir_all(root.join(name)).expect("创建隐藏目录");
        fs::write(root.join(name).join("internal.md"), "内部文件").expect("写入内部文件");
    }

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("扫描文件树");
    assert_eq!(
        collect_workspace_files_on_disk(&root), vec![root.join("note.md")],
        "全局搜索仍应跳过构建目录和内部目录"
    );
    assert_eq!(
        tree.children.iter().map(|node| node.label.as_str()).collect::<Vec<_>>(),
        vec!["dist", "nested", "target", "note.md"],
        "构建目录应显示，内部目录保持隐藏"
    );
    for parent in [&root, &root.join("nested")] {
        let parent_tree = scan_workspace_dir_level(parent, TreeSortPreference::Name)
            .expect("展开父目录");
        for name in ["dist", "target"] {
            let directory = parent.join(name);
            let node = find_workspace_node(std::slice::from_ref(&parent_tree), &file_node_id(&directory))
                .expect("文件树中的构建目录");
            assert!(!node.children_loaded, "构建目录应等展开后再加载");
            assert!(node.children.is_empty(), "未展开时不应扫描构建目录的内容");

            let expanded = scan_workspace_dir_level(&directory, TreeSortPreference::Name)
                .expect("展开构建目录");
            assert_eq!(
                expanded.children.iter().map(|child| child.label.as_str()).collect::<Vec<_>>(),
                vec!["debug", "artifact.md"]
            );
            let debug = expanded.children.first().expect("下一层目录");
            assert!(!debug.children_loaded, "展开时只加载一层");
        }
    }
    fs::remove_dir_all(root).expect("清理夹具");
}

#[test]
fn workspace_tree_includes_plain_viewer_code_extensions() {
    let root = std::env::temp_dir().join(format!(
        "velora-workspace-plain-code-test-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).expect("create root");
    for (name, source) in [
        ("query.sql", "select 1;"),
        ("App.swift", "struct App {}"),
        ("Main.kt", "fun main() {}"),
        ("layout.xml", "<root />"),
    ] {
        fs::write(root.join(name), source).expect("write code sample");
    }

    let tree = scan_workspace_dir_recursive(&root, TreeSortPreference::Name).expect("scan code workspace");
    assert_eq!(tree.children.len(), 4);
    assert!(
        tree.children
            .iter()
            .all(|node| { matches!(node.kind, WorkspaceTreeKind::CodeFile(_)) })
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn code_file_extensions_are_case_insensitive() {
    for extension in ["rs", "sql", "swift", "kt", "xml"] {
        assert!(is_code_file(Path::new(&format!("source.{extension}"))));
        assert!(is_code_file(Path::new(&format!(
            "source.{}",
            extension.to_ascii_uppercase()
        ))));
    }
    assert!(is_code_file(Path::new("notes.txt")));
}

#[gpui::test]
async fn opening_code_files_keeps_them_in_the_editor(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root =
        std::env::temp_dir().join(format!("velora-code-viewer-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create test workspace");
    let path = root.join("main.rs");
    fs::write(&path, "fn main() { println!(\"hello\"); }").expect("write code file");
    let plain_path = root.join("query.sql");
    fs::write(&plain_path, "select 1;").expect("write plain code file");
    let crlf_path = root.join("windows.cs");
    fs::write(&crlf_path, "class A {\r\n}\r\n").expect("write CRLF code file");
    let cleanup_root = root.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(cleanup_root);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert!(editor.code_tab_active());
        assert_eq!(
            editor.document.raw_source_text(cx),
            "fn main() { println!(\"hello\"); }"
        );
        let block = editor.document.first_root().unwrap().read(cx);
        assert!(block.kind().is_code_block());
        assert!(block.code_highlight_result().is_some());
        assert_eq!(editor.workspace.open_documents.len(), 1);
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(plain_path.clone(), window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.workspace.open_documents.len(), 2);
        assert_eq!(editor.workspace.active_document.as_ref(), Some(&plain_path));
        assert_eq!(editor.document.raw_source_text(cx), "select 1;");
    });
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.selected_range = 0..block.visible_len();
            <Block as EntityInputHandler>::replace_text_in_range(
                block,
                None,
                "select 2;",
                window,
                cx,
            );
        });
    });
    cx.run_until_parked();
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.raw_source_text(cx), "select 2;");
    });
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    editor.read_with(cx, |editor, cx| {
        assert!(editor.code_tab_active());
        assert_eq!(editor.document.raw_source_text(cx), "select 1;");
        assert!(
            editor
                .document
                .first_root()
                .unwrap()
                .read(cx)
                .kind()
                .is_code_block()
        );
    });
    editor.update(cx, |editor, cx| editor.redo_document(cx));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.save_document(window, cx));
    });
    assert_eq!(fs::read_to_string(&plain_path).unwrap(), "select 2;");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        });
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.workspace.open_documents.len(), 2);
        assert_eq!(
            editor.document.raw_source_text(cx),
            "fn main() { println!(\"hello\"); }"
        );
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(crlf_path.clone(), window, cx)
        });
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(editor.document.raw_source_text(cx), "class A {\n}\n");
    });
    editor.update(cx, |editor, cx| {
        let source = editor.current_document_source(cx);
        let offset = source
            .split_inclusive('\n')
            .take(1)
            .map(str::len)
            .sum::<usize>()
            .min(source.len());
        editor.jump_to_document_search_range(offset..offset, cx);
    });
    editor.read_with(cx, |editor, cx| {
        assert_eq!(
            editor
                .document
                .first_root()
                .unwrap()
                .read(cx)
                .selected_range
                .start,
            "class A {\n".len()
        );
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.save_document(window, cx));
    });
    assert_eq!(
        fs::read_to_string(&crlf_path).unwrap(),
        "class A {\r\n}\r\n"
    );
}

#[gpui::test]
async fn right_click_menu_renders_for_a_workspace_file(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-menu-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("note.md");
    fs::write(&path, "hello").unwrap();
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
        editor.open_workspace_context_menu(
            point(px(100.0), px(100.0)),
            Some(WorkspaceSelection::File(path)),
            cx,
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.context_menu.as_ref().expect("右键菜单").has_target);
    });
}

#[test]
fn workspace_create_operations_do_not_overwrite_existing_files() {
    let root =
        std::env::temp_dir().join(format!("velora-workspace-create-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create root");
    let file = root.join("notes.md");
    fs::write(&file, "keep this").expect("write existing file");

    assert!(create_workspace_file(&file).is_err());
    assert_eq!(
        fs::read_to_string(&file).expect("read existing file"),
        "keep this"
    );

    let new_file = root.join("new.md");
    create_workspace_file(&new_file).expect("create markdown file");
    assert_eq!(fs::read_to_string(&new_file).expect("read new file"), "");

    let folder = root.join("nested");
    create_workspace_folder(&folder).expect("create folder");
    assert!(folder.is_dir());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn moving_a_folder_remaps_open_document_descendants() {
    let source = Path::new("/workspace/old");
    let destination = Path::new("/workspace/new");
    assert_eq!(
        remap_moved_path(
            Path::new("/workspace/old/docs/readme.md"),
            source,
            destination,
            true,
        ),
        Some(PathBuf::from("/workspace/new/docs/readme.md"))
    );
    assert_eq!(
        remap_moved_path(
            Path::new("/workspace/other/readme.md"),
            source,
            destination,
            true,
        ),
        None
    );
    // 被移动项自身必须**原样**映射到 destination：`Path` 的 `==` 忽略尾
    // 分隔符，`Some(…/new.md/)` 也能通过，所以这里按字符串对账
    // （尾斜杠会让之后的读盘直接 ENOTDIR）。
    assert_eq!(
        remap_moved_path(
            Path::new("/workspace/old.md"),
            Path::new("/workspace/old.md"),
            Path::new("/workspace/new.md"),
            false,
        )
        .map(|path| path.to_string_lossy().into_owned()),
        Some("/workspace/new.md".to_string())
    );
}

#[test]
fn deleting_a_folder_matches_only_its_descendants() {
    let folder = Path::new("/workspace/docs");
    assert!(path_is_affected(
        Path::new("/workspace/docs/readme.md"),
        folder,
        true,
    ));
    assert!(!path_is_affected(
        Path::new("/workspace/docs-old/readme.md"),
        folder,
        true,
    ));
    assert!(path_is_affected(
        Path::new("/workspace/readme.md"),
        Path::new("/workspace/readme.md"),
        false,
    ));
}

#[test]
fn outline_tree_skips_headings_inside_fenced_code() {
    let outline = build_outline_tree(
        "# Root\n\n```md\n# ignored\n```\n\n## Child\n\n### Grandchild\n\n# Next",
    );

    assert_eq!(outline.len(), 2);
    assert_eq!(outline[0].label, "Root");
    assert_eq!(outline[0].children[0].label, "Child");
    assert_eq!(outline[0].children[0].children[0].label, "Grandchild");
    assert_eq!(outline[1].label, "Next");
}

#[gpui::test]
async fn outline_tracks_committed_heading_edits(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, "# Old".into(), None));
    editor.update(cx, |editor, cx| {
        editor.sync_workspace_outline(cx);
        assert_eq!(editor.workspace.outline_tree[0].label, "Old");
        let heading = editor.document.first_root().unwrap().clone();
        heading.update(cx, |heading, cx| {
            heading.prepare_undo_capture(UndoCaptureKind::CoalescibleText, cx);
            heading.replace_text_in_visible_range(0..3, "New", None, false, cx);
        });
    });
    cx.run_until_parked();
    editor.update(cx, |editor, cx| {
        editor.sync_workspace_outline(cx);
        assert_eq!(editor.workspace.outline_tree[0].label, "New");
    });
}

#[test]
fn outline_expansion_state_is_not_auto_populated_and_prunes_stale_ids() {
    let outline = build_outline_tree("# Root\n\n## Child\n\n# Next");
    let mut fresh = WorkspaceState::default();
    prune_outline_state(&mut fresh, &outline);
    assert!(fresh.expanded.is_empty());

    let mut existing = WorkspaceState::default();
    existing.expanded.insert("outline:0".to_string());
    existing.expanded.insert("outline:999".to_string());
    existing
        .expanded
        .insert("workspace-dir:C:/docs".to_string());
    existing.selected = Some(WorkspaceSelection::Outline("outline:999".to_string()));

    prune_outline_state(&mut existing, &outline);

    assert!(existing.expanded.contains("outline:0"));
    assert!(existing.expanded.contains("workspace-dir:C:/docs"));
    assert!(!existing.expanded.contains("outline:999"));
    assert_eq!(existing.selected, None);
}

#[test]
fn workspace_panel_width_stays_within_drag_bounds() {
    assert_eq!(clamp_workspace_panel_width(100.0, 1080.0), 180.0);
    assert_eq!(clamp_workspace_panel_width(320.0, 1080.0), 320.0);
    assert_eq!(clamp_workspace_panel_width(500.0, 720.0), 400.0);
}

/// 代码文件也归缓冲区说了算：改一个字符，保存出去应该还是「原文 + 那一处改动」。
///
/// 这条管的是文件形状（CRLF、连续空行、行首制表符、末行没有换行）。代码文档
/// 以前不走缓冲区，而是每次保存从块树把源码拼回去，形状靠专门的分支硬撑。
#[gpui::test]
async fn saving_an_edited_code_file_keeps_the_file_shape(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root =
        std::env::temp_dir().join(format!("velora-code-shape-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create test workspace");
    let path = root.join("shape.rs");
    // CRLF + 连续空行 + 制表符缩进 + 末行没有换行：每一样都是重新拼接时容易丢的。
    let original = "first\r\n\r\n\r\n\tindented\r\nlast";
    fs::write(&path, original).expect("write code file");
    let cleanup_root = root.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(cleanup_root);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();

    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.selected_range = 0..0;
            <crate::components::Block as EntityInputHandler>::replace_text_in_range(
                block, None, "X", window, cx,
            );
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.save_document(window, cx))
    });
    cx.run_until_parked();

    let saved = fs::read_to_string(&path).expect("read saved file");
    assert_eq!(saved, format!("X{original}"), "改一个字符之后代码文件的形状被重排了");
}

/// 把大纲树摊平成可比较的形状：(整篇行号, 层级, 标题文字, 缩进深度)。
fn outline_shape(nodes: &[WorkspaceTreeNode]) -> Vec<(usize, u8, String, usize)> {
    fn visit(
        nodes: &[WorkspaceTreeNode],
        depth: usize,
        out: &mut Vec<(usize, u8, String, usize)>,
    ) {
        for node in nodes {
            if let WorkspaceTreeKind::Heading { line, level } = node.kind {
                out.push((line, level, node.label.clone(), depth));
            }
            visit(&node.children, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    visit(nodes, 0, &mut out);
    out
}

/// 大纲现在是「每根块一份行摘要」拼出来的，这条路必须与「把整篇按行扫一遍」逐字节
/// 等价：围栏里的 `#`、Setext 的上一行、缩进过的 `#`、四空格缩进的代码、front
/// matter、没闭合的围栏跨过块边界……每一种形状都在打字、按回车、删字之后各比一次。
#[gpui::test]
async fn the_incremental_outline_matches_a_whole_document_scan(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    const SHAPES: &[(&str, &str)] = &[
        ("层级混排", "# 一\n\n## 二\n\n### 三\n\n## 四\n\n# 五\n"),
        (
            "围栏里的井号",
            "# 外面\n\n```text\n# 里面不是标题\n```\n\n# 外面乙\n",
        ),
        (
            "没闭合的围栏",
            "# 前面\n\n```\n# 掉进围栏里\n\n# 也还在围栏里\n",
        ),
        (
            "Setext 标题",
            "一级标题\n========\n\n正文\n\n二级标题\n--------\n\n# ATX\n",
        ),
        (
            "缩进过的井号",
            "  # 两格算标题\n\n    # 四格是代码\n\n# 顶格\n",
        ),
        (
            "front matter",
            "---\ntitle: 笔记\n# 这不是标题\n---\n\n# 这才是\n",
        ),
        (
            "引用与列表里",
            "# 顶\n\n> 引用一\n> # 引用里的井号\n\n- 项\n  # 列表续行\n",
        ),
        (
            "表格里的井号",
            "# 顶\n\n| 名称 | 说明 |\n| --- | --- |\n| a | # 不算 |\n\n# 尾\n",
        ),
        ("划线单独成段", "正文\n\n===\n\n# 标题\n"),
    ];

    for (name, document) in SHAPES {
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, (*document).to_string(), None));
        cx.run_until_parked();

        let check = |tag: &str,
                     editor: &gpui::Entity<Editor>,
                     cx: &mut gpui::VisualTestContext,
                     rescans: &mut u64| {
            editor.update(cx, |editor, cx| {
                editor.sync_workspace_outline(cx);
                // 比对的必须是「按块拼」这条路算出来的大纲：一旦某一步退回整篇重扫，
                // 它照样与整篇扫的一致，这个测试就成了自证。所以退回次数也钉在这里。
                let full = editor.outline_full_rescans.get();
                assert_eq!(
                    full, *rescans,
                    "「{name}」{tag}之后大纲退回了整篇重扫（累计 {} 次）：按块增量这条路没走通",
                    full - *rescans,
                );
                *rescans = full;
                let by_segments = outline_shape(&editor.workspace.outline_tree);
                let whole_source = editor.buffer.text();
                let whole = build_outline_tree(&whole_source);
                assert_eq!(
                    by_segments,
                    outline_shape(&whole),
                    "「{name}」{tag}之后，按块拼的大纲与整篇扫的不一样：{by_segments:?} vs {:?}\n文本 {whole_source:?}",
                    outline_shape(&whole),
                );
            });
        };
        let mut rescans = editor.read_with(cx, |editor, _| editor.outline_full_rescans.get());
        check("打开", &editor, cx, &mut rescans);

        // 一块一块地落笔：在每根块的块首打一个字、再按一次回车，每步都比对一次。
        let root_count = editor.read_with(cx, |editor, _| editor.document.root_count());
        let mut typed = true;
        for index in 0..root_count {
            for _ in 0..2 {
                let Some(root) = editor.update(cx, |editor, _cx| {
                    let root = editor.document.root_blocks().get(index)?.clone();
                    editor.focus_block(root.entity_id());
                    Some(root)
                }) else {
                    break;
                };
                let tag = if typed { "块首打字" } else { "块首回车" };
                cx.update(|window, cx| {
                    root.update(cx, |block, cx| {
                        block.selected_range = 0..0;
                        if typed {
                            <crate::components::Block as EntityInputHandler>::replace_text_in_range(
                                block, None, "甲", window, cx,
                            );
                        } else {
                            block.on_newline(&crate::components::Newline, window, cx);
                        }
                    });
                });
                typed = !typed;
                cx.run_until_parked();
                check(&format!("第 {index} 根{tag}"), &editor, cx, &mut rescans);
            }
        }
    }
}

/// 大纲只为「在看它的人」算：侧栏收起、正文里又没有 `[TOC]` 块时，打一个字都不该
/// 重算一遍（旧实现每帧拿整篇文本比一次，再把整篇按行重扫）；正文里有 `[TOC]` 块时，
/// 收起侧栏也要算——那是它的读者。
#[gpui::test]
async fn the_outline_is_only_built_for_a_reader(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        Editor::from_markdown(cx, "# 一\n\n正文段落。\n".to_string(), None)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();

    let before = editor.read_with(cx, |editor, _| editor.outline_rebuilds.get());
    let root = editor.update(cx, |editor, _cx| {
        let root = editor.document.root_blocks()[1].clone();
        editor.focus_block(root.entity_id());
        root
    });
    cx.update(|window, cx| {
        root.update(cx, |block, cx| {
            block.selected_range = 0..0;
            <crate::components::Block as EntityInputHandler>::replace_text_in_range(
                block, None, "甲", window, cx,
            );
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.outline_rebuilds.get(),
            before,
            "侧栏收起又没有 `[TOC]` 块，打一个字却重算了一遍大纲"
        );
    });

    // 正文里出现 `[TOC]`：它自己是读者，收起侧栏也得把清单算出来。
    editor.update(cx, |editor, cx| {
        editor.replace_document_from_markdown("[TOC]\n\n# 一\n\n正文段落。\n".to_string(), None, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor
                .workspace
                .toc_entries
                .iter()
                .map(|entry| entry.title.clone())
                .collect::<Vec<_>>(),
            vec!["一".to_string()],
            "有 `[TOC]` 块却没算大纲"
        );
    });
}

/// 源码/代码文档按行切片（`SOURCE_DOCUMENT_CHUNK_LINES` 行一片），``` 围栏会跨过片与片
/// 的接缝——那一片的「进入时的围栏状态」不再是空的。摘要按这个状态分档之后，第二片照样
/// 只扫自己那几行；在那之前，跨片围栏让大纲整条增量路失效，只能退回整篇重扫（10 MiB
/// 代码文档实测一键 585499 行 / 140ms）。
#[gpui::test]
async fn a_fence_crossing_a_source_chunk_seam_still_scans_per_block(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let chunk = crate::editor::file_drop::SOURCE_DOCUMENT_CHUNK_LINES;
    let mut lines = vec!["fn main() {}".to_string()];
    while lines.len() < chunk - 1 {
        lines.push(format!("正文第 {} 行，不是标题。", lines.len()));
    }
    // 这一片的最后一行开围栏，闭合行与后面的标题都落在下一片里。
    lines.push("```rust".to_string());
    lines.push("# 掉在围栏里，不算标题".to_string());
    lines.push("```".to_string());
    lines.push("# 围栏外才算标题".to_string());
    let source = format!("{}\n", lines.join("\n"));
    // 按行切片是**代码/纯文本文件**那条路（`build_source_document_roots`），
    // 所以要真走文件加载，`from_markdown` 的源码退回是整篇一块。
    let dir = std::env::temp_dir().join(format!(
        "velora-outline-seam-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("seam.py");
    fs::write(&path, &source).expect("write fixture");
    let cleanup = dir.clone();
    cx.on_quit(move || {
        if let Err(error) = fs::remove_dir_all(&cleanup) {
            eprintln!("夹具目录清理失败 {}: {error}", cleanup.display());
        }
    });
    let document = crate::editor::encoding::load_document(&path).expect("load fixture");
    let (editor, cx) = cx.add_window_view(move |_window, cx| {
        Editor::from_loaded_document(cx, document, Some(path.clone()))
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();

    editor.read_with(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        assert!(
            roots.len() >= 2,
            "这份文档该被切成至少两片，接缝才能压在围栏上：{} 片",
            roots.len()
        );
        assert!(
            roots[0].read(cx).display_text().ends_with("```rust"),
            "接缝没压在围栏上，这个测试就没测到东西"
        );
    });

    // 侧栏收起又没 `[TOC]` 块时大纲根本不算（见 `the_outline_is_only_built_for_a_reader`），
    // 这里按「有人看」的方式显式要一次。
    let before = editor.read_with(cx, |editor, _| editor.outline_full_rescans.get());
    editor.update(cx, |editor, cx| editor.sync_workspace_outline(cx));
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.outline_full_rescans.get(),
            before,
            "打开这份文档就把大纲整篇重扫了：跨片的围栏让按块增量失效"
        );
        assert_eq!(
            editor
                .workspace
                .toc_entries
                .iter()
                .map(|entry| entry.title.clone())
                .collect::<Vec<_>>(),
            vec!["围栏外才算标题".to_string()],
            "跨片的围栏没被认出来：围栏里的 # 也算了标题"
        );
    });

    // 打一个字：只该重扫改动那一片，别退回整篇。
    let root = editor.update(cx, |editor, _cx| {
        let root = editor.document.root_blocks()[1].clone();
        editor.focus_block(root.entity_id());
        root
    });
    cx.update(|window, cx| {
        root.update(cx, |block, cx| {
            block.selected_range = 0..0;
            <crate::components::Block as EntityInputHandler>::replace_text_in_range(
                block, None, "甲", window, cx,
            );
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let before_typing = editor.read_with(cx, |editor, _| editor.outline_full_rescans.get());
    editor.update(cx, |editor, cx| editor.sync_workspace_outline(cx));
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            editor.outline_full_rescans.get(),
            before_typing,
            "打一个字让大纲退回整篇重扫"
        );
        assert_eq!(
            editor
                .workspace
                .toc_entries
                .iter()
                .map(|entry| entry.title.clone())
                .collect::<Vec<_>>(),
            vec!["围栏外才算标题".to_string()],
            "打完字这份大纲就不是原来的了"
        );
    });
}

/// 围栏的状态要能**跨过没动过的块**传下去：开栏长度多一个反引号，后面那些一个字
/// 都没改的片就从「围栏外」变成「围栏里」，它们那份摘要不再算数——缓存键带着
/// 「走进这块时的围栏状态」，所以这里是换键重扫，而不是整篇重扫，也不是留着旧摘要。
#[gpui::test]
async fn a_longer_fence_above_pulls_the_following_chunks_inside_it(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let chunk = crate::editor::file_drop::SOURCE_DOCUMENT_CHUNK_LINES;
    // 五片：0 全正文，1 的第一行开围栏，2 的第一行是井号（在栏里，不算标题），
    // 3 的第一行闭栏、第二行是井号（算），4 的第一行也是井号（算）。
    let mut lines = vec!["value = 0".to_string(); chunk * 5];
    lines[chunk] = "```rust".to_string();
    lines[chunk * 2] = "# 掉在围栏里，不算标题".to_string();
    lines[chunk * 3] = "```".to_string();
    lines[chunk * 3 + 1] = "# 围栏外才算标题".to_string();
    lines[chunk * 4] = "# 也算标题".to_string();
    let source = format!("{}\n", lines.join("\n"));
    let dir = std::env::temp_dir().join(format!(
        "velora-outline-fence-width-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("fence-width.py");
    fs::write(&path, &source).expect("write fixture");
    let cleanup = dir.clone();
    cx.on_quit(move || {
        if let Err(error) = fs::remove_dir_all(&cleanup) {
            eprintln!("夹具目录清理失败 {}: {error}", cleanup.display());
        }
    });
    let document = crate::editor::encoding::load_document(&path).expect("load fixture");
    let (editor, cx) =
        cx.add_window_view(move |_window, cx| Editor::from_loaded_document(cx, document, Some(path.clone())));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();

    editor.read_with(cx, |editor, cx| {
        let roots = editor.document.root_blocks();
        assert!(
            roots.len() >= 5,
            "这份文档该切成五片才测得到跨片传递：{} 片",
            roots.len()
        );
        assert!(
            roots[1].read(cx).display_text().starts_with("```rust"),
            "开栏没落在第二片的第一行，这个测试就没测到东西"
        );
    });

    let titles = |editor: &Editor| -> Vec<String> {
        editor
            .workspace
            .toc_entries
            .iter()
            .map(|entry| entry.title.clone())
            .collect()
    };
    let sync = |editor: &gpui::Entity<Editor>, cx: &mut gpui::VisualTestContext, tag: &str| {
        let before = editor.read_with(cx, |editor, _| editor.outline_full_rescans.get());
        editor.update(cx, |editor, cx| editor.sync_workspace_outline(cx));
        editor.read_with(cx, |editor, _cx| {
            assert_eq!(
                editor.outline_full_rescans.get(),
                before,
                "{tag}：大纲退回了整篇重扫，跨片的围栏状态没接住"
            );
        });
    };

    sync(&editor, cx, "打开");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            titles(editor),
            vec!["围栏外才算标题".to_string(), "也算标题".to_string()],
        );
    });

    // 在开栏那片补一个反引号：四格的栏，三格的那一行关不掉它。
    let root = editor.update(cx, |editor, _cx| {
        let root = editor.document.root_blocks()[1].clone();
        editor.focus_block(root.entity_id());
        root
    });
    cx.update(|window, cx| {
        root.update(cx, |block, cx| {
            block.selected_range = 0..0;
            <crate::components::Block as EntityInputHandler>::replace_text_in_range(
                block, None, "`", window, cx,
            );
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    sync(&editor, cx, "开栏长一格");
    editor.read_with(cx, |editor, _cx| {
        assert!(
            titles(editor).is_empty(),
            "四格的开栏后面三片还在栏里，大纲却认出了标题：{:?}",
            titles(editor)
        );
    });

    // 撤掉那一格：后面几片又回到栏外，摘要得重新算出来（换过键的旧那档不该留着）。
    editor.update(cx, |editor, cx| editor.undo_document(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    sync(&editor, cx, "撤销那一格");
    editor.read_with(cx, |editor, _cx| {
        assert_eq!(
            titles(editor),
            vec!["围栏外才算标题".to_string(), "也算标题".to_string()],
            "撤销之后大纲没有回到栏外那份：{:?}",
            titles(editor)
        );
    });
}
/// CRLF 代码文件里粘贴带 `\r\n` 的剪贴板文本，磁盘上绝不允许出现 `\r\r\n`。
///
/// 缓冲区是 LF 规范空间：剪贴板的 CRLF 原样落进去，保存时 FileShape 再升格一次
/// 就是 `\r\r\n`——字节损坏（重开多出空行），而且版本号会把自己写的文件误判成
/// 外部修改，整个文件被锁死到重载为止。粘贴入口归一 + encode 兜底，两道闸。
#[gpui::test]
async fn pasting_crlf_clipboard_text_into_a_crlf_code_file_never_writes_crcrlf(
    cx: &mut TestAppContext,
) {
    use crate::components::Paste;

    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-crlf-paste-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create test workspace");
    let path = root.join("paste.rs");
    let original = "first\r\nlast\r\n";
    fs::write(&path, original).expect("write code file");
    let cleanup_root = root.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(cleanup_root);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        });
    });
    cx.update(|_window, cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("x\r\ny".into()));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();

    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    cx.update(|_window, cx| {
        block.update(cx, |block, _cx| {
            let len = block.visible_len();
            block.selected_range = len..len;
        });
    });
    cx.update(|window, cx| {
        block.update(cx, |block, cx| block.on_paste(&Paste, window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.save_document(window, cx))
    });
    cx.run_until_parked();

    let saved = fs::read(&path).expect("read saved file");
    // A2 的契约：绝不出现 `\r\r\n`（粘进来的 CRLF 先折平、保存时只升格一次）。
    // 末行的尾换行由代码文档自己的重同步约定决定（总是补齐），不在本条管。
    assert!(
        !saved.windows(3).any(|w| w == b"\r\r\n"),
        "落盘字节出现 \\r\\r\\n（CRLF 被升格了两次）：{:?}",
        String::from_utf8_lossy(&saved)
    );
    assert!(
        saved.windows(4).any(|w| w == b"x\r\ny"),
        "粘贴的内容没按 CRLF 形状落盘：{:?}",
        String::from_utf8_lossy(&saved)
    );

    // 第二次保存不得把自己写的文件误判成外部修改：保存要成功，字节要稳定。
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.save_document(window, cx))
    });
    cx.run_until_parked();
    let saved_again = fs::read(&path).expect("read saved file");
    assert_eq!(saved_again, saved, "第二次保存改写了字节（版本号自误判）");
}

/// 脏文档被 autosave 时，活动文档落盘的是**缓冲区字节**：CRLF 文件不能被洗成
/// LF（那是「打开没动的字节被改写」的旁门版本）。手动保存已有此保证，这里钉
/// autosave 这条旁路。
#[gpui::test]
async fn autosaving_a_dirty_crlf_document_keeps_its_line_endings(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-crlf-autosave-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create test workspace");
    let path = root.join("autosave.rs");
    fs::write(&path, "first\r\nlast\r\n").expect("write code file");
    let cleanup_root = root.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(cleanup_root);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();

    // 改一个字（文档变脏），等 autosave 防抖到期。
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.selected_range = 0..0;
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "X", window, cx,
            );
        });
    });
    cx.run_until_parked();
    cx.executor().advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();

    let saved = fs::read(&path).expect("read autosaved file");
    assert!(
        !saved.windows(3).any(|window| window == b"\r\r\n"),
        "autosave 落盘出现 \\r\\r\\n：{:?}",
        String::from_utf8_lossy(&saved)
    );
    assert!(
        saved.starts_with(b"Xfirst\r\nlast\r\n"),
        "autosave 把 CRLF 文件洗成了别的形状：{:?}",
        String::from_utf8_lossy(&saved)
    );
}

/// 外部改动触发的重载也必须接上原始字节与文件形状：重载换掉了整个缓冲区，
/// 不接上的话重载后的第一次保存就把 CRLF 全文件洗成 LF。
#[gpui::test]
async fn reloading_an_externally_changed_file_keeps_its_shape(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-crlf-reload-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("create test workspace");
    let path = root.join("reload.rs");
    fs::write(&path, "first\r\nlast\r\n").expect("write code file");
    let cleanup_root = root.clone();
    cx.on_quit(move || {
        let _ = fs::remove_dir_all(cleanup_root);
    });

    let (editor, cx) =
        cx.add_window_view(|_window, cx| Editor::from_markdown(cx, String::new(), None));
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.open_workspace_file(path.clone(), window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();

    // 外部把文件改成别的 CRLF 内容，然后走重载。
    fs::write(&path, "changed\r\nexternally\r\n").expect("external write");
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.reload_externally_changed_document(&path, cx)
        });
    });
    cx.run_until_parked();

    // 重载后的文档是外部新内容；再编辑一个字并保存，CRLF 必须原样保留。
    let block = editor.read_with(cx, |editor, _| {
        editor.document.first_root().unwrap().clone()
    });
    cx.update(|window, cx| {
        block.update(cx, |block, cx| {
            block.selected_range = 0..0;
            <Block as EntityInputHandler>::replace_text_in_range(
                block, None, "X", window, cx,
            );
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.save_document(window, cx))
    });
    cx.run_until_parked();

    let saved = fs::read(&path).expect("read saved file");
    assert_eq!(
        saved,
        b"Xchanged\r\nexternally\r\n".to_vec(),
        "重载后的保存把文件形状洗掉了：{:?}",
        String::from_utf8_lossy(&saved)
    );
}

/// 文件树与标签共用 Kit 标准菜单行，不能再分别手写一套行高与内边距。
#[gpui::test]
async fn every_context_menu_row_uses_the_same_geometry(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-menu-geometry-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("note.md");
    fs::write(&path, "hello").unwrap();
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
        });
    });
    cx.run_until_parked();

    editor.update(cx, |editor, cx| {
        editor.open_workspace_context_menu(
            point(px(60.0), px(120.0)),
            Some(WorkspaceSelection::File(path.clone())),
            cx,
        )
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let tree_row = workspace_menu_action_bounds(cx, WorkspaceMenuAction::Reveal)
        .expect("文件树菜单的第一行");

    editor.update(cx, |editor, cx| {
        editor.open_tab_context_menu(point(px(80.0), px(40.0)), 0, cx)
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let close_label = cx.cx.read(|cx| cx.global::<crate::i18n::I18nManager>().strings().tab_close.clone());
    let tab_row = menu_label_bounds(cx, &close_label).expect("标签菜单的第一行");

    assert!(tree_row.size.height > px(0.0), "标准菜单行必须有实际布局");
    assert_eq!(
        tab_row.size.height, tree_row.size.height,
        "两个右键菜单的行高必须一致（同一份行渲染）"
    );
}

/// 菜单分节线的画法收在 `src/components/menu.rs` 一处。`src/editor` 下再出现
/// `menu_separator_margin_x` 就说明有人又手抄了一份分隔线。
#[test]
fn menu_separator_is_rendered_from_one_place() {
    let mut offenders = Vec::new();
    collect_menu_separator_offenders(std::path::Path::new("src/editor"), &mut offenders);
    assert!(
        offenders.is_empty(),
        "菜单分隔线只应由 components::menu::menu_separator 画，手抄在：{offenders:?}"
    );
}

fn collect_menu_separator_offenders(dir: &std::path::Path, offenders: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_menu_separator_offenders(&path, offenders);
            continue;
        }
        if path.components().any(|part| part.as_os_str() == "tests") {
            // 测试源码里会出现这个令牌名（本文件的守卫就是），不参与扫描。
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if text.contains("menu_separator_margin_x") {
            offenders.push(path.display().to_string());
        }
    }
}

#[gpui::test]
async fn dirs_below_the_default_depth_load_when_expanded(cx: &mut TestAppContext) {
    // 换根默认只扫一层（WORKSPACE_SCAN_DEPTH）：更深的目录先留占位（`children_loaded`
    // 为 false），展开时才扫它下一层。这是「打开文件不再把整个目录树走穿」的守卫——
    // 谁把扫描改回递归，这条用例就会红（深层文件在展开前就出现在树里）。
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-lazy-depth-{}", uuid::Uuid::new_v4()));
    let deep_dir = root.join("a").join("b").join("c").join("d");
    fs::create_dir_all(&deep_dir).expect("create deep dirs");
    fs::write(deep_dir.join("note.md"), "# note\n").expect("write deep note");
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
        editor.ensure_workspace_files_walk(cx);
    });
    cx.run_until_parked();

    // 根会被 canonicalize（macOS 的 /var 是 /private/var 的软链），节点 id 按
    // 规范路径算。
    let dir_path = crate::file_url::canonical_path(&root.join("a"));
    let dir_id = file_node_id(&dir_path);
    editor.read_with(cx, |editor, _| {
        let tree = editor.workspace.file_tree.as_ref().expect("换根后应有树");
        let dir = find_workspace_node(std::slice::from_ref(tree), &dir_id)
            .expect("第一层的 a 目录应已列出");
        assert!(
            !dir.children_loaded,
            "一层以下的目录应留占位，等展开再扫：{:?}",
            dir.label
        );
        assert!(dir.children.is_empty(), "占位目录不该带着子项");
        assert!(
            editor
                .workspace
                .files_on_disk
                .iter()
                .any(|path| path.ends_with("note.md")),
            "深层文件仍应出现在走盘名单里（搜索/替换不受树的加载状态影响）"
        );
    });

    // 展开：扫它下一层，b 目录出现（且它自己仍是占位）。
    editor.update(cx, |editor, cx| editor.toggle_workspace_node(&dir_id, cx));
    cx.run_until_parked();
    editor.read_with(cx, |editor, _| {
        let tree = editor.workspace.file_tree.as_ref().expect("树还在");
        let dir = find_workspace_node(std::slice::from_ref(tree), &dir_id).expect("a 目录");
        assert!(dir.children_loaded, "展开后应标记这一层已扫");
        assert_eq!(
            dir.children
                .iter()
                .map(|node| node.label.as_str())
                .collect::<Vec<_>>(),
            vec!["b"],
            "展开后应出现下一层"
        );
        let next = &dir.children[0];
        assert!(!next.children_loaded, "更深的目录继续留占位");
    });
}

#[gpui::test]
async fn folder_hover_has_no_name_tooltip_and_file_metadata_tooltip_remains(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!("velora-folder-tooltip-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).expect("创建目录");
    let path = root.join("note.md");
    fs::write(&path, "正文").expect("写入文件");
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update(cx, |editor, cx| {
        editor.workspace.root = Some(root.clone());
        editor.workspace.file_tree =
            Some(scan_workspace_dir_recursive(&root, TreeSortPreference::Name).expect("扫描夹具"));
        editor.workspace.expanded.insert(file_node_id(&root));
        cx.notify();
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let selector = |path: &Path| -> &'static str {
        Box::leak(
            format!(
                "workspace-node-{}",
                super::super::stable_node_hash(&file_node_id(path))
            )
            .into_boxed_str(),
        )
    };
    let folder = cx.debug_bounds(selector(&root)).expect("目录行");
    cx.simulate_mouse_move(folder.center(), None, gpui::Modifiers::none());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(600));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("workspace-tooltip").is_none(),
        "文件夹不应弹出只有名称的提示"
    );
    let file = cx.debug_bounds(selector(&path)).expect("文件行");
    cx.simulate_mouse_move(file.center(), None, gpui::Modifiers::none());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(600));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("workspace-tooltip").is_some(),
        "文件的大小、修改时间提示应保留"
    );
    fs::remove_dir_all(&root).expect("清理夹具");
}

/// 按修改时间排序的行为等价守卫（无红点，纯形状优化的钉子）：最新在前、
/// 目录成组在前。比较器里每次比较现开两次 fs::metadata（约 2·n·log n 次
/// 系统调用）已收进 sort_by_cached_key 的「每节点一次」——谁改回按比较
/// 取元数据，排序行为不变但慢回去了，这条只钉行为那一半。
#[test]
fn workspace_scan_sorts_by_modified_time_newest_first() {
    use std::time::UNIX_EPOCH;

    let root =
        std::env::temp_dir().join(format!("velora-workspace-mtime-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("目录乙")).expect("create dirs");
    let stamp = |file: &str, seconds: u64| {
        let path = root.join(file);
        fs::write(&path, file).expect("write file");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open for mtime");
        file.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(seconds))
            .expect("set mtime");
    };
    stamp("老.md", 1_000);
    stamp("中.md", 2_000);
    stamp("新.md", 3_000);

    let tree =
        scan_workspace_dir_recursive(&root, TreeSortPreference::ModifiedTime).expect("scan tree");
    let labels = tree
        .children
        .iter()
        .map(|node| node.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["目录乙", "新.md", "中.md", "老.md"]);
    let _ = fs::remove_dir_all(root);
}

/// 扫失败的目录不许每帧重试。
///
/// 面板的补齐（`load_expanded_workspace_dirs`）挂在每帧的 `sync_workspace_models`
/// 上：失败不留下任何记号时，一个读不了的目录（被删掉、没权限、网络盘掉线）会
/// **每帧**重新排一次 `read_dir`，还跟着一次 `notify`（又催出下一帧）——帧帧不停。
/// 这条钉住「失败留痕：每帧的补齐不再重排队，等用户再展一次或树重扫」。
#[gpui::test]
async fn a_directory_that_failed_to_scan_is_not_retried_every_frame(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let root = std::env::temp_dir().join(format!("velora-scan-failure-{}", uuid::Uuid::new_v4()));
    // 默认只扫一层：a 是占位目录，展开时才扫。
    let placeholder = root.join("a");
    fs::create_dir_all(placeholder.join("d")).expect("create deep dirs");
    fs::write(placeholder.join("d").join("note.md"), "# note\n").expect("write deep note");
    cx.on_quit({
        let root = root.clone();
        move || {
            let _ = fs::remove_dir_all(root);
        }
    });

    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    cx.run_until_parked();

    let dir_path = crate::file_url::canonical_path(&placeholder);
    let dir_id = file_node_id(&dir_path);
    editor.read_with(cx, |editor, _| {
        let tree = editor.workspace.file_tree.as_ref().expect("换根后应有树");
        let node = find_workspace_node(std::slice::from_ref(tree), &dir_id).expect("a 目录");
        assert!(!node.children_loaded, "第一层应是占位目录");
    });

    // 目录从盘上消失：展开时那一次扫描必然失败，而树里那一行还在。
    fs::remove_dir_all(root.join("a")).expect("remove subtree");

    let before = editor.read_with(cx, |editor, _| editor.dir_scan_attempts.get());
    editor.update(cx, |editor, cx| editor.toggle_workspace_node(&dir_id, cx));
    cx.run_until_parked();
    let after_expand = editor.read_with(cx, |editor, _| editor.dir_scan_attempts.get());
    assert!(
        after_expand > before,
        "展开没触发扫描，这条闸门量不到东西"
    );

    // 面板每帧都会跑一次补齐：失败之后不许再排新的扫描。
    for _ in 0..5 {
        editor.update(cx, |editor, cx| editor.sync_workspace_models(cx));
        cx.run_until_parked();
    }
    let after_frames = editor.read_with(cx, |editor, _| editor.dir_scan_attempts.get());
    editor.read_with(cx, |editor, _| {
        let tree = editor.workspace.file_tree.as_ref().expect("树还在");
        let node = find_workspace_node(std::slice::from_ref(tree), &dir_id).expect("节点还在");
        assert!(
            !node.children_loaded,
            "这次扫描本来就该失败：目录已经从盘上删了"
        );
        assert!(
            node.children_load_failed,
            "失败没留痕，每帧的补齐会一直重新排队"
        );
    });
    assert_eq!(
        after_frames, after_expand,
        "扫失败的目录被每帧重试了：展开时 {after_expand} 次，五帧之后 {after_frames} 次"
    );
}

#[gpui::test]
fn workspace_tree_empty_directories_keep_native_row_and_disclosure_semantics(
    cx: &mut TestAppContext,
) {
    use gpui_kit::test::TestWindowExt as _;
    let root = std::env::temp_dir().join(format!("velora-tree-controls-{}", uuid::Uuid::new_v4()));
    let empty = root.join("a-empty");
    fs::create_dir_all(&empty).unwrap();
    fs::write(root.join("z.md"), "file").unwrap();
    // 树里的路径统一去掉 Windows 的 `\\?\` 前缀（`strip_verbatim_prefix`），
    // 用例按同一口径比较，别把某个平台的写法钉进断言。
    let root = crate::file_url::strip_verbatim_prefix(&root.canonicalize().unwrap());
    let empty = root.join("a-empty");
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    cx.run_until_parked();
    let id = file_node_id(&empty);
    editor.update(cx, |editor, cx| editor.toggle_workspace_node(&id, cx));
    cx.run_until_parked();
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    editor.read_with(cx, |editor, _| {
        let tree = editor.workspace.file_tree.as_ref().unwrap();
        let directory = find_workspace_node(std::slice::from_ref(tree), &id).unwrap();
        assert!(directory.children_loaded && directory.children.is_empty());
    });
    let hash = super::super::stable_node_hash(&id);
    let toggle = cx.update(|window, _| {
        assert_eq!(
            window
                .try_find("workspace-panel-scroll")
                .and_then(|item| item.role()),
            Some(gpui::Role::Tree)
        );
        assert_eq!(
            window
                .try_find("workspace-tree-viewport")
                .and_then(|item| item.role()),
            Some(gpui::Role::Group),
            "树应有独立的原生虚拟滚动视口"
        );
        let row = window.find(("workspace-node", hash));
        assert_eq!(row.role(), Some(gpui::Role::TreeItem));
        assert_eq!(row.label(), Some("a-empty"));
        assert_eq!(row.expanded(), Some(true));
        let disclosure = window.find(("workspace-node-toggle", hash));
        assert_eq!(disclosure.role(), Some(gpui::Role::Button));
        disclosure.bounds().center()
    });
    let icon_selector: &'static str =
        Box::leak(format!("workspace-node-icon-{hash}").into_boxed_str());
    let icon = cx.debug_bounds(icon_selector).expect("目录图标");
    let disclosure = cx.update(|window, _| window.find(("workspace-node-toggle", hash)).bounds());
    let chevron_selector: &'static str =
        Box::leak(format!("workspace-node-chevron-{hash}").into_boxed_str());
    let chevron = cx.debug_bounds(chevron_selector).expect("树箭头图形");
    assert!(chevron.size.width < icon.size.width && chevron.size.width >= icon.size.width * 0.75,
        "箭头应比文件夹略小，仍保持清晰：箭头 {chevron:?}，图标 {icon:?}");
    assert_eq!(chevron.center().y, icon.center().y, "箭头图形与图标同轴");
    assert_eq!(
        disclosure.center().y,
        icon.center().y,
        "箭头与文件夹图标垂直居中"
    );
    let file_hash = super::super::stable_node_hash(&file_node_id(&root.join("z.md")));
    let file_selector: &'static str =
        Box::leak(format!("workspace-node-icon-{file_hash}").into_boxed_str());
    let file_icon = cx.debug_bounds(file_selector).expect("同级文件图标");
    assert_eq!(file_icon.left(), icon.left(), "同级文件与目录的图标列对齐");
    cx.simulate_click(toggle, gpui::Modifiers::none());
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    editor.read_with(cx, |editor, _| {
        assert!(
            !editor.workspace.expanded.contains(&id),
            "展开按钮只能切换一次"
        );
        assert_eq!(
            editor.workspace_files_rows().len(),
            3,
            "空目录不生成虚假子行"
        );
        assert_eq!(editor.buffer.text(), "正文");
    });
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.focus_workspace_tree(window, cx);
    });
    cx.simulate_keystrokes("home down");
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.selected,
            Some(WorkspaceSelection::Directory(empty.clone())),
            "方向键按真实节点选择"
        )
    });
    cx.simulate_keystrokes("right down");
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.expanded.contains(&id));
        assert_eq!(
            editor.workspace.selected,
            Some(WorkspaceSelection::File(root.join("z.md"))),
            "展开空目录后，下一个键盘目标应为真实文件"
        );
        assert_eq!(editor.buffer.text(), "正文", "选择行不自动打开文档");
    });
    cx.on_quit(move || fs::remove_dir_all(root).unwrap());
}

#[gpui::test]
fn workspace_outline_native_rows_support_keyboard_navigation_and_opening(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let source = "# Parent\n\n## Child\n\n# Last";
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, source.into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.workspace.active_tab = super::super::WorkspaceTab::Outline;
        editor.workspace.is_open = true;
        editor.sync_workspace_outline(cx);
        editor.workspace.expanded.remove("outline:0");
        window.activate_window();
        editor.focus_workspace_tree(window, cx);
        cx.notify();
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    editor.read_with(cx, |editor, _| {
        assert!(!editor.workspace.expanded.contains("outline:0"))
    });
    cx.simulate_keystrokes("home right down");
    editor.read_with(cx, |editor, _| {
        assert_eq!(
            editor.workspace.selected,
            Some(WorkspaceSelection::Outline("outline:2".into()))
        )
    });
    cx.simulate_keystrokes("down enter");
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    editor.read_with(cx, |editor, cx| {
        let last = editor
            .document
            .visible_blocks()
            .iter()
            .find(|block| block.entity.read(cx).display_text() == "Last")
            .unwrap()
            .entity
            .entity_id();
        assert_eq!(
            editor.current_edit_target_entity_id_from_state(cx),
            Some(last)
        );
        assert_eq!(editor.buffer.text(), source);
    });
}

#[gpui::test]
fn workspace_tree_virtual_rows_measure_name_validation_and_reuse_size_cache(
    cx: &mut TestAppContext,
) {
    let root = std::env::temp_dir().join(format!("velora-tree-measure-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("note.md"), "note").unwrap();
    let root = crate::file_url::strip_verbatim_prefix(&root.canonicalize().unwrap());
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update(cx, |editor, cx| editor.set_workspace_root(root.clone(), cx));
    cx.run_until_parked();
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let sizes = editor.read_with(cx, |editor, _| {
        editor
            .workspace
            .tree_list_layout
            .as_ref()
            .unwrap()
            .sizes
            .clone()
    });
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    editor.read_with(cx, |editor, _| {
        assert!(
            std::rc::Rc::ptr_eq(
                &sizes,
                &editor.workspace.tree_list_layout.as_ref().unwrap().sizes
            ),
            "普通重绘复用原生行尺寸缓存"
        )
    });
    editor.update_in(cx, |editor, window, cx| {
        window.activate_window();
        editor.create_generic_workspace_file(window, cx);
    });
    for _ in 0..4 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let normal = editor.read_with(cx, |editor, _| {
        editor
            .workspace
            .name_edit
            .as_ref()
            .unwrap()
            .row_height
            .unwrap()
    });
    cx.simulate_keystrokes("enter");
    for _ in 0..4 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    editor.read_with(cx, |editor, _| {
        let edit = editor.workspace.name_edit.as_ref().unwrap();
        assert!(edit.error.is_some());
        assert!(edit.row_height.unwrap() > normal, "校验信息应占据真实行高");
    });
    cx.simulate_input("valid.md");
    for _ in 0..4 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    editor.read_with(cx, |editor, _| {
        let edit = editor.workspace.name_edit.as_ref().unwrap();
        assert!(edit.error.is_none());
        assert_eq!(edit.row_height, Some(normal), "错误消失后不保留多余空白");
        assert_eq!(editor.buffer.text(), "正文");
    });
    cx.on_quit(move || fs::remove_dir_all(root).unwrap());
}

#[gpui::test]
fn workspace_tree_failed_directory_does_not_keep_a_loading_row(cx: &mut TestAppContext) {
    let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));
    editor.update(cx, |editor, _| {
        let id = "failed-directory".to_string();
        editor.workspace.expanded.insert(id.clone());
        editor.workspace.file_tree = Some(WorkspaceTreeNode {
            id,
            label: "failed".into(),
            kind: WorkspaceTreeKind::Directory(PathBuf::from("failed")),
            children: Vec::new(),
            children_loaded: false,
            children_load_failed: true,
        });
        assert!(
            !editor.workspace_files_rows().iter().any(|row| matches!(
                row,
                super::super::render_panel::WorkspaceTreeRow::Loading(..)
            )),
            "扫描失败后不能继续表示正在加载"
        );
    });
}

#[test]
fn workspace_folder_icons_distinguish_styles_and_expansion_and_are_embedded() {
    use crate::config::FolderIconStyle;
    use gpui::AssetSource as _;
    let mut paths = std::collections::HashSet::new();
    for style in [FolderIconStyle::Blue, FolderIconStyle::Outline] {
        let closed = Editor::workspace_folder_icon_path(style, false);
        let expanded = Editor::workspace_folder_icon_path(style, true);
        assert_ne!(
            closed, expanded,
            "展开与收起的文件夹必须有不同图形：{style:?}"
        );
        for path in [closed, expanded] {
            assert!(paths.insert(path.clone()), "两种图标样式不能共用同一个图形");
            let bytes = crate::VeloraAssets
                .load(&path)
                .unwrap()
                .expect("图标随安装包嵌入");
            let svg = std::str::from_utf8(&bytes).unwrap();
            assert!(svg.contains("<svg"));
            assert!(
                svg.contains(r#"viewBox="0 0 24 24""#),
                "文件夹使用相同的尺寸基准"
            );
            if style == FolderIconStyle::Blue {
                assert!(
                    svg.contains(r#"fill="currentColor""#) && !svg.contains(r#"fill="none""#),
                    "蓝色文件夹展开和收起均使用实心图形"
                );
            }
        }
    }
}
