//! workspace 模块测试：按主题拆分，共享导入在每个文件头部。

mod document_find;
mod document_matches;
mod external_changes;
mod save_external;
mod search;
mod search_cross_block;
mod search_engine;
mod search_matcher_snapshot;
mod search_perf;
mod search_random_consistency;
mod sidebar;
mod outline_fences;
mod tabs_sessions;
mod tabs_state;
mod untitled;
mod tree_files;
mod tree_edit;
mod workspace_scan;

fn workspace_menu_action_bounds(
    cx: &mut gpui::VisualTestContext,
    action: super::WorkspaceMenuAction,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    cx.update(|window, cx| {
        let label = action.label(cx.global::<crate::i18n::I18nManager>().strings());
        gpui_kit::base::test_support::snapshots(window)
            .into_iter()
            .find(|item| {
                item.visible()
                    && item.role() == Some(gpui::Role::MenuItem)
                    && item.label() == Some(label)
            })
            .map(|item| item.bounds())
    })
}

fn menu_label_bounds(
    cx: &mut gpui::VisualTestContext,
    label: &str,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    cx.update(|window, _| {
        gpui_kit::base::test_support::snapshots(window)
            .into_iter()
            .find(|item| {
                item.visible()
                    && item.role() == Some(gpui::Role::MenuItem)
                    && item.label() == Some(label)
            })
            .map(|item| item.bounds())
    })
}
