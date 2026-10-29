use super::{Editor, SplitDirection};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};
use std::path::PathBuf;

fn fixture(
    cx: &mut TestAppContext,
    check: impl FnOnce(Entity<Editor>, &mut VisualTestContext, PathBuf),
) {
    fixture_with_callback(cx, Box::new(check));
}

// 擦除回调类型，避免每条用例都重复实例化整套窗口初始化并撑大 macOS 展开表。
fn fixture_with_callback(
    cx: &mut TestAppContext,
    check: Box<dyn FnOnce(Entity<Editor>, &mut VisualTestContext, PathBuf) + '_>,
) {
    let root = std::env::temp_dir().join(format!("velora-split-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("分屏夹具目录");
    let root = crate::file_url::canonical_path(root);
    let _config = crate::config::override_test_config_root(root.join("config"));
    let path = root.join("notes.md");
    std::fs::write(&path, "正文\n").expect("分屏夹具");
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, String::new(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_workspace_file(path.clone(), window, cx)
    });
    check(editor, cx, path);
    cx.cx
        .on_quit(move || std::fs::remove_dir_all(root).expect("清理分屏夹具"));
}

fn redraw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
}

fn drag_tree_file(
    cx: &mut VisualTestContext,
    path: &std::path::Path,
    end: gpui::Point<gpui::Pixels>,
) {
    use gpui::{Modifiers, MouseButton, point, px};
    let selector = format!(
        "workspace-node-{}",
        super::super::stable_node_hash(&super::super::file_node_id(
            &crate::file_url::canonical_path(path)
        ))
    );
    let row = cx
        .debug_bounds(Box::leak(selector.into_boxed_str()))
        .expect("文件树节点");
    let start = point(row.left() + px(65.0), row.center().y);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        start + point(px(15.0), px(0.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
    redraw(cx);
    assert!(cx.cx.read(|cx| cx.has_active_drag()), "文件树文件应能拖动");
}

#[gpui::test]
fn tree_file_drag_previews_and_opens_an_unopened_file_in_a_new_split(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        use gpui::{Modifiers, MouseButton};
        let other = path.with_file_name("other.rs");
        std::fs::write(&other, "fn main() {}\n").expect("代码文件夹具");
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(path.parent().expect("夹具目录").to_path_buf(), cx);
        });
        redraw(cx);
        let edge = cx.debug_bounds("split-drop-1").expect("右侧落点");
        drag_tree_file(cx, &other, edge.center());
        assert!(
            cx.debug_bounds("split-preview-right").is_some(),
            "松手前显示分屏预览"
        );
        assert!(cx.debug_bounds("split-preview-label").is_some());
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.file_path.clone()),
            Some(path.clone()),
            "拖动期间不打开文件"
        );
        cx.simulate_mouse_up(edge.center(), MouseButton::Left, Modifiers::none());
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups
                .first()
                .expect("左组")
                .read_with(cx, |editor, _| editor.file_path.clone()),
            Some(path)
        );
        groups.get(1).expect("右组").read_with(cx, |editor, _| {
            assert_eq!(editor.file_path.as_ref(), Some(&other));
            assert_eq!(editor.buffer.text(), "fn main() {}\n");
            assert!(
                !editor
                    .workspace
                    .open_documents
                    .iter()
                    .find(|tab| tab.path == other)
                    .expect("新标签")
                    .preview,
                "拖入文件固定为标签"
            );
        });
    });
}

#[gpui::test]
fn tree_file_drag_splits_the_target_pane_and_preserves_shared_unsaved_content(
    cx: &mut TestAppContext,
) {
    fixture(cx, |editor, cx, path| {
        use gpui::{Modifiers, MouseButton, point};
        editor.update_in(cx, |editor, window, cx| {
            editor.set_workspace_root(path.parent().expect("夹具目录").to_path_buf(), cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左组").clone();
        left.update(cx, |editor, cx| {
            editor.buffer.edit(0..0, "未保存");
            editor.document_dirty = true;
            cx.notify();
        });
        redraw(cx);
        let bounds = cx.debug_bounds("editor-group-0").expect("左组正文");
        let end = point(bounds.center().x, bounds.top() + bounds.size.height * 0.9);
        drag_tree_file(cx, &path, end);
        assert!(cx.debug_bounds("split-preview-down").is_some());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 3);
        assert_eq!(groups.first(), Some(&left), "保留目标组的原视图");
        let upper = cx.debug_bounds("editor-group-0").expect("左上");
        let lower = cx.debug_bounds("editor-group-1").expect("左下");
        assert!(upper.bottom() <= lower.top(), "应拆分鼠标所在的左组");
        for group in groups {
            assert_eq!(
                group.read_with(cx, |editor, _| editor.buffer.text()),
                "未保存正文\n"
            );
        }
        assert_eq!(std::fs::read_to_string(path).expect("原文件"), "正文\n");
    });
}

#[gpui::test]
fn tree_file_drag_to_center_opens_in_place_and_cancellation_keeps_tabs(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        use gpui::{Modifiers, MouseButton, point, px};
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "其他\n").expect("夹具");
        editor.update(cx, |editor, cx| {
            editor.set_workspace_root(path.parent().expect("目录").to_path_buf(), cx)
        });
        redraw(cx);
        // Windows 的首个标签可以从窗口左缘开始，取消点必须在窗口之外。
        let outside = point(px(-20.0), px(-20.0));
        drag_tree_file(cx, &other, outside);
        cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.file_path.clone()),
            Some(path)
        );
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.workspace.open_documents.len()),
            1
        );
        let center = cx.debug_bounds("split-drop-4").expect("中央落点").center();
        drag_tree_file(cx, &other, center);
        cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
        redraw(cx);
        editor.read_with(cx, |editor, _| {
            assert!(editor.split_workspace.is_none());
            assert_eq!(editor.file_path.as_ref(), Some(&other));
            assert_eq!(editor.workspace.open_documents.len(), 2);
        });
    });
}

#[gpui::test]
fn tree_file_drag_to_an_inactive_pane_activates_it_without_moving_the_source_tab(
    cx: &mut TestAppContext,
) {
    fixture(cx, |editor, cx, path| {
        use gpui::{Modifiers, MouseButton};
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "其他\n").expect("夹具");
        editor.update_in(cx, |editor, window, cx| {
            editor.set_workspace_root(path.parent().expect("目录").to_path_buf(), cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左组").clone();
        let right = groups.get(1).expect("右组").clone();
        left.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(other.clone(), window, cx)
        });
        right.update(cx, |editor, cx| {
            editor.buffer.edit(0..0, "未保存");
            editor.document_dirty = true;
            cx.notify();
        });
        redraw(cx);
        let center = cx.debug_bounds("editor-group-0").expect("左组").center();
        drag_tree_file(cx, &path, center);
        cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            2
        );
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.active_split_editor()),
            Some(left.clone()),
            "应激活落点组"
        );
        for group in [left, right] {
            group.read_with(cx, |editor, _| {
                assert_eq!(editor.file_path.as_ref(), Some(&path));
                assert_eq!(editor.buffer.text(), "未保存正文\n");
                assert!(
                    editor
                        .workspace
                        .open_documents
                        .iter()
                        .any(|tab| tab.path == path)
                );
            });
        }
    });
}

fn split_untitled_by_tree_drag(
    editor: &Entity<Editor>,
    cx: &mut VisualTestContext,
    path: &std::path::Path,
) {
    use gpui::{Modifiers, MouseButton};
    editor.update(cx, |editor, cx| {
        editor.set_workspace_root(path.parent().expect("目录").to_path_buf(), cx);
        editor.workspace.open_documents.clear();
        editor.workspace.active_document = None;
        editor.replace_document_from_markdown("未保存的新文档\n".into(), None, cx);
        editor.document_dirty = true;
        editor.show_welcome = false;
    });
    redraw(cx);
    let edge = cx.debug_bounds("split-drop-1").expect("右侧落点").center();
    drag_tree_file(cx, &path, edge);
    cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::none());
    redraw(cx);
}

