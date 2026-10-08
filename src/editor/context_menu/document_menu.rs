//! 正文右键菜单的内容：一行项对应一个已有的动作。
//!
//! 菜单不自己实现编辑行为：撤销、剪切这类把 gpui 动作原样派发出去，与键盘走同一条
//! 路径；行内格式与段落转换直接调编辑器层那两条入口（`toggle_inline_format_on_selection`、
//! `apply_block_kind_to_selection`），而那两条正是快捷键动作处理器所调的函数。
//! 一个动作只有一处实现，快捷键能做的菜单必定能做，反过来也一样。

use gpui::*;

use std::time::Duration;

use super::super::{ContextMenuState, Editor};
use crate::components::{
    Copy, CopyAsHtml, CopyAsMarkdown, Cut, InlineFormat, Paste, PasteAsPlainText, Redo,
    ShortcutCommand, ToggleViewMode, Undo, effective_shortcut_key,
    menu::{MENU_ICON_SIZE, MENU_ROW_GAP},
};
use crate::editor::insert_ops::InsertBlockTarget;
use crate::editor::paragraph_ops::BlockKindTarget;
use crate::theme::ThemeDimensions;

/// 指针离开菜单之后，二级面板还留着的时限。手快时滑出去一眼再滑回来，
/// 靠这一段兜住（任一个悬停区重新收到 hover 就撤销这次收起）。
/// 240ms：120ms 时手滑出去一眼面板就没了（用户报修），400ms 又嫌走得慢。
const SUBMENU_CLOSE_DELAY: Duration = Duration::from_millis(240);

/// 标题那一档的行 id，按下标取用（`level` 已经在 1..=6 内）。
const HEADING_ROW_NAMES: [&str; 6] = [
    "heading-1",
    "heading-2",
    "heading-3",
    "heading-4",
    "heading-5",
    "heading-6",
];

/// 二级菜单父行的文字。
pub(crate) fn document_submenu_label(
    submenu: DocumentSubmenu,
    strings: &crate::i18n::I18nStrings,
) -> String {
    match submenu {
        DocumentSubmenu::Format => strings.context_menu_format.clone(),
        DocumentSubmenu::Paragraph => strings.context_menu_paragraph.clone(),
        DocumentSubmenu::Insert => strings.context_menu_insert.clone(),
    }
}

/// 主面板一行的行首图标。渲染与量宽都读这一处，两边不会一处画了图标、另一处按纯文字算宽。
/// 二级面板里的行不给图标：那一档排的是「一级标题」「有序列表」这种条目名，图标只会把每行拉宽。
pub(crate) fn document_menu_command_icon(command: DocumentMenuCommand) -> Option<&'static str> {
    match command {
        DocumentMenuCommand::Undo => Some("icon/editor/undo.svg"),
        DocumentMenuCommand::Redo => Some("icon/editor/redo.svg"),
        DocumentMenuCommand::Cut => Some("icon/editor/cut.svg"),
        DocumentMenuCommand::Copy => Some("icon/editor/copy.svg"),
        DocumentMenuCommand::Paste => Some("icon/editor/paste.svg"),
        DocumentMenuCommand::PasteAsPlainText => Some("icon/editor/paste-plain.svg"),
        DocumentMenuCommand::CopyAsMarkdown => Some("icon/editor/copy-markdown.svg"),
        DocumentMenuCommand::CopyAsHtml => Some("icon/editor/copy-html.svg"),
        DocumentMenuCommand::ToggleSourceView => Some("icon/editor/toggle-source.svg"),
        DocumentMenuCommand::Format(_)
        | DocumentMenuCommand::Heading(_)
        | DocumentMenuCommand::NormalText
        | DocumentMenuCommand::BulletList
        | DocumentMenuCommand::NumberedList
        | DocumentMenuCommand::TaskList
        | DocumentMenuCommand::Quote
        | DocumentMenuCommand::CodeBlock
        | DocumentMenuCommand::Link
        | DocumentMenuCommand::ClearFormat
        | DocumentMenuCommand::InsertTable
        | DocumentMenuCommand::InsertImage
        | DocumentMenuCommand::InsertCodeBlock
        | DocumentMenuCommand::InsertMathBlock
        | DocumentMenuCommand::InsertSeparator
        | DocumentMenuCommand::InsertToc
        | DocumentMenuCommand::InsertFrontMatter => None,
    }
}

/// 三个二级菜单入口的行首图标。「段落」那一颗与选中工具栏共用同一份图形。
pub(crate) fn document_submenu_icon(submenu: DocumentSubmenu) -> Option<&'static str> {
    match submenu {
        DocumentSubmenu::Format => Some("icon/editor/format.svg"),
        DocumentSubmenu::Paragraph => Some("icon/editor/paragraph.svg"),
        DocumentSubmenu::Insert => Some("icon/editor/insert.svg"),
    }
}

/// 二级菜单的三个入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DocumentSubmenu {
    Format,
    Paragraph,
    Insert,
}

/// 菜单一行指向的动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DocumentMenuCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    /// 「粘贴为纯文本」：只用剪贴板的文本味道，不转 HTML、不把网址写成链接。
    PasteAsPlainText,
    /// 「复制为 Markdown」：选区的源码文本（无选区时整篇）进剪贴板。
    CopyAsMarkdown,
    /// 「复制为 HTML」：整篇的 HTML 进剪贴板，与导出那一条同一份内容。
    CopyAsHtml,
    Format(InlineFormat),
    Heading(u8),
    NormalText,
    BulletList,
    NumberedList,
    TaskList,
    Quote,
    CodeBlock,
    /// 「格式 → 链接」：把选中的那段包成 `[文字]()`。不是行内样式标记，
    /// 写法要成对补方括号与圆括号，走的也是另一条动作。
    Link,
    /// 「格式 → 清除格式」：剥掉选区里的行内样式记号，不动链接与块级记号。
    ClearFormat,
    InsertTable,
    /// 「插入 → 图片」：打开原生文件选择器，选中的图片走粘贴那条插入路径。
    InsertImage,
    InsertCodeBlock,
    InsertMathBlock,
    InsertSeparator,
    InsertToc,
    InsertFrontMatter,
    ToggleSourceView,
}

