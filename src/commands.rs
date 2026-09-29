//! 命令注册表（roadmap H5）：菜单与命令面板共用同一份命令清单。
//!
//! 每条命令只登记一次——稳定 id、i18n 文案、动作构造器、所属菜单、分隔位置。
//! 菜单按所属分组消费注册表，命令面板列出全部条目，因此不会再出现
//! 「菜单里有、面板里没有」或反向的漂移；新增命令改这一处即可。
//!
//! 派发仍由 `crate::app_menu::dispatch_menu_action` 按动作类型负责，
//! `every_registered_command_has_a_dispatch_branch` 用例扫描该函数的源码，
//! 保证注册表里的每条命令都有对应分支（否则面板/菜单点了没反应）。

use gpui::Action;

use crate::components::{
    CloseWindow, CopyAsHtml, ExportHtml, ExportPdf, ExportPng, FindInDocument, FindNextMatch,
    FindPreviousMatch, NewWindow, OpenCommandPalette, OpenFile, OpenFolder, OpenPreferences, PrintDocument,
    QuitApplication, SaveDocument, SaveDocumentAs, ShowAbout, ToggleFocusMode, ToggleFullscreen,
    ToggleSidebar, ToggleTypewriterMode, ToggleViewMode, ZoomIn, ZoomOut, ZoomReset,
};
use crate::i18n::I18nStrings;

/// 命令所属的菜单。`App` 只在 macOS 出现（应用菜单），非 macOS 并入文件菜单。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandMenu {
    App,
    File,
    Export,
    View,
    Help,
}

/// 一条命令的静态声明。
pub(crate) struct CommandSpec {
    /// 稳定标识：用于查重、测试与后续扩展（快捷键自定义、插件注册）。
    pub(crate) id: &'static str,
    pub(crate) menu: CommandMenu,
    /// 在菜单中该条目之前插入分隔线（首个条目忽略）。
    pub(crate) separator_before: bool,
    label: fn(&I18nStrings) -> String,
    action: fn() -> Box<dyn Action>,
}

impl CommandSpec {
    /// 本地化文案。
    pub(crate) fn label(&self, strings: &I18nStrings) -> String {
        (self.label)(strings)
    }

    /// 构造该命令的动作。
    pub(crate) fn boxed_action(&self) -> Box<dyn Action> {
        (self.action)()
    }
}

/// 登记一条命令；`sep` 形式在其前面插分隔线。
macro_rules! command {
    ($id:literal, $menu:ident, $label:ident, $action:ident) => {
        command!(@entry $id, $menu, false, $label, $action)
    };
    (sep $id:literal, $menu:ident, $label:ident, $action:ident) => {
        command!(@entry $id, $menu, true, $label, $action)
    };
    (@entry $id:literal, $menu:ident, $sep:expr, $label:ident, $action:ident) => {
        CommandSpec {
            id: $id,
            menu: CommandMenu::$menu,
            separator_before: $sep,
            label: |strings: &I18nStrings| strings.$label.clone(),
            action: || Box::new($action),
        }
    };
}

static COMMANDS: &[CommandSpec] = &[
    // 应用菜单（macOS）/ 文件菜单末尾（其他平台）
    command!("preferences", App, menu_preferences, OpenPreferences),
    command!(sep "quit", App, menu_quit, QuitApplication),
    // 文件
    command!("new_window", File, menu_new_window, NewWindow),
    command!("close_window", File, menu_close_window, CloseWindow),
    command!("open_file", File, menu_open_file, OpenFile),
    command!("open_folder", File, menu_open_folder, OpenFolder),
    command!(sep "save", File, menu_save, SaveDocument),
    command!("save_as", File, menu_save_as, SaveDocumentAs),
    // 导出
    command!("export_html", Export, menu_export_html, ExportHtml),
    command!("export_pdf", Export, menu_export_pdf, ExportPdf),
    command!("export_png", Export, menu_export_png, ExportPng),
    command!("print", Export, menu_print, PrintDocument),
    command!("copy_as_html", Export, menu_copy_as_html, CopyAsHtml),
    // 视图
    command!("toggle_sidebar", View, command_toggle_sidebar, ToggleSidebar),
    command!(
        "toggle_fullscreen",
        View,
        preferences_shortcut_toggle_fullscreen,
        ToggleFullscreen
    ),
    command!(
        sep "toggle_view_mode",
        View,
        command_toggle_view_mode,
        ToggleViewMode
    ),
    command!(
        "toggle_focus_mode",
        View,
        command_toggle_focus_mode,
        ToggleFocusMode
    ),
    command!(
        "toggle_typewriter_mode",
        View,
        command_toggle_typewriter_mode,
        ToggleTypewriterMode
    ),
    command!(
        sep "open_command_palette",
        View,
        menu_open_command_palette,
        OpenCommandPalette
    ),
    command!(
        "find_in_document",
        View,
        command_find_in_document,
        FindInDocument
    ),
    command!("find_next", View, command_find_next, FindNextMatch),
    command!(
        "find_previous",
        View,
        command_find_previous,
        FindPreviousMatch
    ),
    command!(sep "zoom_in", View, command_zoom_in, ZoomIn),
    command!("zoom_out", View, command_zoom_out, ZoomOut),
    command!("zoom_reset", View, command_zoom_reset, ZoomReset),
    // 帮助
    command!("show_about", Help, menu_about, ShowAbout),
];

/// 全部注册命令，按菜单与其内顺序排列。
pub(crate) fn commands() -> &'static [CommandSpec] {
    COMMANDS
}

/// 某个菜单下的命令。
pub(crate) fn commands_for(menu: CommandMenu) -> impl Iterator<Item = &'static CommandSpec> {
    commands().iter().filter(move |spec| spec.menu == menu)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{CommandMenu, commands};
    use crate::i18n::I18nManager;

    #[test]
    fn registry_ids_are_unique_and_labels_resolve() {
        let mut ids = HashSet::new();
        let strings = I18nManager::default().strings().clone();

        for spec in commands() {
            assert!(ids.insert(spec.id), "重复的命令 id：{}", spec.id);
            assert!(
                !spec.label(&strings).trim().is_empty(),
                "命令 {} 的文案为空",
                spec.id
            );
        }
    }

    #[test]
    fn registry_covers_every_menu_section_in_order() {
        let view_ids = super::commands_for(CommandMenu::View)
            .map(|spec| spec.id)
            .collect::<Vec<_>>();

        assert_eq!(
            view_ids,
            vec![
                "toggle_sidebar",
                "toggle_fullscreen",
                "toggle_view_mode",
                "toggle_focus_mode",
                "toggle_typewriter_mode",
                "open_command_palette",
                "find_in_document",
                "find_next",
                "find_previous",
                "zoom_in",
                "zoom_out",
                "zoom_reset",
            ]
        );

        let export_ids = super::commands_for(CommandMenu::Export)
            .map(|spec| spec.id)
            .collect::<Vec<_>>();

        assert_eq!(
            export_ids,
            vec![
                "export_html",
                "export_pdf",
                "export_png",
                "print",
                "copy_as_html",
            ]
        );
    }

    #[test]
    fn app_section_keeps_the_ids_non_macos_folds_into_the_file_menu() {
        // 非 macOS 没有应用菜单，app_menu 按 id 把这两条折进文件菜单；
        // id 是跨平台约定，改名会同时打断非 macOS 的菜单构建。
        let app_ids = super::commands_for(CommandMenu::App)
            .map(|spec| spec.id)
            .collect::<Vec<_>>();

        assert_eq!(app_ids, vec!["preferences", "quit"]);
    }
}