#[gpui::test]
fn tree_file_drag_does_not_discard_an_unsaved_untitled_document(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        split_untitled_by_tree_drag(&editor, cx, &path);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 2, "未命名文档保留在原分屏");
        assert!(
            cx.debug_bounds("untitled-tab").is_some(),
            "未命名文档显示 Untitled 标签"
        );
        groups.first().expect("原分屏").read_with(cx, |editor, _| {
            assert_eq!(editor.buffer.text(), "未保存的新文档\n");
            assert!(editor.document_dirty);
            assert!(editor.file_path.is_none());
        });
        assert_eq!(
            groups
                .get(1)
                .expect("新分屏")
                .read_with(cx, |editor, _| editor.file_path.clone()),
            Some(path.clone())
        );
        assert!(
            !editor.update_in(cx, |editor, window, cx| editor
                .on_window_should_close(window, cx)),
            "未命名文档必须参与关窗检查"
        );
        groups.get(1).expect("文件分屏").update(cx, |editor, cx| {
            editor.buffer.edit(0..0, "已修改");
            editor.document_dirty = true;
            cx.notify();
        });
        redraw(cx);
        let saved = path.with_file_name("saved.md");
        editor.update_in(cx, |editor, window, cx| {
            editor.on_save_and_close(&gpui::ClickEvent::default(), window, cx)
        });
        cx.run_until_parked();
        assert!(
            cx.cx.did_prompt_for_new_path(),
            "保存并关闭应先为 Untitled 选择路径"
        );
        cx.cx.simulate_new_path_selection(|_| Some(saved.clone()));
        cx.run_until_parked();
        assert_eq!(
            std::fs::read_to_string(saved).expect("保存未命名文档"),
            "未保存的新文档\n"
        );
        assert_eq!(
            std::fs::read_to_string(path).expect("保存其他脏文件"),
            "已修改正文\n"
        );
        assert!(cx.cx.windows().is_empty(), "所有分屏都保存后才关闭窗口");
    });
}

#[gpui::test]
fn cancelling_untitled_split_save_keeps_the_window_and_unsaved_content(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        split_untitled_by_tree_drag(&editor, cx, &path);
        editor.update_in(cx, |editor, window, cx| {
            editor.on_save_and_close(&gpui::ClickEvent::default(), window, cx);
        });
        cx.run_until_parked();
        assert!(cx.cx.did_prompt_for_new_path());
        cx.cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
        assert_eq!(cx.cx.windows().len(), 1);
        assert!(!editor.update_in(cx, |editor, window, cx| {
            editor.on_window_should_close(window, cx)
        }));
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        groups
            .first()
            .expect("Untitled 分屏")
            .read_with(cx, |editor, _| {
                assert_eq!(editor.buffer.text(), "未保存的新文档\n");
                assert!(editor.document_dirty);
                assert!(!editor.pending_close_after_save);
            });
        assert_eq!(std::fs::read_to_string(path).expect("原文件"), "正文\n");
    });
}

#[gpui::test]
fn untitled_split_autosave_keeps_a_recovery_snapshot_without_writing_a_file(
    cx: &mut TestAppContext,
) {
    fixture(cx, |editor, cx, path| {
        cx.cx
            .update(|cx| crate::config::EditorSettings::init(cx, true));
        split_untitled_by_tree_drag(&editor, cx, &path);
        let group = editor
            .read_with(cx, |editor, _| editor.split_groups())
            .first()
            .expect("Untitled 分屏")
            .clone();
        let recovery_id = group.read_with(cx, |editor, _| editor.recovery_id);
        group.update(cx, |editor, cx| {
            editor.buffer.edit(0..0, "继续编辑");
            editor.schedule_autosave(cx);
            cx.notify();
        });
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(2));
        cx.run_until_parked();
        let snapshots = crate::config::read_recovery_snapshots().expect("读取恢复快照");
        let snapshot = snapshots
            .iter()
            .find(|snapshot| snapshot.id == recovery_id)
            .expect("Untitled 应有独立恢复快照");
        assert_eq!(snapshot.markdown, "继续编辑未保存的新文档\n");
        assert!(snapshot.source_path.is_none(), "Untitled 不是磁盘文件路径");
        assert_eq!(std::fs::read_to_string(path).expect("原文件"), "正文\n");
        assert!(group.read_with(cx, |editor, _| editor.document_dirty));
    });
}

#[gpui::test]
fn split_command_copies_the_document_and_keeps_one_shared_buffer(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 2, "右键分屏应保留原视图并创建第二个编辑器组");
        redraw(cx);
        let first = cx
            .debug_bounds("editor-group-0")
            .expect("第一个编辑器组应显示");
        let second = cx
            .debug_bounds("editor-group-1")
            .expect("第二个编辑器组应显示");
        assert!(first.right() <= second.left(), "向右分屏应左右排列");
        for group in &groups {
            assert_eq!(
                group.read_with(cx, |editor, _| editor.file_path.clone()),
                Some(path.clone())
            );
        }
        groups.first().expect("左侧分组").update(cx, |editor, _| {
            editor.buffer.edit(0..0, "共享");
        });
        assert_eq!(
            groups
                .get(1)
                .expect("右侧分组")
                .read_with(cx, |editor, _| editor.buffer.text()),
            "共享正文\n"
        );
    });
}

/// 分屏长会话里共享缓冲区的变更队列不许随编辑次数无界增长。
///
/// 队列由「存在共享视图」时的每一笔 `buffer.edit` 推入，只有同步循环
/// （组实体的观察通知 → `sync_shared_groups`）收尾那句 clear 才清。这条钉住
/// 「消费即清」的闭环：清空那行被删掉、或通知链断掉，队列就会随会话长度线性涨。
#[gpui::test]
fn a_long_split_session_keeps_the_shared_change_queue_bounded(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        for _ in 0..100 {
            cx.simulate_input("x");
        }
        redraw(cx);
        let queued = editor.read_with(cx, |editor, _| editor.buffer.shared_changes().len());
        assert!(
            queued <= 8,
            "100 次分屏输入后共享变更队列还剩 {queued} 条：没有消费方清空，队列随会话无界增长"
        );
    });
}

#[gpui::test]
fn split_edits_refresh_the_other_projection_and_share_undo(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        cx.simulate_input("新");
        cx.run_until_parked();
        let left = groups.first().expect("左侧分组");
        let right = groups.get(1).expect("右侧分组");
        assert!(
            right.read_with(cx, |editor, cx| editor
                .document
                .first_root()
                .expect("正文块")
                .read(cx)
                .record
                .title_markdown()
                .contains('新')),
            "输入应落在新建的活动分组"
        );
        let left_title = left.read_with(cx, |editor, cx| {
            editor
                .document
                .first_root()
                .expect("正文块")
                .read(cx)
                .record
                .title_markdown()
        });
        assert!(
            left_title.contains('新'),
            "另一侧的实际块投影也必须刷新，而不只是缓冲区变了"
        );
        assert!(
            left.read_with(cx, |editor, _| editor.document_dirty),
            "两侧应显示同一个未保存状态"
        );
        editor.update(cx, |editor, cx| {
            editor.activate_split_group(left.entity_id(), cx)
        });
        left.update(cx, |editor, cx| editor.undo_document(cx));
        cx.run_until_parked();
        assert_eq!(
            right.read_with(cx, |editor, _| editor.buffer.text()),
            "正文\n",
            "应能从另一侧撤销刚才的输入"
        );
    });
}

#[gpui::test]
fn split_an_inactive_tab_copies_the_selected_file(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "另一篇\n").expect("第二篇夹具");
        editor.update_in(cx, |state, window, cx| {
            state.open_workspace_file(other.clone(), window, cx);
            state.request_split_document(
                editor.downgrade(),
                path.clone(),
                SplitDirection::Right,
                true,
                window,
                cx,
            );
        });
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let first = groups.first().expect("原分组");
        let second = groups.get(1).expect("新分组");
        assert_eq!(
            first.read_with(cx, |editor, _| editor.file_path.clone()),
            Some(other)
        );
        assert_eq!(
            second.read_with(cx, |editor, _| editor.file_path.clone()),
            Some(path)
        );
        assert_eq!(
            second.read_with(cx, |editor, _| editor.buffer.text()),
            "正文\n",
            "分屏非活动标签不能拿当前文件的缓冲区"
        );
    });
}

#[gpui::test]
fn split_tab_switch_keeps_unsaved_shared_content(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左侧");
        let right = groups.get(1).expect("右侧");
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "其他\n").expect("夹具");
        left.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(other, window, cx)
        });
        cx.simulate_input("修改");
        cx.run_until_parked();
        let latest = right.read_with(cx, |editor, _| editor.buffer.text());
        left.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(path, window, cx)
        });
        assert_eq!(
            left.read_with(cx, |editor, _| editor.buffer.text()),
            latest,
            "重开后台标签必须读取共享内容"
        );
    });
}