impl DocumentMenuCommand {
    /// 这一行的元素 id 兼测试选择器。菜单的行名只在这里定一次，
    /// 行渲染、置灰与用例点名的都是同一个名字。
    pub(crate) fn row_name(self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::Cut => "cut",
            Self::Copy => "copy",
            Self::Paste => "paste",
            Self::PasteAsPlainText => "paste-as-plain-text",
            Self::CopyAsMarkdown => "copy-as-markdown",
            Self::CopyAsHtml => "copy-as-html",
            Self::Format(InlineFormat::Bold) => "bold",
            Self::Format(InlineFormat::Italic) => "italic",
            Self::Format(InlineFormat::Underline) => "underline",
            Self::Format(InlineFormat::Strikethrough) => "strikethrough",
            Self::Format(InlineFormat::Code) => "code",
            Self::Format(InlineFormat::Highlight) => "highlight",
            Self::Format(InlineFormat::Superscript) => "superscript",
            Self::Format(InlineFormat::Subscript) => "subscript",
            Self::Heading(level) => HEADING_ROW_NAMES[(level as usize) - 1],
            Self::NormalText => "normal-text",
            Self::BulletList => "bullet-list",
            Self::NumberedList => "numbered-list",
            Self::TaskList => "task-list",
            Self::Quote => "quote",
            Self::CodeBlock => "code-block",
            Self::Link => "link",
            Self::ClearFormat => "clear-format",
            Self::InsertTable => "table",
            Self::InsertImage => "insert-image",
            Self::InsertCodeBlock => "insert-code-block",
            Self::InsertMathBlock => "insert-math-block",
            Self::InsertSeparator => "insert-separator",
            Self::InsertToc => "insert-toc",
            Self::InsertFrontMatter => "insert-front-matter",
            Self::ToggleSourceView => "toggle-source-view",
        }
    }

    /// 这一行指向的段落转换目标；格式与编辑那几行返回 None。
    /// 选中工具栏的档位列表用它把行数据映回自己的动作。
    pub(crate) fn as_block_target(self) -> Option<BlockKindTarget> {
        match self {
            Self::Heading(level) => Some(BlockKindTarget::Heading(level)),
            Self::NormalText => Some(BlockKindTarget::Paragraph),
            Self::BulletList => Some(BlockKindTarget::BulletList),
            Self::NumberedList => Some(BlockKindTarget::NumberedList),
            Self::TaskList => Some(BlockKindTarget::TaskList),
            Self::Quote => Some(BlockKindTarget::Quote),
            Self::CodeBlock => Some(BlockKindTarget::CodeBlock),
            Self::Format(_)
            | Self::Link
            | Self::ClearFormat
            | Self::Undo
            | Self::Redo
            | Self::Cut
            | Self::Copy
            | Self::Paste
            | Self::PasteAsPlainText
            | Self::CopyAsMarkdown
            | Self::CopyAsHtml
            | Self::InsertTable
            | Self::InsertImage
            | Self::InsertCodeBlock
            | Self::InsertMathBlock
            | Self::InsertSeparator
            | Self::InsertToc
            | Self::InsertFrontMatter
            | Self::ToggleSourceView => None,
        }
    }
}

/// 渲染用的一行：条目、二级菜单入口，或分隔线。
pub(crate) enum DocumentMenuRow {
    Item {
        command: DocumentMenuCommand,
        name: &'static str,
        enabled: bool,
    },
    Submenu {
        id: DocumentSubmenu,
        name: &'static str,
    },
    /// 顶部那一行纯图标：撤销/重做与剪切/复制/粘贴/粘贴为纯文本。六件事占一行，
    /// 标签与生效键位写在每颗的悬停说明里。
    QuickActions,
    Separator,
}

/// 图标条上的动作，按屏上从左到右的顺序。`QUICK_ACTION_GROUP_BREAK` 之前是一组
/// （撤销、重做），之后是一组（剪切、复制、粘贴、粘贴为纯文本），两组之间一条竖线。
pub(crate) const DOCUMENT_MENU_QUICK_ACTIONS: [DocumentMenuCommand; 6] = [
    DocumentMenuCommand::Undo,
    DocumentMenuCommand::Redo,
    DocumentMenuCommand::Cut,
    DocumentMenuCommand::Copy,
    DocumentMenuCommand::Paste,
    DocumentMenuCommand::PasteAsPlainText,
];
pub(super) const QUICK_ACTION_GROUP_BREAK: usize = 2;
/// 格子的边长与间隙，与选中工具栏同一份尺寸；竖线 1px。
pub(super) const QUICK_ACTION_BUTTON_SIZE: f32 = 26.0;
pub(super) const QUICK_ACTION_GAP: f32 = 2.0;
pub(super) const QUICK_ACTION_DIVIDER_WIDTH: f32 = 1.0;

/// 图标条整条占的宽度：六颗格子、一条竖线，七个子节点之间六段间隙。
pub(crate) fn quick_actions_width() -> f32 {
    DOCUMENT_MENU_QUICK_ACTIONS.len() as f32 * QUICK_ACTION_BUTTON_SIZE
        + QUICK_ACTION_DIVIDER_WIDTH
        + QUICK_ACTION_GAP * (DOCUMENT_MENU_QUICK_ACTIONS.len() as f32)
}

/// 图标条那一颗的悬停说明：标签 + 生效键位。这一行不给文字，键位就写在这里，
/// 与菜单行右侧那一列同源（都取 `document_menu_shortcut` 那一份生效绑定）。
pub(crate) fn quick_action_tooltip(
    command: DocumentMenuCommand,
    strings: &crate::i18n::I18nStrings,
    cx: &App,
) -> String {
    let label = document_menu_label(command, strings);
    match document_menu_shortcut(command, cx) {
        Some(shortcut) => format!("{label}  {shortcut}"),
        None => label,
    }
}

impl Editor {
    /// 当前是否有一段可选中的正文（跨块选区或块内选区）。剪切、复制与八种行内格式
    /// 共用这一条判定；不成立时菜单把条目置灰而不是藏起来。
    pub(crate) fn has_text_selection(&self, cx: &App) -> bool {
        if self.cross_block_selection.is_some() {
            return self.normalized_cross_block_selection(cx).is_some();
        }
        self.current_edit_target_from_state(cx)
            .map(|target| !target.read(cx).selected_range.is_empty())
            .unwrap_or(false)
    }

