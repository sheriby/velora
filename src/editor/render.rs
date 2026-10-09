//! Editor window rendering: centered scrollable block column,
//! unsaved-changes overlay dialog, custom scrollbar, and deferred
//! operations (focus, scroll, save, window title).

use std::time::Instant;

pub(super) use gpui::*;

pub(super) use super::{Editor, InfoDialogKind, MountedRun, tree::VisibleTreeAnchors};
pub(super) use crate::app_menu::{dispatch_menu_action_for_editor, welcome_recent_entries};
pub(super) use crate::components::CalloutVariant;
pub(super) use crate::components::{AddLanguageConfig, AddThemeConfig, Block, BlockKind, NoRecentFiles};
pub(super) use crate::i18n::{I18nManager, I18nStrings};
pub(super) use crate::theme::{Theme, ThemeDimensions, ThemeManager};
pub(super) use crate::window_chrome::{
    TITLEBAR_MENU_ICON, TITLEBAR_MENU_ICON_SIZE_PX, custom_titlebar_height,
    custom_titlebar_icon_color, render_custom_titlebar,
};

pub(crate) const ABOUT_GITHUB_URL: &str = "https://github.com/sheriby/velora";

/// Rows within this many pixels of the viewport stay mounted, so a fast flick
/// paints them before they scroll in instead of showing a blank edge.
const RENDER_OVERDRAW_PX: f32 = 800.0;

/// 冷启动续挂的帧数上限：行高被低估时一帧挂不满视口，最多再排这么多帧，
/// 避免估不准时每帧重排。8 帧 ≈ 130ms。
const COLD_FILL_MAX_FRAMES: u8 = 8;

pub(crate) fn open_about_github_url(cx: &mut App) {
    cx.open_url(ABOUT_GITHUB_URL);
}

fn editor_text_font(family: &str) -> Font {
    // FontFallbacks is internally `Arc<Vec<String>>` — building it once
    // per process and Arc-cloning per render is the right shape, since
    // editor_text_font() is called from Editor::render on every frame.
    static FALLBACKS: std::sync::OnceLock<FontFallbacks> = std::sync::OnceLock::new();
    let fallbacks = FALLBACKS
        .get_or_init(|| {
            FontFallbacks::from_fonts(tibetan_font_fallbacks_for_target_os(std::env::consts::OS))
        })
        .clone();
    let mut font = font(family.to_string());
    font.fallbacks = Some(fallbacks);
    font
}

fn tibetan_font_fallbacks_for_target_os(target_os: &str) -> Vec<String> {
    let families = match target_os {
        "windows" => &[
            "Microsoft Himalaya",
            "Noto Serif Tibetan",
            "Noto Sans Tibetan",
            "BabelStone Tibetan",
        ][..],
        "macos" => &["Kailasa", "Noto Serif Tibetan", "Noto Sans Tibetan"][..],
        _ => &[
            "Noto Serif Tibetan",
            "Noto Sans Tibetan",
            "Microsoft Himalaya",
            "Kailasa",
            "BabelStone Tibetan",
        ][..],
    };
    families
        .iter()
        .map(|family| (*family).to_string())
        .collect()
}

/// Adjacent-row metadata used to collapse spacing inside visual groups.
///
/// 同一份数据也是折叠过滤的廉价闸门：`heading_level` 决定要不要读实体看折叠状态，
/// `is_toc` / `had_toc` 决定要不要为一个 `[TOC]` 块算目录条目。可见列表同步时
/// 顺手记下（见 `DocumentTree::sync_block_list`），行计划重建就不再逐块读实体。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RenderedRowSpacingInfo {
    pub(crate) quote_group_anchor: Option<uuid::Uuid>,
    pub(crate) visible_quote_group_anchor: Option<uuid::Uuid>,
    pub(crate) callout_anchor: Option<uuid::Uuid>,
    pub(crate) callout_variant: Option<CalloutVariant>,
    pub(crate) is_callout_header: bool,
    pub(crate) footnote_anchor: Option<uuid::Uuid>,
    pub(crate) is_footnote_header: bool,
    pub(crate) heading_level: Option<u8>,
    pub(crate) is_list_item: bool,
    /// 这一块的显示文本是不是 `[TOC]`（同步那趟从块上读出来的）。
    pub(crate) is_toc: bool,
    /// 这一块手上是不是挂着目录条目（上一趟 `[TOC]` 留下的，需要清掉）。
    pub(crate) had_toc: bool,
}