#[gpui::test]
fn split_window_close_guards_dirty_documents_in_every_group(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Down, window, cx)
        });
        redraw(cx);
        cx.simulate_input("未保存");
        cx.run_until_parked();
        let may_close = editor.update_in(cx, |editor, window, cx| {
            editor.on_window_should_close(window, cx)
        });
        assert!(!may_close, "分屏里的未保存内容必须阻止窗口直接关闭");
        redraw(cx);
        assert!(
            cx.debug_bounds("unsaved-changes-overlay").is_some(),
            "关窗确认必须实际显示"
        );
    });
}

#[gpui::test]
fn dragging_tabs_moves_between_groups_and_collapses_empty_groups(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左侧");
        let right = groups.get(1).expect("右侧");
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "其他\n").expect("夹具");
        right.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(other.clone(), window, cx)
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.transfer_split_tab(
                right.downgrade(),
                left.entity_id(),
                other.clone(),
                None,
                false,
                window,
                cx,
            )
        });
        assert!(
            left.read_with(cx, |editor, _| editor
                .workspace
                .open_documents
                .iter()
                .any(|tab| tab.path == other)),
            "中心落点应移动到目标组"
        );
        assert!(
            !right.read_with(cx, |editor, _| editor
                .workspace
                .open_documents
                .iter()
                .any(|tab| tab.path == other)),
            "普通拖拽应移出来源组"
        );
        editor.update_in(cx, |editor, window, cx| {
            editor.transfer_split_tab(
                right.downgrade(),
                left.entity_id(),
                path.clone(),
                None,
                false,
                window,
                cx,
            )
        });
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            1,
            "移空来源组后应折叠分屏"
        );
    });
}

#[gpui::test]
fn dragging_a_tab_to_the_content_edge_creates_a_split(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        use gpui::{Modifiers, MouseButton, point, px};
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "其他\n").expect("夹具");
        editor.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(other.clone(), window, cx)
        });
        redraw(cx);
        let tab = cx.debug_bounds("document-tab-1").expect("拖动标签");
        let edge = cx.debug_bounds("split-drop-1").expect("右侧分屏落点");
        let start = point(tab.left() + px(30.0), tab.center().y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start + point(px(15.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(edge.center(), MouseButton::Left, Modifiers::none());
        redraw(cx);
        assert!(cx.cx.read(|cx| cx.has_active_drag()), "应当进入标签拖拽");
        cx.simulate_mouse_up(edge.center(), MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 2, "鼠标拖到正文右侧应当创建编辑器组");
        assert_eq!(
            groups
                .first()
                .expect("左组")
                .read_with(cx, |editor, _| editor.file_path.clone()),
            Some(path)
        );
        assert_eq!(
            groups
                .get(1)
                .expect("右组")
                .read_with(cx, |editor, _| editor.file_path.clone()),
            Some(other)
        );
    });
}

#[gpui::test]
fn split_save_clears_dirty_state_and_keeps_shared_undo(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左组");
        let right = groups.get(1).expect("右组");
        cx.simulate_input("保存");
        cx.run_until_parked();
        right.update_in(cx, |editor, window, cx| {
            assert!(editor.save_to_existing_path(&path, window, cx))
        });
        cx.run_until_parked();
        assert!(
            !left.read_with(cx, |editor, _| editor.document_dirty),
            "保存后所有视图的脏位应清除"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("落盘"),
            right.read_with(cx, |editor, _| editor.buffer.text())
        );
        right.update_in(cx, |editor, window, cx| {
            editor.close_workspace_document(&path, window, cx)
        });
        cx.run_until_parked();
        left.update(cx, |editor, cx| editor.undo_document(cx));
        assert_eq!(
            left.read_with(cx, |editor, _| editor.buffer.text()),
            "正文\n",
            "关闭编辑来源视图后仍能撤销"
        );
    });
}

#[gpui::test]
fn split_session_restores_nested_groups_and_active_tabs(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        let right = editor
            .read_with(cx, |editor, _| editor.active_split_editor())
            .expect("右组");
        cx.update(|window, cx| right.update(cx, |editor, cx| {
            editor.split_active_document(SplitDirection::Down, window, cx)
        }));
        cx.run_until_parked();
        let layout = editor.read_with(cx, |editor, cx| editor.split_session_layout(cx));
        assert!(layout.is_some(), "会话必须记录分屏树而不是扁平标签集");
        let layout = layout.expect("分屏布局");
        let encoded = serde_json::to_string(&layout).expect("编码布局");
        let layout = serde_json::from_str(&encoded).expect("解码布局");
        let restored = cx
            .cx
            .new(|cx| Editor::from_markdown(cx, String::new(), None));
        cx.update(|window, cx| restored.update(cx, |editor, cx| {
            editor.restore_split_session(&layout, window, cx)
        }));
        let groups = restored.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 3, "应恢复嵌套三组布局");
        for group in groups {
            assert_eq!(
                group.read_with(cx, |editor, _| editor.file_path.clone()),
                Some(path.clone())
            );
        }
    });
}

#[gpui::test]
fn typing_in_a_split_keeps_the_other_groups_document_plan(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        let text = (0..200)
            .map(|index| format!("段落 {index}\n\n"))
            .collect::<String>();
        std::fs::write(&path, &text).expect("长文夹具");
        editor.update_in(cx, |editor, window, cx| {
            editor.replace_document_from_markdown(text, Some(path), cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        redraw(cx);
        let left = editor
            .read_with(cx, |editor, _| editor.split_groups().first().cloned())
            .expect("左组");
        let before = left.read_with(cx, |editor, _| editor.row_plan_rebuilds.get());
        cx.simulate_input("新");
        cx.run_until_parked();
        redraw(cx);
        assert_eq!(
            left.read_with(cx, |editor, _| editor.row_plan_rebuilds.get()),
            before,
            "普通输入只刷新受影响块，不能让另一侧每键重建整篇行计划"
        );
    });
}

#[gpui::test]
fn copy_modifier_drag_keeps_the_original_tab(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        use gpui::{Modifiers, MouseButton, point, px};
        redraw(cx);
        let tab = cx.debug_bounds("document-tab-0").expect("标签");
        let edge = cx.debug_bounds("split-drop-0").expect("左落点");
        let mut modifiers = Modifiers::none();
        if cfg!(target_os = "macos") {
            modifiers.alt = true;
        } else {
            modifiers.control = true;
        }
        let start = point(tab.left() + px(30.0), tab.center().y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start + point(px(15.0), px(0.0)),
            MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(edge.center(), MouseButton::Left, modifiers);
        redraw(cx);
        cx.simulate_mouse_up(edge.center(), MouseButton::Left, modifiers);
        cx.run_until_parked();
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 2, "复制修饰键拖拽单标签必须保留两个视图");
        for group in groups {
            assert_eq!(
                group.read_with(cx, |editor, _| editor.file_path.clone()),
                Some(path.clone())
            );
        }
    });
}

#[gpui::test]
fn source_edits_change_the_other_groups_markdown_structure(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左组");
        let right = groups.get(1).expect("右组");
        right.update(cx, |editor, cx| editor.toggle_view_mode(cx));
        redraw(cx);
        cx.simulate_input("# ");
        cx.run_until_parked();
        redraw(cx);
        assert_eq!(
            left.read_with(cx, |editor, cx| editor
                .document
                .first_root()
                .expect("标题块")
                .read(cx)
                .kind()),
            crate::components::BlockKind::Heading { level: 1 },
            "源码侧修改应按同一解析器刷新另一侧结构；左侧={}，右侧={}",
            left.read_with(cx, |editor, _| editor.buffer.text()),
            right.read_with(cx, |editor, _| editor.buffer.text())
        );
        assert_eq!(
            right.read_with(cx, |editor, _| editor.view_mode),
            crate::editor::ViewMode::Source,
            "两侧模式独立"
        );
    });
}

#[gpui::test]
fn closing_a_dirty_shared_view_keeps_the_document_and_undo_in_the_other_group(
    cx: &mut TestAppContext,
) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左组");
        let right = groups.get(1).expect("右组");
        cx.simulate_input("保留");
        cx.run_until_parked();
        right.update_in(cx, |editor, window, cx| {
            editor.close_workspace_document(&path, window, cx)
        });
        cx.run_until_parked();
        assert!(
            !right.read_with(cx, |editor, _| editor.modal_is_open()),
            "仍有别的视图时关闭单个视图不应丢弃或询问共享修改"
        );
        assert!(left.read_with(cx, |editor, _| editor.buffer.text().contains("保留")));
        left.update(cx, |editor, cx| editor.undo_document(cx));
        assert_eq!(
            left.read_with(cx, |editor, _| editor.buffer.text()),
            "正文\n"
        );
    });
}