    /// 图标条上一颗点得动吗。这六条判定与此前那六行菜单项一字不差，只是换了一处放：
    /// 剪切看「有选区且写得动」，复制只看有选区，粘贴两条还要剪贴板里有东西，
    /// 撤销/重做看对应的历史。
    pub(crate) fn quick_action_is_available(&self, command: DocumentMenuCommand, cx: &App) -> bool {
        let selectable = self.has_text_selection(cx);
        let editable = self.writes_through_the_buffer();
        match command {
            DocumentMenuCommand::Undo => !self.undo_history.is_empty() && editable,
            DocumentMenuCommand::Redo => !self.redo_history.is_empty() && editable,
            DocumentMenuCommand::Cut => selectable && editable,
            DocumentMenuCommand::Copy => selectable,
            DocumentMenuCommand::Paste | DocumentMenuCommand::PasteAsPlainText => {
                editable && cx.read_from_clipboard().is_some()
            }
            _ => false,
        }
    }

    /// 主菜单的行序：顶部图标条 → 复制为 → 格式 / 段落 / 插入 → 视图。
    pub(crate) fn document_menu_rows(&self) -> Vec<DocumentMenuRow> {
        vec![
            DocumentMenuRow::QuickActions,
            DocumentMenuRow::Separator,
            DocumentMenuRow::Item {
                command: DocumentMenuCommand::CopyAsMarkdown,
                name: "copy-as-markdown",
                enabled: !self.document.root_blocks().is_empty(),
            },
            DocumentMenuRow::Item {
                command: DocumentMenuCommand::CopyAsHtml,
                name: "copy-as-html",
                enabled: !self.document.root_blocks().is_empty(),
            },
            DocumentMenuRow::Separator,
            DocumentMenuRow::Submenu {
                id: DocumentSubmenu::Format,
                name: "format",
            },
            DocumentMenuRow::Submenu {
                id: DocumentSubmenu::Paragraph,
                name: "paragraph",
            },
            DocumentMenuRow::Submenu {
                id: DocumentSubmenu::Insert,
                name: "insert",
            },
            DocumentMenuRow::Separator,
            DocumentMenuRow::Item {
                command: DocumentMenuCommand::ToggleSourceView,
                name: "toggle-source-view",
                enabled: true,
            },
        ]
    }