impl RenderedRowSpacingInfo {
    /// 由可见列表同步那一趟刚算出来的锚点构造。字段取自**本次**计算而不是块上
    /// 的旧值——同一趟里旧值还没写回块。
    pub(crate) fn from_visible_tree_sync(
        kind: &BlockKind,
        anchors: VisibleTreeAnchors,
        is_toc: bool,
        had_toc: bool,
    ) -> Self {
        Self {
            quote_group_anchor: anchors.quote_group_anchor,
            visible_quote_group_anchor: anchors.visible_quote_group_anchor,
            callout_anchor: anchors.callout_anchor,
            callout_variant: anchors.callout_variant,
            is_callout_header: kind.is_callout(),
            footnote_anchor: anchors.footnote_anchor,
            is_footnote_header: kind.is_footnote_definition(),
            heading_level: match kind {
                BlockKind::Heading { level } => Some(*level),
                _ => None,
            },
            is_list_item: kind.is_list_item(),
            is_toc,
            had_toc,
        }
    }
}

/// P4b：行结构计划——折叠过滤与分组扫描的一次性产物，按
/// (行元数据版本, fold_state_version, toc 版本, 渲染模式) 缓存。未变更帧直接
/// 复用，不再对全文档做逐块实体读取；元素只在行真正挂载时构建。
/// 行元数据版本见 [`DocumentTree::row_meta_version`](crate::editor::tree)。
pub(crate) struct RenderedRowPlan {
    pub row_meta_version: u64,
    pub fold_version: u64,
    pub toc_version: u64,
    pub rendered_mode: bool,
    pub block_gap: f32,
    /// 可见块数：渐进导入（G8）每步只 append + notify，不推进
    /// 行元数据版本（append 会推进）——键里必须带块数，续建出的新块才会渲染。
    pub visible_len: usize,
    pub rows: Vec<RenderedRowPlanRow>,
    /// P7：行元数据预计算（普通行距/起始下标/行首 id），未变更帧零重算。
    pub visible_starts: Vec<usize>,
    pub gaps: Vec<f32>,
    pub first_ids: Vec<EntityId>,
    /// 每行 footprint；学习更新原地写回，未变更帧免 160k 次哈希查找。
    pub strides: std::rc::Rc<std::cell::RefCell<Vec<f32>>>,
    /// 滚动条坐标使用固定的行权重；首次测量行高只改变布局，不改变文档进度。
    pub scrollbar_strides: Vec<f32>,
}

pub(crate) struct RenderedRowPlanRow {
    /// 行首块在可见块序列中的下标（透明度/窗口计算用）。
    pub visible_start: usize,
    pub first_id: EntityId,
    pub body: RenderedRowBody,
}

pub(crate) enum RenderedRowBody {
    Ordinary {
        entity: Entity<Block>,
        spacing: RenderedRowSpacingInfo,
    },
    /// Callout 组（callout_variant = Some）或独立脚注组（None）。成员自带
    /// 行距信息与锚点，元素构建推迟到行挂载时。
    Group {
        callout_variant: Option<CalloutVariant>,
        members: Vec<RenderedGroupMember>,
    },
}

pub(crate) struct RenderedGroupMember {
    pub entity: Entity<Block>,
    pub spacing: RenderedRowSpacingInfo,
}

