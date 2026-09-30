use super::super::{
    Editor, TreeSortPreference, WorkspaceSelection,
    WorkspaceState,
    WorkspaceTreeKind, build_outline_tree, clamp_workspace_panel_width,
    create_workspace_file, create_workspace_folder, is_code_file, tree_node_path,
    path_is_affected, prune_outline_state, remap_moved_path, rewrite_relative_image_targets,
    scan_workspace_dir,
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

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan tree");
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
    let typed = scan_workspace_dir(&root, TreeSortPreference::Type).expect("scan typed");
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

    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).expect("scan code workspace");
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
    cx.update(|window, cx| window.draw(cx).clear());
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
    cx.update(|window, cx| window.draw(cx).clear());
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
    cx.update(|window, cx| window.draw(cx).clear());
    editor.read_with(cx, |editor, _| {
        assert!(editor.workspace.context_menu.unwrap().has_target);
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
    assert_eq!(
        remap_moved_path(
            Path::new("/workspace/old.md"),
            Path::new("/workspace/old.md"),
            Path::new("/workspace/new.md"),
            false,
        ),
        Some(PathBuf::from("/workspace/new.md"))
    );
}

#[test]
fn moving_a_markdown_file_rewrites_relative_inline_image_paths() {
    let markdown = "![diagram](./assets/diagram.png \"Diagram\")\n\n![online](https://example.com/image.png)";
    assert_eq!(
        rewrite_relative_image_targets(
            markdown,
            Path::new("/workspace/docs"),
            Path::new("/workspace/notes"),
        ),
        "![diagram](../docs/assets/diagram.png \"Diagram\")\n\n![online](https://example.com/image.png)"
    );
}

#[test]
fn moving_a_markdown_file_rewrites_reference_image_definitions() {
    let markdown = "![cover][hero]\n\n[hero]: ./assets/cover.png \"Cover\"";
    assert_eq!(
        rewrite_relative_image_targets(
            markdown,
            Path::new("/workspace/docs"),
            Path::new("/workspace/notes"),
        ),
        "![cover][hero]\n\n[hero]: ../docs/assets/cover.png \"Cover\""
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