    /// 二级菜单的行。链接排在「格式」那一档（写法在行内，与八种行内样式同一条口径），
    /// 图片与其余几样给的是整块，排在「插入」那一档。
    pub(crate) fn document_submenu_rows(
        &self,
        submenu: DocumentSubmenu,
        cx: &App,
    ) -> Vec<DocumentMenuRow> {
        let selectable = self.has_text_selection(cx);
        match submenu {
            DocumentSubmenu::Format => {
                let item = |command: DocumentMenuCommand, enabled: bool| DocumentMenuRow::Item {
                    enabled,
                    name: command.row_name(),
                    command,
                };
                [
                    InlineFormat::Bold,
                    InlineFormat::Italic,
                    InlineFormat::Underline,
                    InlineFormat::Strikethrough,
                    InlineFormat::Code,
                    InlineFormat::Highlight,
                    InlineFormat::Superscript,
                    InlineFormat::Subscript,
                ]
                .into_iter()
                .map(|format| {
                    item(
                        DocumentMenuCommand::Format(format),
                        selectable && self.writes_through_the_buffer(),
                    )
                })
                // 分隔线之后两行：链接补 `[文字]()` 外壳、清除格式剥掉已有的样式记号。
                // 清除格式这一行的置灰多看一眼「有没有样式可剥」：只选中一串没样式的字时
                // 点它确实什么都不发生，灰着比亮着诚实（判定与动作同源，见 `clear_format_is_available`）。
                .chain([DocumentMenuRow::Separator])
                .chain(
                    [
                        (DocumentMenuCommand::Link, self.link_insert_is_available(cx)),
                        (
                            DocumentMenuCommand::ClearFormat,
                            self.clear_format_is_available(cx),
                        ),
                    ]
                    .into_iter()
                    .map(|(command, enabled)| item(command, enabled)),
                )
                .collect()
            }
            DocumentSubmenu::Paragraph => {
                let item = |command: DocumentMenuCommand| DocumentMenuRow::Item {
                    enabled: self.block_kind_target_is_available(
                        command.as_block_target().expect("这一档全是段落转换"),
                        cx,
                    ),
                    name: command.row_name(),
                    command,
                };
                // 分节只是把「标题—正文」与「列表」两族隔开；行数据仍是同一份，
                // 三个入口（快捷键、这里、工具栏的下拉）拿到的顺序一致。
                [
                    DocumentMenuCommand::Heading(1),
                    DocumentMenuCommand::Heading(2),
                    DocumentMenuCommand::Heading(3),
                    DocumentMenuCommand::Heading(4),
                    DocumentMenuCommand::Heading(5),
                    DocumentMenuCommand::Heading(6),
                    DocumentMenuCommand::NormalText,
                ]
                .into_iter()
                .map(item)
                .chain([DocumentMenuRow::Separator])
                .chain(
                    [
                        DocumentMenuCommand::BulletList,
                        DocumentMenuCommand::NumberedList,
                        DocumentMenuCommand::TaskList,
                        DocumentMenuCommand::Quote,
                        DocumentMenuCommand::CodeBlock,
                    ]
                    .into_iter()
                    .map(item),
                )
                .collect()
            }
            DocumentSubmenu::Insert => {
                let item = |command: DocumentMenuCommand, name: &'static str, enabled: bool| {
                    DocumentMenuRow::Item {
                        command,
                        name,
                        enabled,
                    }
                };
                let insertable = self.writes_through_the_buffer();
                vec![
                    item(DocumentMenuCommand::InsertTable, "table", insertable),
                    item(
                        DocumentMenuCommand::InsertImage,
                        "insert-image",
                        self.image_insert_is_available(cx),
                    ),
                    item(
                        DocumentMenuCommand::InsertCodeBlock,
                        "insert-code-block",
                        self.insert_block_target_is_available(InsertBlockTarget::CodeBlock, cx),
                    ),
                    item(
                        DocumentMenuCommand::InsertMathBlock,
                        "insert-math-block",
                        self.insert_block_target_is_available(InsertBlockTarget::MathBlock, cx),
                    ),
                    item(
                        DocumentMenuCommand::InsertSeparator,
                        "insert-separator",
                        self.insert_block_target_is_available(InsertBlockTarget::Separator, cx),
                    ),
                    item(
                        DocumentMenuCommand::InsertToc,
                        "insert-toc",
                        self.insert_block_target_is_available(InsertBlockTarget::Toc, cx),
                    ),
                    item(
                        DocumentMenuCommand::InsertFrontMatter,
                        "insert-front-matter",
                        self.insert_block_target_is_available(InsertBlockTarget::FrontMatter, cx),
                    ),
                ]
            }
        }
    }

    /// 点一行菜单：除「插入表格」外先收起菜单，动作与键盘派发的同一个。
    pub(crate) fn run_document_menu_command(
        &mut self,
        command: DocumentMenuCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 「插入表格」要从菜单状态里取插入位置，收菜单的活由它自己那条路做。
        if !matches!(command, DocumentMenuCommand::InsertTable) {
            self.close_context_menu(cx);
        }
        match command {
            DocumentMenuCommand::Undo => window.dispatch_action(Box::new(Undo), cx),
            DocumentMenuCommand::Redo => window.dispatch_action(Box::new(Redo), cx),
            DocumentMenuCommand::Cut => window.dispatch_action(Box::new(Cut), cx),
            DocumentMenuCommand::Copy => window.dispatch_action(Box::new(Copy), cx),
            DocumentMenuCommand::Paste => window.dispatch_action(Box::new(Paste), cx),
            DocumentMenuCommand::PasteAsPlainText => {
                window.dispatch_action(Box::new(PasteAsPlainText), cx);
            }
            DocumentMenuCommand::CopyAsMarkdown => {
                window.dispatch_action(Box::new(CopyAsMarkdown), cx);
            }
            DocumentMenuCommand::CopyAsHtml => window.dispatch_action(Box::new(CopyAsHtml), cx),
            DocumentMenuCommand::Format(format) => {
                self.toggle_inline_format_on_selection(format, cx);
            }
            DocumentMenuCommand::Heading(level) => {
                self.apply_block_kind_to_selection(BlockKindTarget::Heading(level), cx);
            }
            DocumentMenuCommand::NormalText => {
                self.apply_block_kind_to_selection(BlockKindTarget::Paragraph, cx);
            }
            DocumentMenuCommand::BulletList => {
                self.apply_block_kind_to_selection(BlockKindTarget::BulletList, cx);
            }
            DocumentMenuCommand::NumberedList => {
                self.apply_block_kind_to_selection(BlockKindTarget::NumberedList, cx);
            }
            DocumentMenuCommand::TaskList => {
                self.apply_block_kind_to_selection(BlockKindTarget::TaskList, cx);
            }
            DocumentMenuCommand::Quote => {
                self.apply_block_kind_to_selection(BlockKindTarget::Quote, cx);
            }
            DocumentMenuCommand::CodeBlock => {
                self.apply_block_kind_to_selection(BlockKindTarget::CodeBlock, cx);
            }
            DocumentMenuCommand::Link => {
                self.insert_link_on_selection(cx);
            }
            DocumentMenuCommand::ClearFormat => {
                self.clear_inline_format_on_selection(cx);
            }
            DocumentMenuCommand::ToggleSourceView => {
                window.dispatch_action(Box::new(ToggleViewMode), cx);
            }
            DocumentMenuCommand::InsertTable => self.open_table_insert_dialog_from_menu(cx),
            DocumentMenuCommand::InsertImage => self.open_image_picker(cx),
            DocumentMenuCommand::InsertCodeBlock => {
                self.insert_block_after_selection(InsertBlockTarget::CodeBlock, cx);
            }
            DocumentMenuCommand::InsertMathBlock => {
                self.insert_block_after_selection(InsertBlockTarget::MathBlock, cx);
            }
            DocumentMenuCommand::InsertSeparator => {
                self.insert_block_after_selection(InsertBlockTarget::Separator, cx);
            }
            DocumentMenuCommand::InsertToc => {
                self.insert_block_after_selection(InsertBlockTarget::Toc, cx);
            }
            DocumentMenuCommand::InsertFrontMatter => {
                self.insert_block_after_selection(InsertBlockTarget::FrontMatter, cx);
            }
        }
    }

    /// 悬停展开：`submenu` 为 `Some` 表示鼠标正停在某个二级菜单的父行上。
    pub(crate) fn set_document_menu_hover(
        &mut self,
        hovered: bool,
        submenu: Option<DocumentSubmenu>,
        cx: &mut Context<Self>,
    ) {
        let Some(ContextMenuState::Document {
            open_submenu,
            parent_row_hovered,
            submenu_panel_hovered,
            ..
        }) = self.context_menu.as_mut()
        else {
            return;
        };

        let hovered_row = submenu.filter(|_| hovered);
        let mut changed = false;
        if let Some(row) = hovered_row {
            if *parent_row_hovered != Some(row) {
                *parent_row_hovered = Some(row);
                changed = true;
            }
            if open_submenu != &Some(row) {
                *open_submenu = Some(row);
                // 换了一档就是换了一块面板：旧面板那一次的悬停不该留给新的那一块。
                *submenu_panel_hovered = false;
                changed = true;
            }
        } else if let Some(row) = submenu {
            // 鼠标离开了某一条父行：只清掉「就是这一条」的那份记录。
            // 三条父行挨着，一次移动会同时送来「进了新的」与「离开了旧的」，
            // 后到的这一份不能把前一份抹掉（后者的监听注册在后，先跑）。
            if *parent_row_hovered == Some(row) {
                *parent_row_hovered = None;
                changed = true;
            }
        }
        self.settle_document_submenu_hover(cx);
        if changed {
            cx.notify();
        }
    }

    /// 二级面板自己的悬停。与父行各记一份：从父行横到面板上时两边的事件在
    /// 同一次移动里先后到达，共用一份状态就会让后到的那个把先到的抹掉，
    /// 鼠标还停在面板上，延时一到它自己就收掉了。
    pub(crate) fn set_document_submenu_panel_hover(
        &mut self,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(ContextMenuState::Document {
            submenu_panel_hovered,
            ..
        }) = self.context_menu.as_mut()
        {
            *submenu_panel_hovered = hovered;
        }
        self.settle_document_submenu_hover(cx);
    }

    /// 主面板与二级面板中间那条缝的悬停：停在这条缝里也算「还在菜单上」。
    /// 缝只有几个像素宽，从父行横到面板上时正好会经过它（用户报修：停在缝里
    /// 面板立马就收）。
    pub(crate) fn set_document_submenu_bridge_hover(
        &mut self,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(ContextMenuState::Document {
            submenu_bridge_hovered,
            ..
        }) = self.context_menu.as_mut()
        {
            *submenu_bridge_hovered = hovered;
        }
        self.settle_document_submenu_hover(cx);
    }

    /// 三个悬停区（父行、两块面板中间那条缝、二级面板）任一处还停着指针，
    /// 展开的那一档就留着；全离开了才挂上延时收起。各区各记一份状态：同一次
    /// 移动里「进了新的」与「离开了旧的」前后脚到达，谁后到都不能把另一处抹掉。
    fn settle_document_submenu_hover(&mut self, cx: &mut Context<Self>) {
        let Some((hovered, open)) = self
            .context_menu
            .as_ref()
            .and_then(ContextMenuState::submenu_hover_regions)
        else {
            return;
        };
        if hovered {
            self.context_menu_submenu_close_task = None;
        } else if open {
            self.schedule_document_menu_submenu_close(cx);
        }
    }

    fn schedule_document_menu_submenu_close(&mut self, cx: &mut Context<Self>) {
        let weak_editor = cx.entity().downgrade();
        self.context_menu_submenu_close_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor().timer(SUBMENU_CLOSE_DELAY).await;
                let _ = weak_editor.update(cx, |editor, cx| {
                    editor.context_menu_submenu_close_task = None;
                    let Some((false, true)) = editor
                        .context_menu
                        .as_ref()
                        .and_then(ContextMenuState::submenu_hover_regions)
                    else {
                        return;
                    };
                    if let Some(ContextMenuState::Document { open_submenu, .. }) =
                        editor.context_menu.as_mut()
                    {
                        *open_submenu = None;
                        cx.notify();
                    }
                });
            },
        ));
    }
}