impl RenderedRowPlanRow {
    fn first_spacing(&self) -> RenderedRowSpacingInfo {
        match &self.body {
            RenderedRowBody::Ordinary { spacing, .. } => *spacing,
            RenderedRowBody::Group { members, .. } => members[0].spacing,
        }
    }

    fn last_spacing(&self) -> RenderedRowSpacingInfo {
        match &self.body {
            RenderedRowBody::Ordinary { spacing, .. } => *spacing,
            RenderedRowBody::Group { members, .. } => members
                .last()
                .map(|member| member.spacing)
                .unwrap_or_default(),
        }
    }
}

fn rendered_row_top_gap(
    previous: Option<RenderedRowSpacingInfo>,
    current: RenderedRowSpacingInfo,
    default_gap: f32,
    rendered_mode: bool,
) -> f32 {
    let Some(previous) = previous else {
        return 0.0;
    };

    if previous.quote_group_anchor.is_some()
        && previous.quote_group_anchor == current.quote_group_anchor
    {
        return 0.0;
    }
    if !rendered_mode {
        // 源码模式的根块是缓冲区的连续切片：块边界就是一个换行，加任何段间距
        // 都会让行距变成「行高 + gap」，看着像同一份文本被撑开（用户报修）。
        return 0.0;
    }

    if let Some(level) = current.heading_level {
        default_gap
            * match level {
                1 => 2.4,
                2 => 1.9,
                3 => 1.5,
                _ => 1.25,
            }
    } else if previous.heading_level.is_some() {
        default_gap * 0.75
    } else if previous.is_list_item && current.is_list_item {
        default_gap * 0.5
    } else {
        default_gap
    }
}

fn focus_mode_row_opacity(
    active: bool,
    focused_visible_index: Option<usize>,
    start: usize,
    end: usize,
) -> f32 {
    if !active || focused_visible_index.is_none_or(|index| (start..end).contains(&index)) {
        1.0
    } else {
        0.38
    }
}

fn typewriter_target_scroll_offset(
    current_offset: f32,
    viewport_center: f32,
    caret_center: f32,
    max_offset: f32,
) -> f32 {
    (current_offset + viewport_center - caret_center).clamp(-max_offset.max(0.0), 0.0)
}

fn callout_colors(variant: CalloutVariant, theme: &Theme) -> (Hsla, Hsla) {
    let c = &theme.colors;
    match variant {
        CalloutVariant::Note => (c.callout_note_border, c.callout_note_bg),
        CalloutVariant::Tip => (c.callout_tip_border, c.callout_tip_bg),
        CalloutVariant::Important => (c.callout_important_border, c.callout_important_bg),
        CalloutVariant::Warning => (c.callout_warning_border, c.callout_warning_bg),
        CalloutVariant::Caution => (c.callout_caution_border, c.callout_caution_bg),
    }
}

fn callout_row_top_gap(
    previous: Option<RenderedRowSpacingInfo>,
    current: RenderedRowSpacingInfo,
    dimensions: &ThemeDimensions,
) -> f32 {
    let Some(previous) = previous else {
        return 0.0;
    };

    if previous.visible_quote_group_anchor.is_some()
        && previous.visible_quote_group_anchor == current.visible_quote_group_anchor
    {
        return 0.0;
    }

    if previous.is_callout_header {
        dimensions.callout_header_margin_bottom
    } else {
        dimensions.callout_body_gap
    }
}

fn footnote_row_top_gap(previous: Option<RenderedRowSpacingInfo>, default_gap: f32) -> f32 {
    let Some(previous) = previous else {
        return 0.0;
    };

    if previous.is_footnote_header {
        default_gap * 0.75
    } else {
        default_gap
    }
}

fn is_wide_menu_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11ff
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe10..=0xfe6f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
    )
}