#[gpui::test]
fn autosave_includes_dirty_documents_in_inactive_groups(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        cx.cx.update(|cx| {
            crate::config::EditorSettings::init(cx, true);
            crate::config::EditorSettings::set_autosave_in_memory(true, cx);
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "另一篇\n").expect("夹具");
        groups
            .get(1)
            .expect("右组")
            .update_in(cx, |editor, window, cx| {
                editor.open_workspace_file(other.clone(), window, cx)
            });
        cx.run_until_parked();
        for group in &groups {
            group.update(cx, |editor, cx| {
                editor.buffer.edit(0..0, "新增");
                editor.rebuild_document_from_buffer(cx);
                editor.document_dirty = true;
                editor.schedule_autosave(cx);
                cx.notify();
            });
        }
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(2));
        cx.run_until_parked();
        assert_eq!(
            std::fs::read_to_string(&path).expect("左组文件"),
            "新增正文\n",
            "后台组也应参与自动保存"
        );
        assert_eq!(
            std::fs::read_to_string(&other).expect("右组文件"),
            "新增另一篇\n"
        );
        for group in groups {
            assert!(!group.read_with(cx, |editor, _| editor.document_dirty));
        }
    });
}

#[gpui::test]
fn window_save_rejects_a_snapshot_changed_in_another_group(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        cx.simulate_input("第一笔");
        cx.run_until_parked();
        editor.update(cx, |editor, cx| editor.collect_split_tabs(cx));
        let snapshot = editor
            .read_with(cx, |editor, _| {
                editor.workspace.open_documents.first().cloned()
            })
            .expect("保存快照");
        let right = editor
            .read_with(cx, |editor, _| editor.active_split_editor())
            .expect("右组");
        right.update(cx, |editor, _| {
            editor.buffer.edit(0..0, "后来的输入");
        });
        assert!(
            !editor.read_with(cx, |editor, _| editor.workspace_save_snapshot_is_current(
                snapshot.recovery_id,
                &path,
                &snapshot.markdown
            )),
            "关窗保存不能只校验扁平标签快照，否则会写旧内容后关闭窗口"
        );
    });
}

#[gpui::test]
fn split_divider_drag_changes_the_group_widths(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        use gpui::{Modifiers, MouseButton, point, px};
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let first = cx.debug_bounds("editor-group-0").expect("左组");
        let divider = cx.debug_bounds("split-divider").expect("分隔条");
        let start = divider.center();
        let end = start + point(px(50.0), px(0.0));
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start + point(px(10.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        redraw(cx);
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        redraw(cx);
        assert!(
            cx.debug_bounds("editor-group-0")
                .expect("调整后的左组")
                .size
                .width
                > first.size.width + px(30.0),
            "拖动分隔条应调整宽度"
        );
    });
}

#[gpui::test]
fn save_as_cannot_overwrite_a_dirty_document_in_another_group(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "另一篇\n").expect("夹具");
        let left = groups.first().expect("左组");
        let right = groups.get(1).expect("右组");
        right.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(other, window, cx)
        });
        right.update_in(cx, |editor, window, cx| {
            editor.close_workspace_document(&path, window, cx)
        });
        cx.run_until_parked();
        left.update(cx, |editor, cx| {
            editor.buffer.edit(0..0, "未保存");
            editor.document_dirty = true;
            cx.notify();
        });
        cx.run_until_parked();
        let result = right.update(cx, |editor, cx| {
            editor.complete_save_request(
                path.clone(),
                editor.document_save_identity(),
                editor.buffer.text(),
                editor.buffer.file_bytes(),
                cx,
            )
        });
        assert!(result.is_err(), "另存为必须保护其他组里尚未保存的目标文件");
        assert_eq!(std::fs::read_to_string(&path).expect("原文件"), "正文\n");
    });
}

#[gpui::test]
fn table_changes_refresh_peer_cells_without_retaining_old_cells(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        let text = "| A | B |\n| --- | --- |\n| 原文 | 内容 |\n";
        std::fs::write(&path, text).expect("表格夹具");
        editor.update_in(cx, |editor, window, cx| {
            editor.replace_document_from_markdown(text.to_string(), Some(path), cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        cx.run_until_parked();
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左组");
        let right = groups.get(1).expect("右组");
        for _ in 0..3 {
            right.update(cx, |editor, cx| {
                let position = editor.buffer.text().find("原文").expect("表格正文");
                editor.buffer.edit(position..position, "新");
                editor.rebuild_document_from_buffer(cx);
                editor.document_dirty = true;
                cx.notify();
            });
            cx.run_until_parked();
        }
        assert_eq!(
            left.read_with(cx, |editor, _| editor.table_cells.len()),
            4,
            "重投影后旧格子绑定必须移除"
        );
        assert!(
            left.read_with(cx, |editor, cx| editor.table_cells.values().any(
                |binding| binding
                    .cell
                    .read(cx)
                    .record
                    .title_markdown()
                    .contains("新新新原文")
            )),
            "另一侧必须显示新的表格格子"
        );
    });
}

#[gpui::test]
fn split_shortcut_and_tab_context_menu_reach_the_same_editor_groups(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        redraw(cx);
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-\\"
        } else {
            "ctrl-\\"
        });
        cx.run_until_parked();
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            2,
            "默认分屏快捷键应创建组"
        );
        redraw(cx);
        let tab = cx.debug_bounds("document-tab-0").expect("右组标签");
        let click = gpui::point(tab.left() + gpui::px(24.0), tab.center().y);
        cx.simulate_mouse_down(click, gpui::MouseButton::Right, gpui::Modifiers::none());
        redraw(cx);
        let menu = cx.debug_bounds("tab-context-panel").expect("标签右键菜单");
        assert!(
            menu.left() >= gpui::px(0.0)
                && menu.right() <= cx.update(|window, _| window.viewport_size().width),
            "分屏菜单仍应使用窗口坐标"
        );
        assert!(
            (menu.top() - click.y).abs() < gpui::px(8.0),
            "右组菜单不能重复加上分屏原点"
        );
    });
}

#[gpui::test]
fn dragging_between_group_centers_moves_the_tab_and_collapses_its_source(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        use gpui::{Modifiers, MouseButton, point, px};
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        let right = editor
            .read_with(cx, |editor, _| editor.active_split_editor())
            .expect("右组");
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "另一篇\n").expect("夹具");
        right.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(other.clone(), window, cx);
            editor.close_workspace_document(&path, window, cx);
        });
        cx.run_until_parked();
        redraw(cx);
        let tab = cx.debug_bounds("document-tab-0").expect("来源标签");
        let left = cx.debug_bounds("editor-group-0").expect("左组");
        let start = point(tab.left() + px(25.0), tab.center().y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start + point(px(15.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(left.center(), MouseButton::Left, Modifiers::none());
        redraw(cx);
        cx.simulate_mouse_up(left.center(), MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 1, "跨组拖走最后标签应折叠来源组");
        assert_eq!(
            groups
                .first()
                .expect("剩余组")
                .read_with(cx, |editor, _| editor.file_path.clone()),
            Some(other)
        );
    });
}

#[gpui::test]
fn external_reload_updates_a_document_hidden_in_all_groups(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let other = path.with_file_name("other.md");
        std::fs::write(&other, "另一篇\n").expect("夹具");
        for group in &groups {
            group.update_in(cx, |editor, window, cx| {
                editor.open_workspace_file(other.clone(), window, cx)
            });
        }
        cx.run_until_parked();
        std::fs::write(&path, "外部更新\n").expect("外部写入");
        editor.update(cx, |editor, cx| editor.on_watched_path_changed(&path, cx));
        groups
            .first()
            .expect("左组")
            .update_in(cx, |editor, window, cx| {
                editor.open_workspace_file(path, window, cx)
            });
        assert_eq!(
            groups
                .first()
                .expect("左组")
                .read_with(cx, |editor, _| editor.buffer.text()),
            "外部更新\n",
            "后台标签重开时也必须取到外部更新后的共享内容"
        );
    });
}