/// 一行菜单的文字。
pub(crate) fn document_menu_label(
    command: DocumentMenuCommand,
    strings: &crate::i18n::I18nStrings,
) -> String {
    match command {
        DocumentMenuCommand::Undo => strings.context_menu_undo.clone(),
        DocumentMenuCommand::Redo => strings.context_menu_redo.clone(),
        DocumentMenuCommand::Cut => strings.context_menu_cut.clone(),
        DocumentMenuCommand::Copy => strings.context_menu_copy.clone(),
        DocumentMenuCommand::Paste => strings.context_menu_paste.clone(),
        DocumentMenuCommand::PasteAsPlainText => {
            strings.context_menu_paste_as_plain_text.clone()
        }
        DocumentMenuCommand::CopyAsMarkdown => strings.context_menu_copy_as_markdown.clone(),
        DocumentMenuCommand::CopyAsHtml => strings.menu_copy_as_html.clone(),
        DocumentMenuCommand::Format(format) => inline_format_label(format, strings),
        DocumentMenuCommand::Heading(level) => match level {
            1 => strings.paragraph_heading1.clone(),
            2 => strings.paragraph_heading2.clone(),
            3 => strings.paragraph_heading3.clone(),
            4 => strings.paragraph_heading4.clone(),
            5 => strings.paragraph_heading5.clone(),
            _ => strings.paragraph_heading6.clone(),
        },
        DocumentMenuCommand::NormalText => strings.paragraph_normal_text.clone(),
        DocumentMenuCommand::BulletList => strings.paragraph_bullet_list.clone(),
        DocumentMenuCommand::NumberedList => strings.paragraph_numbered_list.clone(),
        DocumentMenuCommand::TaskList => strings.paragraph_task_list.clone(),
        DocumentMenuCommand::Quote => strings.paragraph_quote.clone(),
        DocumentMenuCommand::CodeBlock => strings.paragraph_code_block.clone(),
        DocumentMenuCommand::Link => strings.insert_link.clone(),
        DocumentMenuCommand::ClearFormat => strings.format_clear.clone(),
        DocumentMenuCommand::InsertTable => strings.context_menu_table.clone(),
        DocumentMenuCommand::InsertImage => strings.insert_image.clone(),
        DocumentMenuCommand::InsertCodeBlock => strings.paragraph_code_block.clone(),
        DocumentMenuCommand::InsertMathBlock => strings.insert_math_block.clone(),
        DocumentMenuCommand::InsertSeparator => strings.insert_separator.clone(),
        DocumentMenuCommand::InsertToc => strings.insert_toc.clone(),
        DocumentMenuCommand::InsertFrontMatter => strings.insert_front_matter.clone(),
        DocumentMenuCommand::ToggleSourceView => strings.context_menu_toggle_source_view.clone(),
    }
}

fn inline_format_label(format: InlineFormat, strings: &crate::i18n::I18nStrings) -> String {
    match format {
        InlineFormat::Bold => strings.format_bold.clone(),
        InlineFormat::Italic => strings.format_italic.clone(),
        InlineFormat::Underline => strings.format_underline.clone(),
        InlineFormat::Strikethrough => strings.format_strikethrough.clone(),
        InlineFormat::Code => strings.format_code.clone(),
        InlineFormat::Highlight => strings.format_highlight.clone(),
        InlineFormat::Superscript => strings.format_superscript.clone(),
        InlineFormat::Subscript => strings.format_subscript.clone(),
    }
}