pub(crate) fn estimated_menu_label_width(label: &str, text_size: f32) -> f32 {
    label
        .chars()
        .map(|ch| {
            if ch.is_ascii_whitespace() {
                text_size * 0.35
            } else if ch.is_ascii_punctuation() {
                text_size * 0.45
            } else if ch.is_ascii() {
                text_size * 0.62
            } else if is_wide_menu_char(ch) {
                // 全角字的实际前进宽度比字号略大（回落字体与逐字取整都会多出来一点），
                // 按整 1em 估会短半个字，中文菜单最后一行就被 `.truncate()` 切掉一角。
                text_size * 1.06
            } else {
                text_size * 0.85
            }
        })
        .sum()
}

fn menu_bar_button_width(label: &str, dimensions: &ThemeDimensions) -> f32 {
    let content_width = estimated_menu_label_width(label, dimensions.menu_text_size)
        + dimensions.menu_bar_button_padding_x * 2.0;
    dimensions.menu_bar_button_width.max(content_width.ceil())
}

fn supports_in_window_menu_for_target_os(target_os: &str) -> bool {
    target_os != "macos"
}

fn supports_in_window_menu() -> bool {
    supports_in_window_menu_for_target_os(std::env::consts::OS)
}

/// 是否单独渲染那一行菜单栏（文件/导出/语言/主题）。Windows 上不渲染：改用标题栏
/// 左侧的汉堡按钮 + 竖列菜单，省出一整行高度。
fn supports_menu_bar_row_for_target_os(target_os: &str) -> bool {
    supports_in_window_menu_for_target_os(target_os) && target_os != "windows"
}

fn supports_menu_bar_row() -> bool {
    supports_menu_bar_row_for_target_os(std::env::consts::OS)
}

/// 一级菜单是否由标题栏的汉堡按钮承载（目前只有 Windows）。
fn supports_hamburger_menu_for_target_os(target_os: &str) -> bool {
    supports_in_window_menu_for_target_os(target_os) && target_os == "windows"
}

fn supports_hamburger_menu() -> bool {
    supports_hamburger_menu_for_target_os(std::env::consts::OS)
}

fn in_window_menu_bar_height_for_target_os(
    target_os: &str,
    has_menus: bool,
    dimensions: &ThemeDimensions,
) -> f32 {
    if has_menus && supports_menu_bar_row_for_target_os(target_os) {
        dimensions.menu_bar_height
    } else {
        0.0
    }
}

fn menu_panel_left<S: AsRef<str>>(
    open_index: usize,
    menu_labels: &[S],
    dimensions: &ThemeDimensions,
) -> f32 {
    let prior_width: f32 = menu_labels
        .iter()
        .take(open_index)
        .map(|label| menu_bar_button_width(label.as_ref(), dimensions))
        .sum();
    dimensions.menu_bar_padding_x + prior_width + dimensions.menu_bar_gap * open_index as f32
}

fn menu_panel_width_for_labels<S: AsRef<str>>(labels: &[S], dimensions: &ThemeDimensions) -> f32 {
    let widest_label = labels
        .iter()
        .map(|label| estimated_menu_label_width(label.as_ref(), dimensions.menu_text_size))
        .fold(0.0, f32::max);
    let content_width = widest_label + dimensions.menu_item_padding_x * 2.0;
    dimensions.menu_panel_width.max(content_width.ceil())
}

fn owned_menu_item_labels(items: &[OwnedMenuItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| match item {
            OwnedMenuItem::Action { name, .. } => Some(name.to_string()),
            OwnedMenuItem::Submenu(menu) => Some(menu.name.to_string()),
            OwnedMenuItem::SystemMenu(menu) => Some(menu.name.to_string()),
            OwnedMenuItem::Separator => None,
        })
        .collect()
}

fn menu_item_visual_height(item: &OwnedMenuItem, dimensions: &ThemeDimensions) -> f32 {
    match item {
        OwnedMenuItem::Separator => {
            dimensions.menu_separator_height + dimensions.menu_separator_margin_y * 2.0
        }
        OwnedMenuItem::Action { .. } | OwnedMenuItem::Submenu(_) | OwnedMenuItem::SystemMenu(_) => {
            dimensions.menu_item_height
        }
    }
}

