use super::*;

/// 由命令注册表构造某个菜单的动作条目（含分隔线，roadmap H5）。
pub(super) fn command_menu_items(strings: &I18nStrings, menu: CommandMenu) -> Vec<MenuItem> {
    let mut items = Vec::new();
    for spec in crate::commands::commands_for(menu) {
        if spec.separator_before && !items.is_empty() {
            items.push(MenuItem::separator());
        }
        items.push(command_menu_item(strings, spec));
    }
    items
}

pub(super) fn command_menu_item(strings: &I18nStrings, spec: &CommandSpec) -> MenuItem {
    MenuItem::Action {
        name: spec.label(strings).into(),
        action: spec.boxed_action(),
        os_action: None,
    }
}

/// 按 id 取注册命令。
pub(crate) fn command_spec(id: &str) -> &'static CommandSpec {
    crate::commands::commands()
        .iter()
        .find(|spec| spec.id == id)
        .expect("command registry should keep the id")
}

/// 文件菜单把三个打开入口连在一起；非 macOS 的退出入口仍放在末尾。
pub(super) fn file_menu_items(strings: &I18nStrings, recent_items: Vec<MenuItem>, has_app_menu: bool) -> Vec<MenuItem> {
    let mut items = Vec::new();
    let mut recent_items = Some(recent_items);
    for spec in crate::commands::commands_for(CommandMenu::File) {
        if spec.separator_before && !items.is_empty() {
            items.push(MenuItem::separator());
        }
        items.push(command_menu_item(strings, spec));
        if spec.id == "open_folder" {
            if let Some(recent_items) = recent_items.take() {
                items.push(MenuItem::submenu(Menu {
                    name: strings.menu_open_recent_file.clone().into(),
                    items: recent_items,
                }));
            }
        }
    }
    if !has_app_menu {
        items.push(MenuItem::separator());
        items.push(command_menu_item(strings, command_spec("quit")));
    }
    items
}