/// 行右侧的快捷键文字，取当前生效的那份键位（用户在偏好页改过就是改成的键）；
/// 没有快捷键位的条目返回 None（留空位对齐，菜单宽度不跳）。
pub(crate) fn document_menu_shortcut(
    command: DocumentMenuCommand,
    cx: &App,
) -> Option<SharedString> {
    let shortcut = match command {
        DocumentMenuCommand::Undo => ShortcutCommand::Undo,
        DocumentMenuCommand::Redo => ShortcutCommand::Redo,
        DocumentMenuCommand::Cut => ShortcutCommand::Cut,
        DocumentMenuCommand::Copy => ShortcutCommand::Copy,
        DocumentMenuCommand::Paste => ShortcutCommand::Paste,
        DocumentMenuCommand::PasteAsPlainText => ShortcutCommand::PasteAsPlainText,
        DocumentMenuCommand::CopyAsMarkdown => ShortcutCommand::CopyAsMarkdown,
        // 「复制为 HTML」的 ⌘⇧C 是写死的一份绑定（src/components/actions.rs 末尾），
        // 不在键位表里，因此这一列留空——等它补成表内条目时一起显示。
        DocumentMenuCommand::Format(InlineFormat::Bold) => ShortcutCommand::BoldSelection,
        DocumentMenuCommand::Format(InlineFormat::Italic) => ShortcutCommand::ItalicSelection,
        DocumentMenuCommand::Format(InlineFormat::Underline) => ShortcutCommand::UnderlineSelection,
        DocumentMenuCommand::Format(InlineFormat::Strikethrough) => {
            ShortcutCommand::StrikethroughSelection
        }
        DocumentMenuCommand::Format(InlineFormat::Code) => ShortcutCommand::CodeSelection,
        DocumentMenuCommand::Format(InlineFormat::Superscript) => {
            ShortcutCommand::SuperscriptSelection
        }
        DocumentMenuCommand::Format(InlineFormat::Subscript) => ShortcutCommand::SubscriptSelection,
        DocumentMenuCommand::Format(InlineFormat::Highlight) => ShortcutCommand::HighlightSelection,
        DocumentMenuCommand::ClearFormat => ShortcutCommand::ClearFormatSelection,
        DocumentMenuCommand::Link => ShortcutCommand::LinkSelection,
        DocumentMenuCommand::InsertImage => ShortcutCommand::InsertImage,
        DocumentMenuCommand::ToggleSourceView => ShortcutCommand::ToggleViewMode,
        // 段落与插入那一档不给默认键：标题的 ⌘1..⌘6 与本仓换标签的 ⌘1-9 撞车，列表、引用、
        // 代码块与目录、分割线这些也没有通行的键位可以对齐，硬造一串反而要多记一件事。
        DocumentMenuCommand::Heading(_)
        | DocumentMenuCommand::NormalText
        | DocumentMenuCommand::BulletList
        | DocumentMenuCommand::NumberedList
        | DocumentMenuCommand::TaskList
        | DocumentMenuCommand::Quote
        | DocumentMenuCommand::CodeBlock
        | DocumentMenuCommand::InsertTable
        | DocumentMenuCommand::InsertCodeBlock
        | DocumentMenuCommand::InsertMathBlock
        | DocumentMenuCommand::InsertSeparator
        | DocumentMenuCommand::InsertToc
        | DocumentMenuCommand::InsertFrontMatter
        | DocumentMenuCommand::CopyAsHtml => return None,
    };
    Some(SharedString::from(key_label(&effective_shortcut_key(
        shortcut, cx,
    )?)))
}

/// 修饰键的显示次序与字形：macOS 用 ⌃⌥⌘⇧，其他平台用 Ctrl+Alt+Super+Shift。
const MODIFIERS: [(&str, &str, &str); 4] = [
    ("ctrl", "\u{2303}", "Ctrl"),
    ("alt", "\u{2325}", "Alt"),
    ("cmd", "\u{2318}", "Super"),
    ("shift", "\u{21E7}", "Shift"),
];

/// `cmd-shift-x` → `\u{21E7}\u{2318}X`（macOS）或 `Ctrl+Shift+X` 一类的写法。
fn key_label(key: &str) -> String {
    let mac = cfg!(target_os = "macos");
    let mut held = Vec::new();
    let mut main = String::new();
    for part in key.split('-') {
        let part = match part {
            "win" | "super" => "cmd",
            _ => part,
        };
        match MODIFIERS.iter().position(|(name, _, _)| *name == part) {
            Some(index) => held.push(index),
            None => main = readable_key(part),
        }
    }
    held.sort_unstable();
    if mac {
        let mut label = String::new();
        for index in held {
            label.push_str(MODIFIERS[index].1);
        }
        label.push_str(&main);
        label
    } else {
        let mut parts: Vec<&str> = held.iter().map(|index| MODIFIERS[*index].2).collect();
        parts.push(&main);
        parts.join("+")
    }
}

fn readable_key(part: &str) -> String {
    match part {
        "space" => "Space",
        "enter" => "Return",
        "backspace" => "Delete",
        "delete" => "Del",
        "left" => "\u{2190}",
        "right" => "\u{2192}",
        "up" => "\u{2191}",
        "down" => "\u{2193}",
        "tab" => "Tab",
        "esc" => "Esc",
        other => {
            return other
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic())
                .then(|| other.to_uppercase())
                .unwrap_or_else(|| other.to_string())
        }
    }
    .to_string()
}

/// 菜单离窗口边缘至少留出的距离：贴边摆放会让圆角和描边看着像被裁掉。
const MENU_VIEWPORT_MARGIN: f32 = 6.0;
/// 二级菜单父行右侧那枚箭头的占位。
const SUBMENU_ARROW: &str = "\u{203a}";
/// 一行宽度的余量：`estimated_menu_label_width` 是按字符类别估的，实测英文最宽那一行
/// （`Numbered List`，12 号字）估出来只比真实宽度多 0.4px——换个字体回落或缩放就截字
/// （用户报修：「一级标题」只剩「一级标」）。Windows 原生测试又量到面板被内容
/// 撑宽 1px，留 4px 覆盖字形回落和像素取整差异，面板尺寸仍由同一份几何决定。
const MENU_ROW_WIDTH_ALLOWANCE: f32 = 4.0;

/// 一列行的几何：面板尺寸与每一行的顶部偏移。渲染摆放与落点计算共用同一份，
/// 免得两处对行高的理解不一致。
pub(crate) struct DocumentMenuGeometry {
    pub size: Size<Pixels>,
    row_tops: Vec<Pixels>,
}