#[gpui::test]
fn closing_an_inactive_view_does_not_replace_the_shared_undo_history(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左组");
        let right = groups.get(1).expect("右组");
        cx.simulate_input("最新输入");
        cx.run_until_parked();
        left.update_in(cx, |editor, window, cx| {
            editor.close_workspace_document(&path, window, cx)
        });
        cx.run_until_parked();
        assert!(
            right.read_with(cx, |editor, _| editor.document_dirty),
            "关闭后台视图不能清除共享脏位"
        );
        right.update(cx, |editor, cx| editor.undo_document(cx));
        assert_eq!(
            right.read_with(cx, |editor, _| editor.buffer.text()),
            "正文\n",
            "旧视图的撤销栈不能覆盖实际编辑者的最新撤销栈"
        );
    });
}

#[gpui::test]
fn typing_with_a_source_peer_does_not_rescan_all_source_highlights(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        let text = (0..1100)
            .map(|line| format!("正文 {line}\n"))
            .collect::<String>();
        std::fs::write(&path, &text).expect("源码分块夹具");
        editor.update_in(cx, |editor, window, cx| {
            editor.replace_document_from_markdown(text, Some(path), cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        let left = editor
            .read_with(cx, |editor, _| editor.split_groups().first().cloned())
            .expect("源码组");
        left.update(cx, |editor, cx| editor.toggle_view_mode(cx));
        redraw(cx);
        let before = left.read_with(cx, |editor, _| editor.source_fence_full_rebuilds.get());
        cx.simulate_input("新");
        cx.run_until_parked();
        redraw(cx);
        assert!(
            left.read_with(cx, |editor, cx| editor
                .document
                .first_root()
                .expect("源码首块")
                .read(cx)
                .record
                .title_markdown()
                .starts_with('新')),
            "另一侧的源码投影应显示新输入"
        );
        assert_eq!(
            left.read_with(cx, |editor, _| editor.source_fence_full_rebuilds.get()),
            before,
            "普通输入不能重扫源码分屏的全部高亮；只有接缝状态改变才级联后续块"
        );
    });
}

#[gpui::test]
fn split_shortcut_targets_the_active_group_when_document_focus_is_outside_the_groups(
    cx: &mut TestAppContext,
) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.workspace.root = path.parent().map(std::path::Path::to_path_buf);
            editor.workspace.is_open = true;
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let active = editor
            .read_with(cx, |editor, _| editor.active_split_editor())
            .expect("活动组");
        active.update_in(cx, |editor, window, cx| {
            editor.focus_workspace_tree(window, cx)
        });
        redraw(cx);
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-\\"
        } else {
            "ctrl-\\"
        });
        cx.run_until_parked();
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            3,
            "焦点在侧栏等分组外区域时，分屏命令仍应作用到活动编辑器组"
        );
    });
}

#[gpui::test]
fn reload_from_the_window_conflict_dialog_updates_every_shared_view(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        cx.simulate_input("本地");
        cx.run_until_parked();
        std::fs::write(&path, "外部更新\n").expect("外部修改");
        editor.update(cx, |editor, cx| {
            editor.report_external_change_conflict(path, "磁盘版本已改变".into(), cx)
        });
        editor.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
        cx.run_until_parked();
        for group in editor.read_with(cx, |editor, _| editor.split_groups()) {
            assert_eq!(
                group.read_with(cx, |editor, _| editor.buffer.text()),
                "外部更新\n",
                "窗口级冲突框的重载必须作用到实际文档"
            );
            assert!(!group.read_with(cx, |editor, _| editor.document_dirty));
        }
        assert!(
            !editor.read_with(cx, |editor, _| editor.has_external_autosave_conflict()),
            "成功重载后应恢复自动保存"
        );
    });
}

#[gpui::test]
fn editor_groups_keep_their_tabs_in_one_window_titlebar(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let body = cx.debug_bounds("editor-group-0").expect("分屏正文");
        for group in groups {
            let name = format!("group-tab-{:?}-0", group.entity_id());
            let tabs = cx
                .debug_bounds(Box::leak(name.into_boxed_str()))
                .expect("分组标签");
            assert!(
                tabs.bottom() <= body.top(),
                "所有分组的标签必须保留在同一条窗口标题栏，不能另起一行挤占正文"
            );
        }
        let active = editor
            .read_with(cx, |editor, _| editor.active_split_editor())
            .expect("活动组");
        active.update_in(cx, |editor, window, cx| editor.close_active_tab(window, cx));
        cx.run_until_parked();
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 1, "关闭后只留一组");
        let group = groups.first().expect("剩余组");
        let name = format!("group-tab-{:?}-0", group.entity_id());
        let tabs = cx
            .debug_bounds(Box::leak(name.into_boxed_str()))
            .expect("单组标签");
        let body = cx.debug_bounds("editor-group-0").expect("单组正文");
        assert!(tabs.bottom() <= body.top(), "收回单组后仍只保留一条标题栏");
    });
}

#[gpui::test]
fn editor_groups_do_not_frame_the_document_with_an_active_border(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let body = cx.debug_bounds("editor-group-1").expect("活动分屏");
        let group = groups.get(1).expect("活动组");
        let name = format!("editor-column-{:?}", group.entity_id());
        let column = cx
            .debug_bounds(Box::leak(name.into_boxed_str()))
            .expect("正文列");
        assert_eq!(
            column.left(),
            body.left(),
            "活动组不能在正文外围加绿色边框及内缩"
        );
        assert_eq!(column.top(), body.top(), "正文顶部也不能加整圈活动边框");
    });
}

#[gpui::test]
fn split_autosave_uses_the_shared_buffers_selected_file_encoding(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        cx.cx.update(|cx| {
            crate::config::EditorSettings::init(cx, true);
            crate::config::EditorSettings::set_autosave_in_memory(true, cx);
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        let active = editor
            .read_with(cx, |editor, _| editor.active_split_editor())
            .expect("活动组");
        let shape = crate::editor::buffer::FileShape::detect(&[0xff, 0xfe, b'a', 0]);
        active.update(cx, |editor, cx| {
            editor.buffer.restore_file_shape(shape);
            editor.document_dirty = true;
            editor.schedule_autosave(cx);
            cx.notify();
        });
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(2));
        cx.run_until_parked();
        assert_eq!(
            std::fs::read(&path).expect("自动保存文件"),
            shape.encode("正文\n"),
            "窗口统一保存也必须写缓冲区选择的编码，不能重新沿用磁盘旧编码"
        );
    });
}

#[gpui::test]
fn split_drag_shows_a_labelled_half_pane_preview_before_drop(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        use gpui::{Modifiers, MouseButton, point, px};
        redraw(cx);
        assert!(cx.debug_bounds("split-preview-right").is_none());
        let tab = cx.debug_bounds("document-tab-0").expect("标签");
        let edge = cx.debug_bounds("split-drop-1").expect("右侧落点");
        let start = point(tab.left() + px(30.0), tab.center().y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start + point(px(15.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(edge.center(), MouseButton::Left, Modifiers::none());
        redraw(cx);
        let preview = cx
            .debug_bounds("split-preview-right")
            .expect("松手前应显示向右分屏的落区预览");
        assert_eq!(preview.right(), edge.right());
        assert!(
            preview.size.width > edge.size.width,
            "高亮应覆盖新分屏的一半正文"
        );
        assert!(
            cx.debug_bounds("split-preview-label").is_some(),
            "应有文字说明落点行为"
        );
        cx.simulate_mouse_up(edge.center(), MouseButton::Left, Modifiers::none());
        redraw(cx);
        assert!(
            cx.debug_bounds("split-preview-right").is_none(),
            "松手后移除预览"
        );
        drop(editor);
    });
}

#[gpui::test]
fn single_editor_group_hides_tab_color_after_splits_collapse(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        redraw(cx);
        let initial_marker =
            Box::leak(format!("tab-group-left-border-{:?}-0", editor.entity_id()).into_boxed_str());
        assert!(
            cx.debug_bounds(initial_marker).is_none(),
            "未分组时不应有组颜色"
        );

        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 2);
        let left = groups.first().expect("左组").clone();
        let right = groups.get(1).expect("右组").clone();
        let left_marker =
            Box::leak(format!("tab-group-left-border-{:?}-0", left.entity_id()).into_boxed_str());
        let right_marker =
            Box::leak(format!("tab-group-left-border-{:?}-0", right.entity_id()).into_boxed_str());
        assert!(cx.debug_bounds(left_marker).is_some(), "多组时保留组颜色");
        assert!(cx.debug_bounds(right_marker).is_some(), "每组都应有组颜色");

        editor.update_in(cx, |editor, window, cx| {
            editor.transfer_split_tab(
                right.downgrade(),
                left.entity_id(),
                path.clone(),
                None,
                false,
                window,
                cx,
            );
        });
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            1
        );
        let remaining_tab =
            Box::leak(format!("group-tab-{:?}-0", left.entity_id()).into_boxed_str());
        assert!(cx.debug_bounds(remaining_tab).is_some(), "剩余标签仍然显示");
        assert!(
            cx.debug_bounds(left_marker).is_none(),
            "只剩一组时必须移除组颜色"
        );
    });
}

