//! 选中一段文字后浮出来的那一条工具栏：与右键菜单同一批动作，就近给一次。
//!
//! 面板本身不实现编辑行为——按钮走的是编辑器层那两条入口（行内格式、段落转换），
//! 与快捷键、右键菜单三个入口共用同一份实现。位置跟着选区的外接框走：优先在选区
//! 上方，放不下改到下方，左右再按视口收回。

use gpui::*;

use super::context_menu::{
    document_menu_label, document_menu_shortcut, DocumentMenuCommand, DocumentMenuGeometry,
    DocumentMenuRow, DocumentSubmenu,
};
use super::paragraph_ops::BlockKindTarget;
use super::{Editor, ViewMode};
use crate::components::HoverPreviewTooltip;
use crate::components::{menu::menu_item, InlineFormat};
use crate::editor::render::estimated_menu_label_width;
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// 一个方形按钮的边长。
const BUTTON_SIZE: f32 = 26.0;
/// 按钮之间的间距；面板内边距另算一份。
const BUTTON_GAP: f32 = 2.0;
const PANEL_PADDING: f32 = 4.0;
/// 面板与选区外接框之间的距离。
const SELECTION_OFFSET: f32 = 8.0;
/// 面板离窗口边缘的最小距离。
const VIEWPORT_MARGIN: f32 = 6.0;
/// 「标题」按钮里文字与箭头之间的间距。
const HEADING_ARROW_GAP: f32 = 4.0;
/// 箭头字形（下指的 chevron）。
const HEADING_ARROW: &str = "\u{2304}";

/// 工具栏上的一次动作。
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SelectionToolbarCommand {
    Format(InlineFormat),
    /// 「插入链接」：把选中的文字包成 `[文字]()`，光标停在括号里等写地址。
    Link,
    /// 段落那一档的转换目标：六档标题、正文与三种列表，行数据与右键菜单同一份。
    Paragraph(BlockKindTarget),
}

/// 工具栏的现场：本帧面板（含展开的档位列表）在屏幕上的框，与档位列表是否开着。
#[derive(Default)]
pub(crate) struct SelectionToolbarState {
    panel_bounds: Option<Bounds<Pixels>>,
    heading_menu_open: bool,
}

impl SelectionToolbarState {
    fn contains(&self, position: Point<Pixels>) -> bool {
        self.panel_bounds
            .is_some_and(|bounds| bounds.contains(&position))
    }
}

/// 方形按钮那几颗：id 同时是元素 id 与测试选择器。上下标不在这条上（出现频率低，
/// 右键菜单的格式那一档里有）。
const FORMAT_BUTTONS: [(InlineFormat, &str); 6] = [
    (InlineFormat::Bold, "toolbar-bold"),
    (InlineFormat::Italic, "toolbar-italic"),
    (InlineFormat::Underline, "toolbar-underline"),
    (InlineFormat::Strikethrough, "toolbar-strikethrough"),
    (InlineFormat::Code, "toolbar-code"),
    (InlineFormat::Highlight, "toolbar-highlight"),
];

impl Editor {
    /// 按下落在工具栏（或它展开的档位列表）上时不能当成正文落点：否则这一次按下
    /// 先把选区收成光标，工具栏自己就先消失了。
    pub(crate) fn selection_toolbar_contains_point(&self, position: Point<Pixels>) -> bool {
        self.selection_toolbar
            .as_ref()
            .is_some_and(|state| state.contains(position))
    }

    /// 选区在屏幕上的外接框（窗口绝对坐标）。跨块时逐块量再并起来。
    fn selection_screen_bounds(&self, cx: &App) -> Option<Bounds<Pixels>> {
        if self.cross_block_selection.is_some() {
            let mut union: Option<Bounds<Pixels>> = None;
            for entry in self.document.visible_blocks() {
                let block = entry.entity.read(cx);
                let Some(range) = block.editor_selection_range.clone() else {
                    continue;
                };
                if range.is_empty() {
                    continue;
                }
                let Some(bounds) = block.visible_range_bounds(range) else {
                    continue;
                };
                union = Some(match union {
                    Some(existing) => existing.union(&bounds),
                    None => bounds,
                });
            }
            return union;
        }
        let target = self.current_edit_target_from_state(cx)?;
        let block = target.read(cx);
        if block.selected_range.is_empty() {
            return None;
        }
        block.visible_range_bounds(block.selected_range.clone())
    }