impl DocumentMenuGeometry {
    /// `shortcut_of` 由调用方给（渲染路径用当前生效的键位，纯函数用例给一份桩），
    /// 面板宽度按屏上真要写出来的那串按键估，用户改过键位也不会截字。
    pub(crate) fn measure(
        rows: &[DocumentMenuRow],
        strings: &crate::i18n::I18nStrings,
        dimensions: &ThemeDimensions,
        shortcut_of: &dyn Fn(DocumentMenuCommand) -> Option<SharedString>,
    ) -> Self {
        let mut widest = 0.0_f32;
        // 第一行的顶部就是面板内边距；往下逐行累加行高与行间距。
        let mut top = dimensions.menu_panel_padding;
        let mut row_tops = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                top += dimensions.menu_panel_gap;
            }
            row_tops.push(px(top));
            let row_height = match row {
                DocumentMenuRow::Separator => {
                    dimensions.menu_separator_height + dimensions.menu_separator_margin_y * 2.0
                }
                DocumentMenuRow::Item { .. } | DocumentMenuRow::Submenu { .. } => {
                    dimensions.menu_item_height
                }
                DocumentMenuRow::QuickActions => QUICK_ACTION_BUTTON_SIZE,
            };
            top += row_height;
            widest = widest.max(Self::row_width(row, strings, dimensions, shortcut_of));
        }
        // 末尾再补一份内边距：面板高度 = 最后一行底部 + 内边距。宽度那一条算过边框，
        // 高度同理漏了会把面板下沿推出落点之外（`document_menu_origins` 按这份高夹紧）。
        let height = top + dimensions.menu_panel_padding + dimensions.dialog_border_width * 2.0;
        // 宽度同理：`row_width` 里只含行自己的左右内边距，面板还要各加一份
        // `menu_panel_padding` 与一份边框。少算这些，面板的内框就比最宽那一行窄，
        // 中文标签会被截掉半个字（`menu_item` 那一份是 `.truncate()`，截了不报错）。
        let width =
            widest + dimensions.menu_panel_padding * 2.0 + dimensions.dialog_border_width * 2.0;
        Self {
            size: Size {
                width: px(width.ceil()),
                height: px(height.ceil()),
            },
            row_tops,
        }
    }

    /// 一行占的宽度：图标那一列 + 文字 + 快捷键那一列 + 二级菜单的箭头，再加左右内边距。
    fn row_width(
        row: &DocumentMenuRow,
        strings: &crate::i18n::I18nStrings,
        dimensions: &ThemeDimensions,
        shortcut_of: &dyn Fn(DocumentMenuCommand) -> Option<SharedString>,
    ) -> f32 {
        let text_size = dimensions.menu_text_size;
        let (label, shortcut, submenu, icon) = match row {
            // 图标条没有文字可估：宽度就是那六颗格子加竖线与间隙，行高另说。
            DocumentMenuRow::Separator => return 0.0,
            DocumentMenuRow::QuickActions => return quick_actions_width(),
            DocumentMenuRow::Item { command, .. } => (
                document_menu_label(*command, strings),
                shortcut_of(*command),
                false,
                document_menu_command_icon(*command),
            ),
            DocumentMenuRow::Submenu { id, .. } => (
                document_submenu_label(*id, strings),
                None,
                true,
                document_submenu_icon(*id),
            ),
        };
        let mut width = crate::editor::render::estimated_menu_label_width(&label, text_size);
        // 行首图标占的那一列：图标框边一份，它与文字之间用的就是 flex 那一份行距。
        // 这一列漏算就等于面板内框比行窄，中文标签被截掉半个字（FP12 那条根因）。
        if icon.is_some() {
            width += MENU_ICON_SIZE + MENU_ROW_GAP;
        }
        if let Some(shortcut) = shortcut {
            width += MENU_ROW_GAP
                + crate::editor::render::estimated_menu_label_width(shortcut.as_ref(), text_size);
        }
        if submenu {
            width += MENU_ROW_GAP
                + crate::editor::render::estimated_menu_label_width(SUBMENU_ARROW, text_size);
        }
        width + dimensions.menu_item_padding_x * 2.0 + MENU_ROW_WIDTH_ALLOWANCE
    }

    /// 第 `index` 行的顶部相对面板顶的偏移；越界与 `None` 都按面板顶部算。
    pub(crate) fn row_top(&self, index: Option<usize>) -> Pixels {
        index
            .and_then(|index| self.row_tops.get(index).copied())
            .unwrap_or(px(0.0))
    }
}

/// 两块面板中间那条缝的感应带横向范围：从主面板边缘一直盖到二级面板外沿。
/// 二级面板在右侧时是「主面板右沿 → 二级面板右沿」，放不下翻到左侧时镜像
/// （「二级面板左沿 → 主面板左沿」）。缝只有几个像素宽，鼠标从父行横过去时
/// 正好会经过它，停在那儿不该被当成离开菜单。
pub(crate) fn submenu_bridge_span(
    main_left: Pixels,
    main_width: Pixels,
    submenu_left: Pixels,
    submenu_width: Pixels,
) -> (Pixels, Pixels) {
    let main_right = main_left + main_width;
    if submenu_left >= main_right {
        (main_right, submenu_left + submenu_width)
    } else {
        (submenu_left, main_left)
    }
}