#[gpui::test]
fn split_groups_give_every_tab_a_matching_left_color_border(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        let other = path.with_file_name("second.md");
        std::fs::write(&other, "第二篇\n").expect("第二个标签");
        editor.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(other, window, cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        redraw(cx);
        assert!(
            cx.debug_bounds("pane-group-marker-0").is_none(),
            "正文不加分组标记"
        );
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        for group in &groups {
            let count = group.read_with(cx, |editor, _| editor.workspace.open_documents.len());
            for index in 0..count {
                let tab = cx
                    .debug_bounds(Box::leak(
                        format!("group-tab-{:?}-{index}", group.entity_id()).into_boxed_str(),
                    ))
                    .expect("分组标签");
                let border = cx
                    .debug_bounds(Box::leak(
                        format!("tab-group-left-border-{:?}-{index}", group.entity_id())
                            .into_boxed_str(),
                    ))
                    .expect("同组每个标签都应显示组色左边框");
                assert_eq!(border.left(), tab.left());
                assert_eq!(
                    border.center().y,
                    tab.center().y,
                    "组色线在标签左侧垂直居中"
                );
                assert!(
                    border.top() > tab.top() && border.bottom() < tab.bottom(),
                    "组色线不应贯穿标签高度"
                );
                assert_eq!(border.size.width, gpui::px(2.0));
                assert_eq!(border.size.height, gpui::px(18.0));
                assert!(
                    cx.debug_bounds(Box::leak(
                        format!("tab-group-swatch-{:?}-{index}", group.entity_id())
                            .into_boxed_str()
                    ))
                    .is_none(),
                    "去掉组首独立色块"
                );
            }
        }
    });
}

#[gpui::test]
fn split_groups_separate_only_group_boundaries_with_spacing_and_no_divider(
    cx: &mut TestAppContext,
) {
    fixture(cx, |editor, cx, path| {
        let other = path.with_file_name("second.md");
        std::fs::write(&other, "第二篇\n").expect("第二个标签");
        editor.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(other, window, cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let first = groups.first().expect("第一组");
        let second = groups.get(1).expect("第二组");
        let first_tab = cx
            .debug_bounds(Box::leak(
                format!("group-tab-{:?}-0", first.entity_id()).into_boxed_str(),
            ))
            .expect("组内首标签");
        let last = cx
            .debug_bounds(Box::leak(
                format!("group-tab-{:?}-1", first.entity_id()).into_boxed_str(),
            ))
            .expect("组内末标签");
        let next = cx
            .debug_bounds(Box::leak(
                format!("group-tab-{:?}-0", second.entity_id()).into_boxed_str(),
            ))
            .expect("下一组首标签");
        assert_eq!(first_tab.right(), last.left(), "同组标签紧挨排列");
        let gap = cx
            .debug_bounds("tab-group-gap-1")
            .expect("不同组之间应留少量间距");
        assert_eq!(gap.size.width, gpui::px(12.0));
        assert_eq!(gap.left(), last.right());
        assert_eq!(gap.right(), next.left());
        assert!(
            cx.debug_bounds("tab-group-separator-1").is_none(),
            "组间只留空隙，不再画分隔符"
        );
    });
}

#[gpui::test]
fn split_real_markdown_keeps_measured_text_when_three_panes_reflow(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        cx.simulate_resize(gpui::size(gpui::px(1800.0), gpui::px(900.0)));
        redraw(cx);
        let markdown = include_str!("../../../../docs/architecture/markdown-model.md");
        std::fs::write(&path, markdown).expect("真实文档夹具");
        editor.update_in(cx, |editor, window, cx| {
            editor.reload_conflicted_document(&path, cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        for _ in 0..5 {
            redraw(cx);
        }
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        assert_eq!(groups.len(), 3, "足够宽的窗口应保留三个分屏");
        for group in groups {
            group.read_with(cx, |editor, cx| {
                for block in editor.document.visible_blocks() {
                    let block = block.entity.read(cx);
                    if block.last_bounds.is_some() && !block.display_text().is_empty() {
                        assert!(
                            block
                                .last_layout
                                .as_ref()
                                .is_some_and(|lines| !lines.is_empty()),
                            "已挂载的 {:?} 文本不能只留空壳：{}",
                            block.kind(),
                            block.display_text()
                        );
                    }
                }
            });
        }
    });
}

#[gpui::test]
fn splitting_refuses_to_make_panes_narrower_than_the_reading_minimum(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        cx.simulate_resize(gpui::size(gpui::px(1000.0), gpui::px(800.0)));
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            2,
            "半宽不足以容纳两个 320px 正文时应拒绝再次左右拆分"
        );
    });
}

#[gpui::test]
fn split_minimum_also_applies_before_the_new_group_has_drawn(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        cx.simulate_resize(gpui::size(gpui::px(1000.0), gpui::px(800.0)));
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            2,
            "连续分屏也必须受最小尺寸保护，不能依赖下一帧才有的边界"
        );
    });
}

#[gpui::test]
fn window_split_commands_split_the_active_pane_instead_of_the_first(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        cx.simulate_resize(gpui::size(gpui::px(1800.0), gpui::px(900.0)));
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let left_before = cx.debug_bounds("editor-group-0").expect("原左组");
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let left_after = cx.debug_bounds("editor-group-0").expect("原左组");
        assert_eq!(
            left_after.size.width, left_before.size.width,
            "在活动右组再次向右分屏，原左组宽度不应被切半"
        );
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            3
        );
    });
}

#[gpui::test]
fn dragging_the_divider_cannot_squeeze_a_pane_below_the_minimum(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        use gpui::{Modifiers, MouseButton, point, px};
        cx.simulate_resize(gpui::size(px(1000.0), px(800.0)));
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let divider = cx.debug_bounds("split-divider").expect("分隔条");
        let left = cx.debug_bounds("editor-group-0").expect("左组");
        let start = divider.center();
        let end = point(left.left() + px(5.0), start.y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start - point(px(15.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        redraw(cx);
        let left = cx.debug_bounds("editor-group-0").expect("左组");
        assert!(
            left.size.width >= px(319.5),
            "拖动分隔条也必须保住 320px 最小正文宽度，当前 {:?}",
            left.size.width
        );
    });
}

#[gpui::test]
fn dropping_on_a_pane_that_is_too_small_just_cancels_the_split(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        cx.simulate_resize(gpui::size(gpui::px(1000.0), gpui::px(800.0)));
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let source = groups.first().expect("来源组").downgrade();
        groups
            .get(1)
            .expect("窄落点组")
            .update_in(cx, |editor, window, cx| {
                for source in [Some(source), None] {
                    editor.receive_editor_file_drop(
                        &super::EditorFileDrag {
                            from_path: path.clone(),
                            source,
                        },
                        None,
                        Some(SplitDirection::Right),
                        window,
                        cx,
                    );
                }
            });
        cx.run_until_parked();
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            2
        );
        assert!(
            !editor.read_with(cx, |editor, _| editor.modal_is_open()),
            "落区已经提示空间不足，松手只需取消拆分，不应再用模态打断阅读"
        );
    });
}