    /// 这一帧工具栏该锚在哪：渲染态、按区间写回的文档、没有别的浮层、鼠标已抬手，
    /// 且有一段量得到边界的选区。不成立时返回 None，一次选区扫描都不多做。
    fn selection_toolbar_anchor(&self, cx: &App) -> Option<Bounds<Pixels>> {
        if self.view_mode != ViewMode::Rendered || !self.writes_through_the_buffer() {
            return None;
        }
        if self.cross_block_drag.is_some()
            || self.context_menu.is_some()
            || self.table_insert_dialog.is_some()
            || self.modal_is_open()
            || self.info_dialog.is_some()
            || self.menu_bar_open.is_some()
            || self.wikilink_completion_is_open()
        {
            return None;
        }
        if !self.has_text_selection(cx) {
            return None;
        }
        self.selection_screen_bounds(cx)
    }

    /// 「标题」那颗按钮的宽度：中文「段落」两字加箭头，英文 `Paragraph` 加箭头，
    /// 按文字估，最小仍是一颗方形按钮。
    fn heading_button_width(strings: &crate::i18n::I18nStrings, theme: &Theme) -> f32 {
        let text_size = theme.dimensions.menu_text_size;
        let content = estimated_menu_label_width(&strings.context_menu_paragraph, text_size)
            + HEADING_ARROW_GAP
            + estimated_menu_label_width(HEADING_ARROW, text_size);
        content.ceil().max(BUTTON_SIZE) + PANEL_PADDING
    }

    fn toolbar_size(strings: &crate::i18n::I18nStrings, theme: &Theme) -> Size<Pixels> {
        // 方形按钮：六个行内样式加一颗链接。
        let square_buttons = FORMAT_BUTTONS.len() as f32 + 1.0;
        let width = PANEL_PADDING * 2.0
            + Self::heading_button_width(strings, theme)
            + BUTTON_GAP
            + square_buttons * (BUTTON_SIZE + BUTTON_GAP);
        Size {
            width: px(width.ceil()),
            height: px(PANEL_PADDING * 2.0 + BUTTON_SIZE),
        }
    }

    /// 选区整段滚出视口时不再画工具栏：`last_bounds` 可能还是滚动前的值，
    /// 只按它摆放会让工具栏留在原地，指着一屏里根本没有的文字。
    pub(crate) fn selection_is_on_screen(
        selection: Bounds<Pixels>,
        viewport: Size<Pixels>,
    ) -> bool {
        selection.bottom() > px(0.0)
            && selection.top() < viewport.height
            && selection.right() > px(0.0)
            && selection.left() < viewport.width
    }

    /// 面板落点：水平居中于选区，优先在选区上方；上方放不下改到下方，越界的边按视口收回。
    pub(crate) fn toolbar_origin(
        selection: Bounds<Pixels>,
        size: Size<Pixels>,
        viewport: Size<Pixels>,
    ) -> Point<Pixels> {
        let margin = px(VIEWPORT_MARGIN);
        let above = selection.top() - px(SELECTION_OFFSET) - size.height;
        let y = if above >= margin {
            above
        } else {
            let below = selection.bottom() + px(SELECTION_OFFSET);
            if below + size.height > viewport.height - margin {
                (viewport.height - margin - size.height).max(margin)
            } else {
                below
            }
        };
        let centered = selection.left() + (selection.size.width - size.width) * 0.5;
        let x = centered
            .max(margin)
            .min((viewport.width - margin - size.width).max(margin));
        Point { x, y }
    }