const SCROLLABLE_IMPORT_MENU_VISIBLE_ITEMS: usize = 12;

fn menu_items_visual_height_with_gaps(
    items: &[OwnedMenuItem],
    dimensions: &ThemeDimensions,
) -> f32 {
    if items.is_empty() {
        return 0.0;
    }

    let items_height: f32 = items
        .iter()
        .map(|item| menu_item_visual_height(item, dimensions))
        .sum();
    items_height + dimensions.menu_panel_gap * items.len().saturating_sub(1) as f32
}

fn import_menu_split_index(items: &[OwnedMenuItem]) -> Option<usize> {
    let [
        prefix @ ..,
        OwnedMenuItem::Separator,
        OwnedMenuItem::Action { action, .. },
    ] = items
    else {
        return None;
    };

    if action.as_ref().as_any().is::<AddThemeConfig>()
        || action.as_ref().as_any().is::<AddLanguageConfig>()
    {
        Some(prefix.len())
    } else {
        None
    }
}

fn scrollable_import_menu_scroll_height(
    scroll_items: &[OwnedMenuItem],
    footer_items: &[OwnedMenuItem],
    viewport_height: f32,
    top_offset: f32,
    dimensions: &ThemeDimensions,
) -> f32 {
    let visible_count = scroll_items.len().min(SCROLLABLE_IMPORT_MENU_VISIBLE_ITEMS);
    if visible_count == 0 {
        return 0.0;
    }

    let default_height =
        menu_items_visual_height_with_gaps(&scroll_items[..visible_count], dimensions);
    let footer_height = menu_items_visual_height_with_gaps(footer_items, dimensions);
    let footer_gap = if footer_items.is_empty() {
        0.0
    } else {
        dimensions.menu_panel_gap
    };
    let available_height = viewport_height
        - top_offset
        - dimensions.menu_panel_top
        - dimensions.menu_panel_padding * 2.0
        - footer_height
        - footer_gap
        - 8.0;
    let min_height = dimensions.menu_item_height.min(default_height).max(1.0);

    default_height.min(available_height.max(min_height))
}