#[gpui::test]
fn vertical_split_keeps_the_lower_documents_text_column_visible(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        cx.simulate_resize(gpui::size(gpui::px(1400.0), gpui::px(1100.0)));
        std::fs::write(
            &path,
            include_str!("../../../../docs/architecture/overview.md"),
        )
        .expect("上方真实文档");
        editor.update_in(cx, |editor, _, cx| {
            editor.reload_conflicted_document(&path, cx)
        });
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let left = editor
            .read_with(cx, |editor, _| editor.split_groups())
            .first()
            .expect("左列")
            .clone();
        editor.update(cx, |editor, cx| {
            editor.activate_split_group(left.entity_id(), cx)
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Down, window, cx)
        });
        cx.run_until_parked();
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let lower = groups.get(1).expect("下方分屏");
        let second = path.with_file_name("editor-core.md");
        std::fs::write(
            &second,
            include_str!("../../../../docs/architecture/editor-core.md"),
        )
        .expect("下方真实文档");
        lower.update_in(cx, |editor, window, cx| {
            editor.open_workspace_file(second, window, cx)
        });
        for _ in 0..5 {
            redraw(cx);
        }
        let bounds = cx.debug_bounds("editor-group-1").expect("下组边界");
        let above = cx.debug_bounds("editor-group-0").expect("上组边界");
        let beside = cx.debug_bounds("editor-group-2").expect("右组边界");
        assert!(
            bounds.size.width >= gpui::px(320.0),
            "下组宽度不能由内容挤成零"
        );
        assert_eq!(bounds.size.width, above.size.width);
        assert!(
            bounds.top() >= above.bottom() && bounds.bottom() <= beside.bottom(),
            "上下组不能互相溢出"
        );
        lower.read_with(cx, |editor, cx| {
            let first = editor.document.first_root().expect("下组标题").read(cx);
            let text = first.last_bounds.expect("下组标题应排版");
            assert!(
                text.size.width > bounds.size.width * 0.5,
                "上下分屏不能把下方正文列挤成窄条：{text:?}"
            );
            assert!(
                first
                    .last_layout
                    .as_ref()
                    .is_some_and(|lines| !lines.is_empty()),
                "下方正文不能只有外壳"
            );
        });
    });
}

#[gpui::test]
fn opening_many_files_after_split_reveals_the_new_tab_without_squeezing_groups(
    cx: &mut TestAppContext,
) {
    fixture(cx, |editor, cx, path| {
        cx.simulate_resize(gpui::size(gpui::px(1400.0), gpui::px(900.0)));
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let active = editor
            .read_with(cx, |editor, _| editor.active_split_editor())
            .expect("活动组");
        for index in 0..12 {
            let file = path.with_file_name(format!("新文件-{index}-long-document-name.md"));
            std::fs::write(&file, "正文\n").expect("新文件");
            active.update_in(cx, |editor, window, cx| {
                editor.open_workspace_file(file, window, cx)
            });
        }
        redraw(cx);
        let bar = cx.debug_bounds("window-editor-tabs").expect("统一标签栏");
        let latest = cx.debug_bounds("document-tab-12").expect("新标签");
        assert!(
            latest.left() >= bar.left() && latest.right() <= bar.right(),
            "打开新文件应完整显示新标签和关闭按钮，不能被组宽截断：tab={latest:?}, bar={bar:?}"
        );
        assert!(
            latest.size.width >= gpui::px(120.0),
            "组不能挤压标签正常宽度"
        );
        editor.update(cx, |editor, cx| {
            editor
                .workspace
                .tabs_scroll_handle
                .set_offset(gpui::point(gpui::px(0.0), gpui::px(0.0)));
            cx.notify();
        });
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor
                .workspace
                .tabs_scroll_handle
                .offset()
                .x),
            gpui::px(0.0),
            "用户滚回旧标签后，普通重绘不能把滚动位置抢回新标签"
        );
    });
}

#[test]
fn tab_group_accents_have_distinct_colors_in_light_and_dark_themes() {
    for theme in [
        crate::theme::Theme::default_theme(),
        crate::theme::Theme::light_theme(),
        crate::theme::Theme::paper_theme(),
        crate::theme::Theme::forest_theme(),
        crate::theme::Theme::midnight_theme(),
        crate::theme::Theme::ink_theme(),
    ] {
        let mut accents = Vec::new();
        for index in 0..6 {
            let accent = super::render::tab_group_accent(&theme, index);
            assert!(
                !accents.contains(&accent),
                "{} 的分组左边框应使用不同颜色",
                theme.name
            );
            assert!(accent.s >= 0.4, "{} 的左边框应能明确区分颜色", theme.name);
            assert_eq!(accent.a, 1.0);
            accents.push(accent);
        }
    }
}

#[gpui::test]
fn split_history_restore_refreshes_every_view_and_keeps_shared_undo(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, path| {
        crate::config::record_file_history(&path, "历史正文\n").expect("历史版本");
        editor.update_in(cx, |editor, window, cx| editor.split_active_document(SplitDirection::Right, window, cx));
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let left = groups.first().expect("左组"); let right = groups.get(1).expect("右组");
        right.update(cx, |editor, cx| { editor.open_file_history(cx); editor.restore_file_history_version(0, cx); });
        right.update_in(cx, |editor, window, cx| editor.dismiss_modal(0, window, cx));
        redraw(cx);
        for group in &groups { group.read_with(cx, |editor, cx| {
            assert_eq!(editor.buffer.text(), "历史正文\n");
            assert_eq!(editor.document.markdown_text(cx), "历史正文");
        }); }
        editor.update(cx, |editor, cx| editor.activate_split_group(left.entity_id(), cx));
        left.update(cx, |editor, cx| editor.undo_document(cx));
        redraw(cx);
        for group in &groups { assert_eq!(group.read_with(cx, |editor, _| editor.buffer.text()), "正文\n"); }
        let versions = crate::config::file_history::list_versions(&path, Default::default(), std::time::SystemTime::now()).expect("历史");
        assert!(versions.iter().any(|version| version.source == crate::config::file_history::HistorySource::BeforeRestore));
    });
}

#[gpui::test]
fn native_group_tab_keyboard_activation_selects_its_editor(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    fixture(cx, |editor, cx, _| {
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        redraw(cx);
        let groups = editor.read_with(cx, |editor, _| editor.split_groups());
        let first = groups.first().unwrap().clone();
        let focus = first.read_with(cx, |editor, _| editor.workspace.tabs_focus.clone().unwrap());
        assert_ne!(
            editor.read_with(cx, |editor, _| editor
                .active_split_editor()
                .unwrap()
                .entity_id()),
            first.entity_id()
        );
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&focus, cx);
            window.press("left", cx);
        });
        redraw(cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor
                .active_split_editor()
                .unwrap()
                .entity_id()),
            first.entity_id()
        );
        assert!(cx.update(|window, _| focus.is_focused(window)));
        for group in groups {
            group.read_with(cx, |editor, _| assert_eq!(editor.buffer.text(), "正文\n"));
        }
    });
}