/// 主面板与二级面板的落点：面板贴着光标，右边或下边放不下就往窗口内收；
/// 二级面板在右侧放不下时改贴主面板左侧，顶部仍与父行对齐（对齐后放不下则向上收）。
pub(crate) fn document_menu_origins(
    cursor: Point<Pixels>,
    viewport: Size<Pixels>,
    main: &DocumentMenuGeometry,
    submenu: Option<(&DocumentMenuGeometry, Pixels)>,
    gap: Pixels,
) -> (Point<Pixels>, Option<Point<Pixels>>) {
    let margin = px(MENU_VIEWPORT_MARGIN);
    let right_limit = viewport.width - margin;
    let bottom_limit = viewport.height - margin;

    let mut panel_x = cursor.x;
    if panel_x + main.size.width > right_limit {
        panel_x = (right_limit - main.size.width).max(margin);
    }
    let mut panel_y = cursor.y;
    if panel_y + main.size.height > bottom_limit {
        panel_y = (bottom_limit - main.size.height).max(margin);
    }

    let submenu = submenu.map(|(geometry, parent_row_top)| {
        let mut x = panel_x + main.size.width + gap;
        if x + geometry.size.width > right_limit {
            x = panel_x - gap - geometry.size.width;
        }
        if x < margin {
            x = (right_limit - geometry.size.width).max(margin);
        }
        let mut y = panel_y + parent_row_top;
        if y + geometry.size.height > bottom_limit {
            y = (bottom_limit - geometry.size.height).max(margin);
        }
        Point { x, y }
    });

    (
        Point {
            x: panel_x,
            y: panel_y,
        },
        submenu,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn platform_modifier_aliases_are_not_lost_in_shortcut_labels() {
        for platform_key in ["win", "super", "cmd"] {
            assert_eq!(
                super::key_label(&format!("alt-{platform_key}-b")),
                super::key_label("alt-cmd-b"),
                "平台键 {platform_key} 不应被当成主键而丢失"
            );
        }
    }

    // 不用 `use super::*`：那样会把 gpui 的 `test` 宏带进来，`#[test]` 就地自我展开。
    use super::{
        document_menu_label, document_menu_origins, submenu_bridge_span, DocumentMenuCommand,
        DocumentMenuGeometry, DocumentMenuRow,
    };
    use crate::i18n::I18nStrings;
    use crate::theme::Theme;
    use gpui::{point, px, SharedString, Size};

    /// 纯函数用例没有 App，快捷键那一列给一份固定文字：测的是「这一列要算进宽度」。
    fn stub_shortcut(command: DocumentMenuCommand) -> Option<SharedString> {
        (command == DocumentMenuCommand::Undo).then(|| SharedString::from("Ctrl+Z"))
    }

    fn rows(count: usize) -> Vec<DocumentMenuRow> {
        (0..count)
            .map(|_| DocumentMenuRow::Item {
                command: DocumentMenuCommand::Undo,
                name: "undo",
                enabled: true,
            })
            .collect()
    }

    /// 菜单贴光标摆放，越界的一侧往窗口内收；二级面板右侧放不下时改贴左侧。
    #[test]
    fn menu_origins_are_pulled_back_at_the_viewport_edges() {
        let dimensions = Theme::default_theme().dimensions;
        let strings = I18nStrings::zh_cn();
        let main = DocumentMenuGeometry::measure(&rows(12), &strings, &dimensions, &stub_shortcut);
        let submenu =
            DocumentMenuGeometry::measure(&rows(4), &strings, &dimensions, &stub_shortcut);
        let viewport = Size {
            width: px(600.0),
            height: px(900.0),
        };

        // 光标在中间：面板贴着光标，二级面板开在右侧、与父行顶部对齐。
        let (origin, submenu_origin) = document_menu_origins(
            point(px(100.0), px(100.0)),
            viewport,
            &main,
            Some((&submenu, px(28.0))),
            px(2.0),
        );
        assert_eq!((f32::from(origin.x), f32::from(origin.y)), (100.0, 100.0));
        let submenu_origin = submenu_origin.expect("右侧放得下时二级面板该有落点");
        assert_eq!(
            (f32::from(submenu_origin.x), f32::from(submenu_origin.y)),
            (100.0 + f32::from(main.size.width) + 2.0, 128.0)
        );

        // 右下角：主面板左移上收，二级面板翻到主面板左侧。
        let (origin, submenu_origin) = document_menu_origins(
            point(px(590.0), px(895.0)),
            viewport,
            &main,
            Some((&submenu, px(28.0))),
            px(2.0),
        );
        let margin = px(6.0);
        assert!(
            origin.x + main.size.width <= viewport.width - margin,
            "右侧越界没收回：落点 {:?}",
            f32::from(origin.x)
        );
        assert!(
            origin.y + main.size.height <= viewport.height - margin,
            "下侧越界没收回：落点 {:?}",
            f32::from(origin.y)
        );
        let flipped = submenu_origin.expect("翻到左侧也该有落点");
        assert!(
            flipped.x + submenu.size.width <= origin.x,
            "二级面板没翻到主面板左侧"
        );
        assert!(
            flipped.y + submenu.size.height <= viewport.height - margin,
            "与父行对齐后仍然越界，该向上收"
        );
    }

    /// 缝的感应带：二级面板在右时盖住「缝 + 整块二级面板」，翻到左侧时镜像——
    /// 两边的缝都要有带子，否刚窄视口里翻过去的那一侧又会“停一下就没”。
    #[test]
    fn submenu_bridge_spans_the_seam_on_either_side() {
        let (left, right) = submenu_bridge_span(px(100.0), px(275.0), px(377.0), px(200.0));
        assert_eq!(
            (f32::from(left), f32::from(right)),
            (375.0, 577.0),
            "二级面板在右侧时带子该从主面板右沿盖到二级面板右沿"
        );

        let (left, right) = submenu_bridge_span(px(326.0), px(275.0), px(94.0), px(230.0));
        assert_eq!(
            (f32::from(left), f32::from(right)),
            (94.0, 326.0),
            "翻到左侧时带子该镜像成「二级面板左沿 → 主面板左沿」"
        );
    }

    /// 面板尺寸按最宽的一行估：带快捷键的那一行比裸文字宽，中英文都算得下才不截字。
    #[test]
    fn panel_size_follows_the_widest_row() {
        let dimensions = Theme::default_theme().dimensions;
        let zh = DocumentMenuGeometry::measure(
            &rows(3),
            &I18nStrings::zh_cn(),
            &dimensions,
            &stub_shortcut,
        );
        let en = DocumentMenuGeometry::measure(
            &rows(3),
            &I18nStrings::en_us(),
            &dimensions,
            &stub_shortcut,
        );
        let single = DocumentMenuGeometry::measure(
            &rows(1),
            &I18nStrings::zh_cn(),
            &dimensions,
            &stub_shortcut,
        );
        assert!(
            f32::from(en.size.width) > f32::from(zh.size.width),
            "英文行该比中文行宽：{:?} vs {:?}",
            f32::from(en.size.width),
            f32::from(zh.size.width)
        );
        assert!(
            en.size.height > single.size.height + px(dimensions.menu_item_height * 2.0),
            "高度没按行数累加"
        );
        // 快捷键那一列 + 左右内边距都算进宽度里，不能只量文字。
        let label_only = crate::editor::render::estimated_menu_label_width(
            &document_menu_label(DocumentMenuCommand::Undo, &I18nStrings::en_us()),
            dimensions.menu_text_size,
        );
        assert!(
            f32::from(single.size.width) > label_only + dimensions.menu_item_padding_x * 2.0,
            "宽度漏了快捷键那一列"
        );
    }

    /// 面板的内框要装得下最宽那一行：行自己带左右 `menu_item_padding_x`，面板还要各加一份
    /// `menu_panel_padding`。少算后一份，中文标签就被 `.truncate()` 截掉半个字。
    #[test]
    fn panel_inner_box_fits_the_widest_row() {
        let dimensions = Theme::default_theme().dimensions;
        let rows = rows(12);
        for (tag, strings) in [
            ("中文", I18nStrings::zh_cn()),
            ("英文", I18nStrings::en_us()),
        ] {
            let geometry =
                DocumentMenuGeometry::measure(&rows, &strings, &dimensions, &stub_shortcut);
            let widest = rows
                .iter()
                .map(|row| {
                    DocumentMenuGeometry::row_width(row, &strings, &dimensions, &stub_shortcut)
                })
                .fold(0.0_f32, f32::max);
            let inner_box = f32::from(geometry.size.width)
                - dimensions.menu_panel_padding * 2.0
                - dimensions.dialog_border_width * 2.0;
            assert!(
                inner_box + 0.5 >= widest,
                "{tag}：面板内框比最宽那一行窄，标签会截字（内框 {inner_box:?} vs 行宽 {widest:?}）"
            );
        }
    }
}