fn submenu_panel_top(
    items: &[OwnedMenuItem],
    item_index: usize,
    dimensions: &ThemeDimensions,
) -> f32 {
    let prior_items_height: f32 = items
        .iter()
        .take(item_index)
        .map(|item| menu_item_visual_height(item, dimensions))
        .sum();
    let prior_gaps = dimensions.menu_panel_gap * item_index as f32;
    dimensions.menu_panel_top + dimensions.menu_panel_padding + prior_items_height + prior_gaps
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MenuSubmenuBridgeGeometry {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

fn submenu_bridge_geometry<T: AsRef<str>>(
    main_panel_left: f32,
    items: &[OwnedMenuItem],
    item_index: usize,
    submenu_labels: &[T],
    dimensions: &ThemeDimensions,
) -> Option<MenuSubmenuBridgeGeometry> {
    let item = items.get(item_index)?;
    let main_panel_width = menu_panel_width_for_labels(&owned_menu_item_labels(items), dimensions);
    let submenu_width = menu_panel_width_for_labels(submenu_labels, dimensions);
    let vertical_tolerance = dimensions.menu_panel_padding + dimensions.menu_panel_gap;
    let item_top = submenu_panel_top(items, item_index, dimensions);
    let top = (item_top - vertical_tolerance).max(dimensions.menu_panel_top);
    Some(MenuSubmenuBridgeGeometry {
        left: main_panel_left + main_panel_width,
        top,
        width: dimensions.menu_panel_gap + submenu_width,
        height: menu_item_visual_height(item, dimensions) + vertical_tolerance * 2.0,
    })
}

/// 菜单面板的锚点（窗口坐标）：一级面板画在哪。横向菜单栏模式下它由按钮位置算出，
/// 汉堡模式下由列表宽度与被划过的行算出。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MenuPanelOrigin {
    panel_left: f32,
    panel_top: f32,
}

/// 汉堡列表面板的顶端 y：紧紧跟在按钮下沿后面（默认主题：按钮底 30 + 2 = 32px）。
///
/// 不能用 `menu_panel_top`（默认 30px）——那是给「菜单栏那一行」算偏移用的，
/// 从标题栏下沿再加 30px 就是白白空一大截（用户报的那个间距）。
/// 也不能用「标题栏下沿」：标题栏比按钮高时，空出来的部分全变成图标与列表之间的缝。
/// 这里只跟按钮高度和一个 gap 有关，主题把标题栏调多高都不会拉开。
fn hamburger_menu_panel_top(titlebar_height: f32, dimensions: &ThemeDimensions) -> f32 {
    (titlebar_height + dimensions.menu_bar_button_height) / 2.0 + dimensions.menu_bar_gap
}

/// 汉堡列表里第 `index` 行的顶端 y。行高与间距跟条目面板里的行保持一致
/// （`menu_item_height` + `menu_panel_gap`），这样展开的条目面板能跟被划过的行对齐。
fn hamburger_menu_row_top(
    index: usize,
    titlebar_height: f32,
    dimensions: &ThemeDimensions,
) -> f32 {
    hamburger_menu_panel_top(titlebar_height, dimensions)
        + dimensions.menu_panel_padding
        + index as f32 * (dimensions.menu_item_height + dimensions.menu_panel_gap)
}

/// 汉堡模式下，第 `index` 个菜单的条目面板该画在哪：贴在列表右侧、与它那一行对齐。
fn hamburger_menu_item_panel_origin<S: AsRef<str>>(
    index: usize,
    titlebar_height: f32,
    menu_labels: &[S],
    dimensions: &ThemeDimensions,
) -> MenuPanelOrigin {
    let list_width = menu_panel_width_for_labels(menu_labels, dimensions);
    MenuPanelOrigin {
        panel_left: dimensions.menu_bar_padding_x + list_width + dimensions.menu_panel_gap,
        // 面板自身的 padding 与 menu_panel_top 要和行的位置对消，让第一行正好落在
        // 被划过的列表行上（面板绘制 y = panel_top + menu_panel_top，内容再 + padding）。
        panel_top: hamburger_menu_row_top(index, titlebar_height, dimensions)
            - dimensions.menu_panel_padding
            - dimensions.menu_panel_top,
    }
}

fn footnote_group_shell(
    children: Vec<AnyElement>,
    theme: &Theme,
    dimensions: &ThemeDimensions,
) -> AnyElement {
    div()
        .w_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(0.0))
        .px(px(dimensions.footnote_padding_x))
        .py(px(dimensions.footnote_padding_y))
        .rounded(px(dimensions.footnote_radius))
        .border(px(1.0))
        .border_color(theme.colors.footnote_border)
        .bg(theme.colors.footnote_bg)
        .children(children)
        .into_any_element()
}

impl Editor {
    /// 计数器：行计划重建路上每读一个块实体加一。闸门看的就是它——见
    /// `row_plan_rebuild_reads_only_the_headings_not_every_block`。
    pub(crate) fn count_row_plan_block_read(&self) {
        self.row_plan_block_reads
            .set(self.row_plan_block_reads.get() + 1);
    }