#[gpui::test]
fn native_split_dividers_support_keyboard_and_keep_session_ratios(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    for (direction, vertical, minimum, positive) in [
        (SplitDirection::Right, false, gpui::px(320.0), "right"),
        (SplitDirection::Down, true, gpui::px(180.0), "down"),
    ] {
        let root =
            std::env::temp_dir().join(format!("velora-native-split-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let _config = crate::config::override_test_config_root(root.join("config"));
        let path = root.join("notes.md");
        std::fs::write(&path, "正文\n").unwrap();
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let handle = cx.update(|cx| {
            crate::ui::open_window(Default::default(), cx, |_, cx| {
                cx.new(|cx| Editor::from_markdown(cx, "正文\n".into(), Some(path.clone())))
            })
            .unwrap()
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.simulate_resize(gpui::size(gpui::px(1100.0), gpui::px(800.0)));
        visual.run_until_parked();
        handle
            .update(&mut visual, |editor, window, cx| {
                window.activate_window();
                editor.split_active_document(direction, window, cx);
            })
            .unwrap();
        for _ in 0..4 {
            redraw(&mut visual);
        }
        let first_before = visual.debug_bounds("editor-group-0").unwrap();
        visual.update(|window, cx| {
            assert_eq!(
                window
                    .try_find("split-divider")
                    .and_then(|item| item.role()),
                Some(gpui::Role::Splitter),
                "分屏边界必须使用可访问的标准分隔控件"
            );
            window.blur(cx);
            window.focus_next(cx);
            for _ in 0..48 {
                window.press("tab", cx);
                window.render_frame(cx);
                if window.find("split-divider").focused() == Some(true) {
                    break;
                }
            }
            assert_eq!(window.find("split-divider").focused(), Some(true));
            window.press(positive, cx);
        });
        for _ in 0..3 {
            redraw(&mut visual);
        }
        let first = visual.debug_bounds("editor-group-0").unwrap();
        let extent = |bounds: gpui::Bounds<gpui::Pixels>| {
            if vertical {
                bounds.size.height
            } else {
                bounds.size.width
            }
        };
        assert!(
            extent(first) > extent(first_before) + gpui::px(8.0),
            "方向键应扩展前一个编辑区"
        );
        visual.update(|window, cx| window.press("home", cx));
        for _ in 0..3 {
            redraw(&mut visual);
        }
        let first = visual.debug_bounds("editor-group-0").unwrap();
        assert!(
            (extent(first) - minimum).abs() < gpui::px(1.0),
            "Home 应到达正文最小尺寸：{first:?}"
        );
        visual.update(|window, cx| window.press("end", cx));
        for _ in 0..3 {
            redraw(&mut visual);
        }
        let first = visual.debug_bounds("editor-group-0").unwrap();
        let second = visual.debug_bounds("editor-group-1").unwrap();
        assert!(
            (extent(second) - minimum).abs() < gpui::px(1.0),
            "End 仍需保住另一个编辑区的最小尺寸"
        );
        handle
            .read_with(&visual, |editor, cx| {
                let crate::config::EditorGroupLayout::Split { ratio, .. } =
                    editor.split_session_layout(cx).unwrap()
                else {
                    panic!("分屏布局");
                };
                let expected = f32::from(extent(first)) / f32::from(extent(first) + extent(second));
                assert!(
                    (f32::from(ratio) / 1000.0 - expected).abs() < 0.002,
                    "键盘调整必须同步会话比例"
                );
                for group in editor.split_groups() {
                    assert_eq!(group.read(cx).buffer.text(), "正文\n");
                }
            })
            .unwrap();
        cx.on_quit(move || std::fs::remove_dir_all(root).unwrap());
    }
}

#[gpui::test]
fn native_nested_splits_fit_a_narrow_window_and_restore_their_proportions(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        cx.simulate_resize(gpui::size(gpui::px(1800.0), gpui::px(900.0)));
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx);
            editor.split_active_document(SplitDirection::Right, window, cx);
        });
        for _ in 0..6 {
            redraw(cx);
        }
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.split_groups().len()),
            3
        );
        let before = (0..3)
            .map(|index| {
                let selector = Box::leak(format!("editor-group-{index}").into_boxed_str());
                cx.debug_bounds(selector).unwrap()
            })
            .collect::<Vec<_>>();
        let focus = editor.read_with(cx, |editor, _| {
            let super::GroupNode::Split { view, .. } =
                &editor.split_workspace.as_ref().unwrap().layout
            else {
                panic!("分屏根");
            };
            view.borrow().as_ref().unwrap().focus.clone()
        });
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.simulate_resize(gpui::size(gpui::px(1000.0), gpui::px(700.0)));
        for _ in 0..6 {
            redraw(cx);
        }
        let current = (0..3)
            .map(|index| {
                let selector = Box::leak(format!("editor-group-{index}").into_boxed_str());
                cx.debug_bounds(selector).unwrap()
            })
            .collect::<Vec<_>>();
        for pane in &current {
            assert!(pane.size.width > gpui::px(0.0));
            assert!(
                pane.left() >= gpui::px(0.0) && pane.right() <= gpui::px(1000.5),
                "缩窄后不能把正文挤出窗口：{pane:?}"
            );
        }
        for pair in current.windows(2) {
            assert!(
                (pair[0].right() - pair[1].left()).abs() < gpui::px(1.0),
                "相邻正文不能重叠或遗留厚分隔空隙"
            );
        }
        assert!(
            cx.update(|window, _| focus.is_focused(window)),
            "重建尺寸状态不能丢失分隔条焦点"
        );
        cx.simulate_resize(gpui::size(gpui::px(1800.0), gpui::px(900.0)));
        for _ in 0..6 {
            redraw(cx);
        }
        for (index, previous) in before.into_iter().enumerate() {
            let selector = Box::leak(format!("editor-group-{index}").into_boxed_str());
            let current = cx.debug_bounds(selector).unwrap();
            assert!(
                (current.size.width - previous.size.width).abs() < gpui::px(1.0),
                "窗口恢复后应保留原比例：{current:?} / {previous:?}"
            );
        }
    });
}

#[gpui::test]
fn split_session_keeps_extreme_ratios_reached_by_the_native_controls(cx: &mut TestAppContext) {
    fixture(cx, |editor, cx, _| {
        cx.simulate_resize(gpui::size(gpui::px(3200.0), gpui::px(900.0)));
        redraw(cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        });
        for _ in 0..4 {
            redraw(cx);
        }
        let state = editor.read_with(cx, |editor, _| {
            let super::GroupNode::Split { view, .. } =
                &editor.split_workspace.as_ref().unwrap().layout
            else {
                panic!("分屏根");
            };
            view.borrow().as_ref().unwrap().state.clone()
        });
        let mut comparisons = Vec::new();
        for requested in [gpui::px(320.0), gpui::Pixels::MAX] {
            cx.update(|window, cx| {
                state.update(cx, |state, cx| state.resize_panel(0, requested, window, cx))
            });
            for _ in 0..3 {
                redraw(cx);
            }
            let layout =
                editor.read_with(cx, |editor, cx| editor.split_session_layout(cx).unwrap());
            let crate::config::EditorGroupLayout::Split {
                ratio: expected, ..
            } = &layout
            else {
                panic!("已保存的分屏布局");
            };
            assert!(
                *expected < 150 || *expected > 850,
                "夹具必须覆盖原先恢复范围之外的合法布局"
            );
            let encoded = serde_json::to_string(&layout).unwrap();
            let layout = serde_json::from_str(&encoded).unwrap();
            let restored = cx
                .cx
                .new(|cx| Editor::from_markdown(cx, String::new(), None));
            cx.update(|window, cx| {
                restored.update(cx, |editor, cx| {
                    editor.restore_split_session(&layout, window, cx)
                })
            });
            let actual = restored.read_with(cx, |editor, cx| {
                let crate::config::EditorGroupLayout::Split { ratio, .. } =
                    editor.split_session_layout(cx).unwrap()
                else {
                    panic!("已恢复的分屏布局");
                };
                for group in editor.split_groups() {
                    assert_eq!(group.read(cx).buffer.text(), "正文\n");
                }
                ratio
            });
            comparisons.push((*expected, actual));
        }
        assert!(
            comparisons
                .iter()
                .all(|(expected, actual)| expected == actual),
            "恢复不能把控件产生的比例改成 15% 或 85%：{comparisons:?}"
        );
    });
}

#[gpui::test]
fn split_shell_keeps_the_configured_interface_font_geometry(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!("velora-split-ui-font-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let _config = crate::config::override_test_config_root(root.join("profile"));
    let config = crate::config::VeloraConfigDirs::from_system()
        .unwrap()
        .app_config_file();
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(config, "[editor]\nui_font_size = 21\n").unwrap();
    let path = root.join("notes.md");
    std::fs::write(&path, "正文\n").unwrap();
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::config::EditorSettings::init(cx, true);
        crate::components::init(cx);
    });
    let handle = cx.update(|cx| {
        crate::ui::open_window(Default::default(), cx, |_, cx| {
            cx.new(|cx| Editor::from_markdown(cx, "正文\n".into(), Some(path.clone())))
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.simulate_resize(gpui::size(gpui::px(1400.0), gpui::px(800.0)));
    for _ in 0..3 {
        redraw(&mut visual);
    }
    let before = visual
        .debug_bounds("window-titlebar-drag-filler")
        .expect("普通窗口标题栏");
    assert!(
        before.size.height >= gpui::px(47.0),
        "夹具必须使用放大的界面字号"
    );
    handle
        .update(&mut visual, |editor, window, cx| {
            editor.split_active_document(SplitDirection::Right, window, cx)
        })
        .unwrap();
    for _ in 0..4 {
        redraw(&mut visual);
    }
    let after = visual
        .debug_bounds("window-titlebar-drag-filler")
        .expect("分屏标题栏");
    let reading = visual
        .debug_bounds("status-bar-reading-mode")
        .expect("状态栏阅读按钮");
    assert!(
        after.size.height == before.size.height && reading.bottom() <= gpui::px(800.0),
        "分屏不能丢掉界面尺寸派生：标题栏 {:?}→{:?}，状态栏按钮 {reading:?}",
        before.size.height,
        after.size.height
    );
    handle
        .read_with(&visual, |editor, cx| {
            let groups = editor.split_groups();
            assert_eq!(groups.len(), 2);
            for group in groups {
                assert_eq!(group.read(cx).buffer.text(), "正文\n");
            }
        })
        .unwrap();
    cx.on_quit(move || std::fs::remove_dir_all(root).unwrap());
}