    /// 档位列表贴着工具栏下沿展开；下方放不下就贴着上沿向上展开。返回它相对工具栏的
    /// 顶部偏移与窗口坐标里的落点（后者用来记账面板边界）。
    fn heading_menu_offsets(
        toolbar: Point<Pixels>,
        toolbar_size: Size<Pixels>,
        menu_size: Size<Pixels>,
        viewport: Size<Pixels>,
    ) -> (Pixels, Point<Pixels>) {
        let margin = px(VIEWPORT_MARGIN);
        let below = toolbar.y + toolbar_size.height + px(BUTTON_GAP);
        let (offset, y) = if below + menu_size.height <= viewport.height - margin {
            (toolbar_size.height + px(BUTTON_GAP), below)
        } else {
            let above = (toolbar.y - px(BUTTON_GAP) - menu_size.height).max(margin);
            (-(menu_size.height + px(BUTTON_GAP)), above)
        };
        let x = toolbar
            .x
            .min((viewport.width - margin - menu_size.width).max(margin));
        (offset, Point { x, y })
    }

    /// 选中工具栏：一颗「标题」下拉加六颗行内格式按钮。
    pub(crate) fn render_selection_toolbar(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Some(selection) = self.selection_toolbar_anchor(cx) else {
            // 这一帧没有可锚的选区：现场收掉，下一帧不再画。这里不 notify，
            // 因为本次渲染已经把工具栏去掉了，再要一帧只会多跑一遍。
            self.selection_toolbar = None;
            return None;
        };
        let strings = cx.global::<I18nManager>().strings().clone();
        let viewport = window.viewport_size();
        if !Self::selection_is_on_screen(selection, viewport) {
            self.selection_toolbar = None;
            return None;
        }
        let size = Self::toolbar_size(&strings, theme);
        let origin = Self::toolbar_origin(selection, size, viewport);

        let heading_menu_open = self
            .selection_toolbar
            .as_ref()
            .is_some_and(|state| state.heading_menu_open);
        let heading_menu = heading_menu_open.then(|| {
            let rows = self.document_submenu_rows(DocumentSubmenu::Paragraph, cx);
            let geometry = DocumentMenuGeometry::measure(&rows, &strings, &theme.dimensions);
            let (offset, menu_origin) =
                Self::heading_menu_offsets(origin, size, geometry.size, viewport);
            (rows, geometry, offset, menu_origin)
        });

        let state = self.selection_toolbar.get_or_insert_default();
        let mut panel = Bounds::new(origin, size);
        if let Some((_, geometry, _, menu_origin)) = heading_menu.as_ref() {
            panel = panel.union(&Bounds::new(*menu_origin, geometry.size));
        }
        state.panel_bounds = Some(panel);

        let mut toolbar = Self::toolbar_panel(theme, origin, size);
        toolbar = toolbar.child(self.heading_button(
            &strings,
            theme,
            Self::heading_button_width(&strings, theme),
            heading_menu_open,
            cx,
        ));
        for (format, id) in FORMAT_BUTTONS {
            toolbar = toolbar.child(self.format_button(format, id, &strings, theme, cx));
        }
        toolbar = toolbar.child(self.link_button(&strings, theme, cx));
        if let Some((rows, geometry, offset, _)) = heading_menu.as_ref() {
            toolbar = toolbar.child(self.heading_menu_panel(
                theme,
                &strings,
                rows,
                geometry.size.width,
                *offset,
                cx,
            ));
        }
        Some(toolbar.into_any_element())
    }

    fn toolbar_panel(theme: &Theme, origin: Point<Pixels>, size: Size<Pixels>) -> Stateful<Div> {
        let c = &theme.colors;
        let d = &theme.dimensions;
        div()
            .id("editor-selection-toolbar")
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(size.width)
            .h(size.height)
            .flex()
            .items_center()
            .gap(px(BUTTON_GAP))
            .p(px(PANEL_PADDING))
            .occlude()
            .bg(c.dialog_surface)
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .rounded(px(d.menu_panel_radius))
            .shadow_lg()
            .debug_selector(|| "editor-selection-toolbar".to_string())
    }