    /// P4b：对折叠过滤剩下的那段可见序列做一次分组扫描，产出可复用的行计划。
    /// `kept` 是过滤后的可见下标（升序）。行元数据全部取自同步可见列表那一趟
    /// 记下的快照，所以这里不读任何块实体；`rows` 只在真正需要挂元素时才克隆实体。
    fn build_rendered_row_plan(
        &self,
        kept: &[u32],
        row_meta_version: u64,
        fold_version: u64,
        toc_version: u64,
        rendered_mode: bool,
        block_gap: f32,
        visible_len: usize,
        cx: &mut Context<Self>,
    ) -> RenderedRowPlan {
        let estimate = cx
            .global::<crate::theme::ThemeManager>()
            .current_arc()
            .dimensions
            .block_min_height
            .max(1.0);
        let spacing_of = |index: usize| -> RenderedRowSpacingInfo {
            self.document.row_spacing_at(index)
        };
        let entity_of = |index: usize| self.document.visible_blocks()[index].entity.clone();
        let mut rows: Vec<RenderedRowPlanRow> = Vec::with_capacity(kept.len());
        let mut position = 0usize;
        while position < kept.len() {
            let first_index = kept[position] as usize;
            let first_spacing = spacing_of(first_index);
            let first_id = self.document.visible_blocks()[first_index]
                .entity
                .entity_id();
            if let (Some(callout_anchor), Some(callout_variant)) = (
                first_spacing.callout_anchor,
                first_spacing.callout_variant,
            ) {
                let mut members = Vec::new();
                let mut group_end = position;
                while group_end < kept.len()
                    && spacing_of(kept[group_end] as usize).callout_anchor
                        == Some(callout_anchor)
                {
                    let index = kept[group_end] as usize;
                    members.push(RenderedGroupMember {
                        entity: entity_of(index),
                        spacing: spacing_of(index),
                    });
                    group_end += 1;
                }
                rows.push(RenderedRowPlanRow {
                    visible_start: position,
                    first_id,
                    body: RenderedRowBody::Group {
                        callout_variant: Some(callout_variant),
                        members,
                    },
                });
                position = group_end;
                continue;
            }

            if let Some(footnote_anchor) = first_spacing.footnote_anchor {
                let mut members = Vec::new();
                let mut group_end = position;
                while group_end < kept.len()
                    && spacing_of(kept[group_end] as usize).footnote_anchor
                        == Some(footnote_anchor)
                {
                    let index = kept[group_end] as usize;
                    members.push(RenderedGroupMember {
                        entity: entity_of(index),
                        spacing: spacing_of(index),
                    });
                    group_end += 1;
                }
                rows.push(RenderedRowPlanRow {
                    visible_start: position,
                    first_id,
                    body: RenderedRowBody::Group {
                        callout_variant: None,
                        members,
                    },
                });
                position = group_end;
                continue;
            }

            rows.push(RenderedRowPlanRow {
                visible_start: position,
                first_id,
                body: RenderedRowBody::Ordinary {
                    entity: entity_of(first_index),
                    spacing: first_spacing,
                },
            });
            position += 1;
        }

        // P7：行元数据与 stride 初值在构建期一次算好，未变更帧零重算。
        let row_count = rows.len();
        let mut visible_starts = Vec::with_capacity(row_count);
        let mut gaps = Vec::with_capacity(row_count);
        let mut first_ids = Vec::with_capacity(row_count);
        let mut previous_row_spacing = None;
        for row in &rows {
            visible_starts.push(row.visible_start);
            first_ids.push(row.first_id);
            let first_spacing = row.first_spacing();
            gaps.push(rendered_row_top_gap(
                previous_row_spacing,
                first_spacing,
                block_gap,
                rendered_mode,
            ));
            previous_row_spacing = Some(row.last_spacing());
        }
        let strides = rows
            .iter()
            .map(|row| self.row_stride_cache.get(&row.first_id).copied())
            .collect::<Vec<Option<f32>>>();
        let strides = std::rc::Rc::new(std::cell::RefCell::new(
            strides
                .into_iter()
                .map(|stride| stride.unwrap_or(estimate))
                .collect::<Vec<f32>>(),
        ));

        let scrollbar_strides = strides.borrow().clone();
        RenderedRowPlan {
            row_meta_version,
            fold_version,
            toc_version,
            rendered_mode,
            block_gap,
            visible_len,
            rows,
            visible_starts,
            gaps,
            first_ids,
            strides,
            scrollbar_strides,
        }
    }
}

mod overlays;
mod menu_render;
mod paint;
mod sync;

#[cfg(test)]
mod tests;