    fn heading_button(
        &self,
        strings: &crate::i18n::I18nStrings,
        theme: &Theme,
        width: f32,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let label = strings.context_menu_paragraph.clone();
        let tooltip = label.clone();
        div()
            .id("toolbar-heading")
            .w(px(width))
            .h(px(BUTTON_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(HEADING_ARROW_GAP))
            .rounded(px(d.menu_item_radius))
            .bg(if open {
                c.dialog_secondary_button_hover
            } else {
                c.dialog_surface
            })
            .text_size(px(d.menu_text_size))
            .text_color(c.dialog_secondary_button_text)
            .cursor_pointer()
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .active(|this| this.opacity(0.92))
            .debug_selector(|| "toolbar-heading".to_string())
            .tooltip(move |_, cx| {
                cx.new(|_| HoverPreviewTooltip {
                    label: tooltip.clone().into(),
                })
                .into()
            })
            .child(label)
            .child(HEADING_ARROW)
            .on_click(cx.listener(|editor, _event, _window, cx| {
                let Some(state) = editor.selection_toolbar.as_mut() else {
                    return;
                };
                state.heading_menu_open = !state.heading_menu_open;
                cx.notify();
            }))
            .into_any_element()
    }

    /// 一颗方形按钮：字面形状就是它要做的事（B 加粗、I 斜体、U 带下划线……）。
    fn format_button(
        &self,
        format: InlineFormat,
        id: &'static str,
        strings: &crate::i18n::I18nStrings,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let menu_command = DocumentMenuCommand::Format(format);
        let label = document_menu_label(menu_command, strings);
        let tooltip = match document_menu_shortcut(menu_command) {
            Some(shortcut) => format!("{label}  {shortcut}"),
            None => label,
        };
        let text_color = c.dialog_secondary_button_text;
        let button = div()
            .id(id)
            .w(px(BUTTON_SIZE))
            .h(px(BUTTON_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(d.menu_item_radius))
            .bg(c.dialog_surface)
            .text_size(px(d.menu_text_size))
            .text_color(text_color)
            .cursor_pointer()
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .active(|this| this.opacity(0.92))
            .debug_selector(move || id.to_string())
            .tooltip(move |_, cx| {
                cx.new(|_| HoverPreviewTooltip {
                    label: tooltip.clone().into(),
                })
                .into()
            });
        let command = SelectionToolbarCommand::Format(format);
        match format {
            InlineFormat::Bold => button
                .font_weight(FontWeight::BOLD)
                .child("B")
                .on_click(cx.listener(move |editor, _event, _window, cx| {
                    editor.run_selection_toolbar_command(command, cx);
                }))
                .into_any_element(),
            InlineFormat::Italic => button
                .italic()
                .child("I")
                .on_click(cx.listener(move |editor, _event, _window, cx| {
                    editor.run_selection_toolbar_command(command, cx);
                }))
                .into_any_element(),
            InlineFormat::Underline => button
                .underline()
                .child("U")
                .on_click(cx.listener(move |editor, _event, _window, cx| {
                    editor.run_selection_toolbar_command(command, cx);
                }))
                .into_any_element(),
            InlineFormat::Strikethrough => button
                .relative()
                .child("S")
                .child(
                    div()
                        .absolute()
                        .left(px(5.0))
                        .right(px(5.0))
                        .top(px(BUTTON_SIZE * 0.5))
                        .h(px(1.0))
                        .bg(text_color),
                )
                .on_click(cx.listener(move |editor, _event, _window, cx| {
                    editor.run_selection_toolbar_command(command, cx);
                }))
                .into_any_element(),
            InlineFormat::Code => button
                .child("</>")
                .on_click(cx.listener(move |editor, _event, _window, cx| {
                    editor.run_selection_toolbar_command(command, cx);
                }))
                .into_any_element(),
            InlineFormat::Highlight => button
                .child(
                    div()
                        .px(px(3.0))
                        .rounded(px(3.0))
                        .bg(c.comment_bg)
                        .text_color(text_color)
                        .child("A"),
                )
                .on_click(cx.listener(move |editor, _event, _window, cx| {
                    editor.run_selection_toolbar_command(command, cx);
                }))
                .into_any_element(),
            InlineFormat::Superscript | InlineFormat::Subscript => button.into_any_element(),
        }
    }

    /// 链接那颗方形按钮。本仓没有图标字体，按钮上的字就是它写下的写法（`[]()`），
    /// 与 `</>` 那颗同一个口径；字号收小一档才塞得进 26 的方格。
    fn link_button(
        &self,
        strings: &crate::i18n::I18nStrings,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let menu_command = DocumentMenuCommand::Link;
        let label = document_menu_label(menu_command, strings);
        let tooltip = match document_menu_shortcut(menu_command) {
            Some(shortcut) => format!("{label}  {shortcut}"),
            None => label,
        };
        div()
            .id("toolbar-link")
            .w(px(BUTTON_SIZE))
            .h(px(BUTTON_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(d.menu_item_radius))
            .bg(c.dialog_surface)
            .text_size(px(10.0))
            .text_color(c.dialog_secondary_button_text)
            .cursor_pointer()
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .active(|this| this.opacity(0.92))
            .debug_selector(|| "toolbar-link".to_string())
            .tooltip(move |_, cx| {
                cx.new(|_| HoverPreviewTooltip {
                    label: tooltip.clone().into(),
                })
                .into()
            })
            .child("[]()")
            .on_click(cx.listener(|editor, _event, _window, cx| {
                editor.run_selection_toolbar_command(SelectionToolbarCommand::Link, cx);
            }))
            .into_any_element()
    }

    /// 标题档位列表：与右键菜单段落那一档同一份行数据、同一条派发路径。
    /// `top` 是相对工具栏面板的偏移，所以它随工具栏一起摆放。
    fn heading_menu_panel(
        &self,
        theme: &Theme,
        strings: &crate::i18n::I18nStrings,
        rows: &[DocumentMenuRow],
        width: Pixels,
        top: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let d = &theme.dimensions;
        let c = &theme.colors;
        div()
            .id("editor-toolbar-heading-menu")
            .absolute()
            .left(px(0.0))
            .top(top)
            .w(width)
            .p(px(d.menu_panel_padding))
            .flex()
            .flex_col()
            .gap(px(d.menu_panel_gap))
            .occlude()
            .bg(c.dialog_surface)
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .rounded(px(d.menu_panel_radius))
            .shadow_lg()
            .debug_selector(|| "editor-toolbar-heading-menu".to_string())
            .children(rows.iter().map(|row| match row {
                DocumentMenuRow::Separator => {
                    crate::components::menu::menu_separator(theme).into_any_element()
                }
                DocumentMenuRow::Item {
                    command,
                    name,
                    enabled,
                } => {
                    let Some(target) = command.as_block_target() else {
                        return div().into_any_element();
                    };
                    let action = SelectionToolbarCommand::Paragraph(target);
                    let row = menu_item(
                        theme,
                        *name,
                        document_menu_label(*command, strings),
                        None,
                        *enabled,
                        false,
                        false,
                        false,
                    );
                    if *enabled {
                        row.on_click(cx.listener(move |editor, _event, _window, cx| {
                            editor.run_selection_toolbar_command(action, cx);
                        }))
                        .into_any_element()
                    } else {
                        row.into_any_element()
                    }
                }
                DocumentMenuRow::Submenu { .. } => div().into_any_element(),
            }))
            .into_any_element()
    }

    /// 点工具栏上的一颗按钮：与右键菜单、快捷键走同一条实现。
    pub(crate) fn run_selection_toolbar_command(
        &mut self,
        command: SelectionToolbarCommand,
        cx: &mut Context<Self>,
    ) {
        match command {
            SelectionToolbarCommand::Format(format) => {
                self.toggle_inline_format_on_selection(format, cx);
            }
            SelectionToolbarCommand::Link => {
                self.insert_link_on_selection(cx);
            }
            SelectionToolbarCommand::Paragraph(target) => {
                self.apply_block_kind_to_selection(target, cx);
                self.close_selection_toolbar_heading_menu(cx);
            }
        }
    }

    fn close_selection_toolbar_heading_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.selection_toolbar.as_mut() {
            state.heading_menu_open = false;
        }
        cx.notify();
    }
}
