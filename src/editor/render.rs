//! Editor window rendering: centered scrollable block column,
//! unsaved-changes overlay dialog, custom scrollbar, and deferred
//! operations (focus, scroll, save, window title).

use std::time::{Duration, Instant};

use gpui::*;

use super::{Editor, InfoDialogKind, MountedRun};
use crate::app_menu::{dispatch_menu_action_for_editor, welcome_recent_entries};
use crate::components::CalloutVariant;
use crate::components::{AddLanguageConfig, AddThemeConfig, Block, BlockKind, NoRecentFiles};
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::{Theme, ThemeDimensions, ThemeManager};
use crate::window_chrome::{
    TITLEBAR_MENU_ICON, TITLEBAR_MENU_ICON_SIZE_PX, custom_titlebar_height,
    custom_titlebar_icon_color, render_custom_titlebar,
};

pub(crate) const ABOUT_GITHUB_URL: &str = "https://github.com/sheriby/velora";

/// Rows within this many pixels of the viewport stay mounted, so a fast flick
/// paints them before they scroll in instead of showing a blank edge.
const RENDER_OVERDRAW_PX: f32 = 800.0;
/// 侧边栏收起后，贴住窗口左边缘多宽就算「想唤出侧边栏」。
const SIDEBAR_AUTO_HIDE_EDGE_PX: f32 = 15.0;
/// 活动栏（窄条）宽度，和 `render_activity_rail` 里的容器一致。
const SIDEBAR_RAIL_WIDTH_PX: f32 = 50.0;
/// 唤出滑入 + 收回滑出共用的动画时长；workspace.rs 的收回定时器用同一值
/// 收尾卸载。
pub(super) const SIDEBAR_SLIDE_DURATION: Duration = Duration::from_millis(350);
/// 贴边后必须停留满这段时长才唤出浮层：扫过左缘不停留不弹，防误触。
pub(super) const SIDEBAR_PEEK_DWELL: Duration = Duration::from_millis(300);

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RenderedRowSpacingInfo {
    quote_group_anchor: Option<uuid::Uuid>,
    visible_quote_group_anchor: Option<uuid::Uuid>,
    callout_anchor: Option<uuid::Uuid>,
    callout_variant: Option<CalloutVariant>,
    is_callout_header: bool,
    footnote_anchor: Option<uuid::Uuid>,
    is_footnote_header: bool,
    heading_level: Option<u8>,
    is_list_item: bool,
}

impl RenderedRowSpacingInfo {
    fn from_block(block: &Block) -> Self {
        let kind = block.kind();
        Self {
            quote_group_anchor: block.quote_group_anchor,
            visible_quote_group_anchor: block.visible_quote_group_anchor,
            callout_anchor: block.callout_anchor,
            callout_variant: block.callout_variant,
            is_callout_header: kind.is_callout(),
            footnote_anchor: block.footnote_anchor,
            is_footnote_header: kind.is_footnote_definition(),
            heading_level: match &kind {
                BlockKind::Heading { level } => Some(*level),
                _ => None,
            },
            is_list_item: kind.is_list_item(),
        }
    }
}

/// P4b：行结构计划——折叠过滤与分组扫描的一次性产物，按
/// (document_revision, fold_state_version, 渲染模式) 缓存。未变更帧直接
/// 复用，不再对全文档做逐块实体读取；元素只在行真正挂载时构建。
pub(crate) struct RenderedRowPlan {
    pub revision: u64,
    pub fold_version: u64,
    pub toc_version: u64,
    pub rendered_mode: bool,
    pub block_gap: f32,
    /// 可见块数：渐进导入（G8）每步只 append + notify，不推进
    /// document_revision——键里必须带块数，续建出的新块才会渲染。
    pub visible_len: usize,
    pub rows: Vec<RenderedRowPlanRow>,
    /// P7：行元数据预计算（普通行距/起始下标/行首 id），未变更帧零重算。
    pub visible_starts: Vec<usize>,
    pub gaps: Vec<f32>,
    pub first_ids: Vec<EntityId>,
    /// 每行 footprint；学习更新原地写回，未变更帧免 160k 次哈希查找。
    pub strides: std::rc::Rc<std::cell::RefCell<Vec<f32>>>,
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
        return default_gap;
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

fn estimated_menu_label_width(label: &str, text_size: f32) -> f32 {
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
                text_size
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
struct MenuPanelOrigin {
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
    /// P4b：对可见块序列做一次折叠过滤后的分组扫描，产出可复用的行计划。
    /// 只读块元数据，不构建任何元素。
    fn build_rendered_row_plan(
        &self,
        visible: &[super::tree::VisibleBlock],
        revision: u64,
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
        let spacing_of = |visible: &super::tree::VisibleBlock| -> RenderedRowSpacingInfo {
            RenderedRowSpacingInfo::from_block(visible.entity.read(cx))
        };
        let mut rows: Vec<RenderedRowPlanRow> = Vec::with_capacity(visible.len());
        let mut index = 0usize;
        while index < visible.len() {
            let first_spacing = spacing_of(&visible[index]);
            let first_id = visible[index].entity.entity_id();
            if let (Some(callout_anchor), Some(callout_variant)) = (
                first_spacing.callout_anchor,
                first_spacing.callout_variant,
            ) {
                let mut members = Vec::new();
                let mut group_end = index;
                while group_end < visible.len()
                    && spacing_of(&visible[group_end]).callout_anchor == Some(callout_anchor)
                {
                    members.push(RenderedGroupMember {
                        entity: visible[group_end].entity.clone(),
                        spacing: spacing_of(&visible[group_end]),
                    });
                    group_end += 1;
                }
                rows.push(RenderedRowPlanRow {
                    visible_start: index,
                    first_id,
                    body: RenderedRowBody::Group {
                        callout_variant: Some(callout_variant),
                        members,
                    },
                });
                index = group_end;
                continue;
            }

            if let Some(footnote_anchor) = first_spacing.footnote_anchor {
                let mut members = Vec::new();
                let mut group_end = index;
                while group_end < visible.len()
                    && spacing_of(&visible[group_end]).footnote_anchor == Some(footnote_anchor)
                {
                    members.push(RenderedGroupMember {
                        entity: visible[group_end].entity.clone(),
                        spacing: spacing_of(&visible[group_end]),
                    });
                    group_end += 1;
                }
                rows.push(RenderedRowPlanRow {
                    visible_start: index,
                    first_id,
                    body: RenderedRowBody::Group {
                        callout_variant: None,
                        members,
                    },
                });
                index = group_end;
                continue;
            }

            rows.push(RenderedRowPlanRow {
                visible_start: index,
                first_id,
                body: RenderedRowBody::Ordinary {
                    entity: visible[index].entity.clone(),
                    spacing: first_spacing,
                },
            });
            index += 1;
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

        RenderedRowPlan {
            revision,
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
        }
    }

    /// 状态栏整篇字数
    fn on_titlebar_close(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.standard_click() {
            self.request_close_current_window(window, cx);
        }
    }

    pub(crate) fn install_close_guard(&mut self, cx: &mut Context<Self>, window: &mut Window) {
        if self.close_guard_installed {
            return;
        }

        self.force_install_close_guard(cx, window);
    }

    pub(crate) fn force_install_close_guard(
        &mut self,
        cx: &mut Context<Self>,
        window: &mut Window,
    ) {
        let editor = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            editor
                .update(cx, |this, cx| this.on_window_should_close(window, cx))
                .unwrap_or(true)
        });
        self.close_guard_installed = true;
    }

    fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entity_id) = self.pending_focus.take()
            && let Some(block) = self.focusable_entity_by_id(entity_id)
        {
            block.read(cx).focus_handle.focus(window);
        }
    }

    fn ensure_focused_caret_visible(&mut self, window: &Window, cx: &App) -> bool {
        let Some(focused_block) = self.focused_edit_target(window, cx) else {
            return false;
        };
        let Some(active_bounds) =
            focused_block.read_with(cx, |block, _cx| block.active_range_or_cursor_bounds())
        else {
            return false;
        };

        let viewport = self.scroll_handle.bounds();
        if self.typewriter_mode
            && self.view_mode == super::ViewMode::Rendered
            && !self.code_tab_active()
            && self.cross_block_selection.is_none()
        {
            let mut offset = self.scroll_handle.offset();
            let viewport_center = f32::from(viewport.top()) + f32::from(viewport.size.height) * 0.5;
            let caret_center =
                f32::from(active_bounds.top()) + f32::from(active_bounds.size.height) * 0.5;
            let target = typewriter_target_scroll_offset(
                f32::from(offset.y),
                viewport_center,
                caret_center,
                f32::from(self.scroll_handle.max_offset().height),
            );
            if (target - f32::from(offset.y)).abs() > 0.5 {
                offset.y = px(target);
                self.scroll_handle.set_offset(offset);
            }
            return true;
        }
        // Outline/search jumps land the target at the viewport center. The
        // scroll range already reserves half a viewport past the end, so
        // trailing content can center too; top-of-document clamps to 0.
        if self.pending_scroll_center_into_view {
            let viewport_center = f32::from(viewport.top()) + f32::from(viewport.size.height) * 0.5;
            let target_center =
                f32::from(active_bounds.top()) + f32::from(active_bounds.size.height) * 0.5;
            let mut offset = self.scroll_handle.offset();
            offset.y += px(viewport_center - target_center);
            let max_offset_y = self.scroll_handle.max_offset().height.max(px(0.0));
            offset.y = offset.y.min(px(0.0)).max(-max_offset_y);
            self.scroll_handle.set_offset(offset);
            return true;
        }

        let padding = px(20.0);
        let top_limit = viewport.top() + padding;
        let bottom_limit = viewport.bottom() - padding;
        let mut offset = self.scroll_handle.offset();
        let mut changed = false;

        if active_bounds.top() < top_limit {
            offset.y += top_limit - active_bounds.top();
            changed = true;
        } else if active_bounds.bottom() > bottom_limit {
            offset.y -= active_bounds.bottom() - bottom_limit;
            changed = true;
        }

        if changed {
            let max_offset_y = self.scroll_handle.max_offset().height.max(px(0.0));
            offset.y = offset.y.min(px(0.0)).max(-max_offset_y);
            self.scroll_handle.set_offset(offset);
        }

        true
    }

    fn apply_pending_scroll_into_view(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.scrollbar_drag.is_some() {
            return;
        }

        if !self.pending_scroll_active_block_into_view {
            return;
        }

        // scroll_to_item indexed children by position, which the spacers break;
        // the focused block is always mounted, so pixel math on its bounds works.
        let has_bounds = self.ensure_focused_caret_visible(window, cx);
        if self.pending_scroll_recheck_after_layout {
            self.pending_scroll_recheck_after_layout = false;
            self.schedule_followup_frame(cx);
            return;
        }

        if !has_bounds {
            self.schedule_followup_frame(cx);
            return;
        }

        self.pending_scroll_active_block_into_view = false;
        self.pending_scroll_center_into_view = false;
        self.scroll_recheck_task = None;
    }

    /// Requests a repaint one frame out for work that cannot finish inside this
    /// frame: a scroll-into-view whose target block has no measured bounds yet,
    /// or a cold-start run that still has not covered the viewport. `cx.notify()`
    /// is swallowed when called from within `render`, so without this the retry
    /// would wait for the next external notify (e.g. the cursor blink, ~0.5s
    /// later).
    fn schedule_followup_frame(&mut self, cx: &mut Context<Self>) {
        self.scroll_recheck_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |_this, cx| cx.notify());
        }));
    }

    fn sync_pending_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_save && !self.has_marked_document_text(cx) {
            self.pending_save = false;
            self.save_document(window, cx);
        }
    }

    /// 切换工作区后补开最近剩下的标签（`set_workspace_root` 当时没有 Window）。
    fn sync_pending_workspace_tab_activation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.pending_workspace_tab_activation.take() else {
            return;
        };
        self.show_welcome = false;
        self.open_workspace_file(path, window, cx);
    }

    fn sync_pending_save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_save_as && !self.has_marked_document_text(cx) {
            self.pending_save_as = false;
            self.save_document_as(window, cx);
        }
    }

    fn sync_window_edited_state(&mut self, window: &mut Window) {
        if self.pending_window_unedited {
            self.pending_window_unedited = false;
            window.set_window_edited(false);
        } else if self.pending_window_edited {
            self.pending_window_edited = false;
            window.set_window_edited(true);
        }
    }

    fn sync_scroll_viewport(&mut self, viewport_size: Size<Pixels>, cx: &mut Context<Self>) {
        match self.last_scroll_viewport_size {
            Some(previous) if Self::viewport_size_changed(previous, viewport_size) => {
                self.last_scroll_viewport_size = Some(viewport_size);
                self.request_active_block_scroll_into_view(cx);
            }
            Some(_) => {}
            None => {
                self.last_scroll_viewport_size = Some(viewport_size);
            }
        }
    }

    fn sync_window_title(&mut self, window: &mut Window, strings: &I18nStrings) {
        if self.pending_window_title_refresh {
            self.pending_window_title_refresh = false;
            let title = Self::window_title(
                self.file_path.as_deref(),
                self.recovery_source_path.as_deref(),
                self.is_recovered_document,
                self.document_dirty,
                strings,
            );
            window.set_window_title(&title);
        }
    }

    /// Windows：标题栏最左侧的汉堡按钮。点一下开/关一级菜单列表。
    fn render_hamburger_menu_button(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let is_open = self.hamburger_menu_open;
        let editor = cx.entity().downgrade();
        div()
            .id("app-hamburger-menu-button")
            .ml(px(d.menu_bar_padding_x))
            .w(px(d.menu_bar_button_height))
            .h(px(d.menu_bar_button_height))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(d.menu_bar_button_radius))
            .bg(if is_open {
                c.dialog_secondary_button_hover
            } else {
                c.dialog_surface
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .active(|this| this.opacity(0.92))
            .cursor_pointer()
            .child(
                svg()
                    .path(TITLEBAR_MENU_ICON)
                    .size(px(TITLEBAR_MENU_ICON_SIZE_PX))
                    .text_color(custom_titlebar_icon_color(theme)),
            )
            // 复用菜单栏的 hover 记账：鼠标停在按钮上就不该触发 120ms 自动关闭。
            .on_hover(cx.listener(Self::on_menu_bar_hover))
            .on_click(move |_, _window, cx| {
                let _ = editor.update(cx, |editor, cx| editor.toggle_hamburger_menu(cx));
            })
            .into_any_element()
    }

    /// Windows：汉堡按钮展开的一级菜单列表（文件/导出/语言/主题，一个竖列）。
    /// 划过哪一项，它的条目就在右边展开——条目面板仍由 `render_in_window_menu_panel`
    /// 渲染，只是锚点不同。
    fn render_hamburger_menu_panel(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
        menus: &[gpui::OwnedMenu],
        titlebar_height: f32,
    ) -> Option<AnyElement> {
        if !self.hamburger_menu_open || menus.is_empty() {
            return None;
        }
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let editor = cx.entity().downgrade();
        let labels: Vec<SharedString> = menus.iter().map(|menu| menu.name.clone()).collect();
        let list_width = menu_panel_width_for_labels(&labels, d);
        Some(
            div()
                .id("app-hamburger-menu-panel")
                .absolute()
                .occlude()
                .top(px(hamburger_menu_panel_top(titlebar_height, d)))
                .left(px(d.menu_bar_padding_x))
                .w(px(list_width))
                .p(px(d.menu_panel_padding))
                .flex()
                .flex_col()
                .gap(px(d.menu_panel_gap))
                .bg(c.dialog_surface)
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .rounded(px(d.menu_panel_radius))
                .shadow_lg()
                .on_hover(cx.listener(Self::on_menu_bar_hover))
                .children(labels.iter().enumerate().map(|(index, label)| {
                    let label = label.clone();
                    let is_open = self.menu_bar_open == Some(index);
                    let entry_editor = editor.clone();
                    div()
                        .id(("app-hamburger-menu-item", index))
                        .w_full()
                        .h(px(d.menu_item_height))
                        .px(px(d.menu_item_padding_x))
                        .flex()
                        .items_center()
                        .rounded(px(d.menu_item_radius))
                        .bg(if is_open {
                            c.dialog_secondary_button_hover
                        } else {
                            c.dialog_surface
                        })
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .active(|this| this.opacity(0.92))
                        .cursor_pointer()
                        .text_size(px(d.menu_text_size))
                        .font_weight(t.dialog_body_weight.to_font_weight())
                        .text_color(c.dialog_secondary_button_text)
                        .whitespace_nowrap()
                        .child(label)
                        .on_hover(move |hovered, _window, cx| {
                            if *hovered {
                                let _ = entry_editor.update(cx, |editor, cx| {
                                    editor.open_hamburger_menu_item(index, cx)
                                });
                            }
                        })
                }))
                .into_any_element(),
        )
    }

    /// Renders the in-window fallback menu bar backed by the app menus
    /// registered through `App::set_menus`. `menus` and `menu_labels` are
    /// fetched and computed once at the caller and shared with
    /// [`Self::render_in_window_menu_panel`].
    fn render_in_window_menu_bar(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
        menus: Option<&[gpui::OwnedMenu]>,
        menu_labels: &[SharedString],
        top_offset: f32,
    ) -> Option<AnyElement> {
        let menus = menus?;
        if menus.is_empty() {
            return None;
        }

        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let editor = cx.entity().downgrade();
        let button_widths = menu_labels
            .iter()
            .map(|label| menu_bar_button_width(label, d))
            .collect::<Vec<_>>();

        Some(
            div()
                .id("app-menu-bar")
                .absolute()
                .top(px(top_offset))
                .left_0()
                .right_0()
                .h(px(d.menu_bar_height))
                .occlude()
                .flex()
                .items_center()
                .gap(px(d.menu_bar_gap))
                .px(px(d.menu_bar_padding_x))
                .py(px(d.menu_bar_padding_y))
                .bg(c.dialog_surface)
                .border_b(px(theme.dimensions.dialog_border_width))
                .border_color(c.dialog_border)
                .on_hover(cx.listener(Self::on_menu_bar_hover))
                .children(menu_labels.iter().enumerate().map(|(index, label)| {
                    let label = label.clone();
                    let is_open = self.menu_bar_open == Some(index);
                    let button_editor = editor.clone();
                    let button_width = button_widths[index];

                    div()
                        .id(("app-menu-button", index))
                        .h(px(d.menu_bar_button_height))
                        .w(px(button_width))
                        .px(px(d.menu_bar_button_padding_x))
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .rounded(px(d.menu_bar_button_radius))
                        .bg(if is_open {
                            c.dialog_secondary_button_hover
                        } else {
                            c.dialog_surface
                        })
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .active(|this| this.opacity(0.92))
                        .cursor_pointer()
                        .text_size(px(d.menu_text_size))
                        .font_weight(t.dialog_button_weight.to_font_weight())
                        .text_color(c.dialog_secondary_button_text)
                        .whitespace_nowrap()
                        .child(label)
                        .on_hover(move |hovered, _window, cx| {
                            if *hovered {
                                let _ = button_editor
                                    .update(cx, |editor, cx| editor.open_menu_bar(index, cx));
                            }
                        })
                }))
                .into_any_element(),
        )
    }

    fn render_in_window_menu_item(
        &self,
        item: OwnedMenuItem,
        item_index: usize,
        theme: &Theme,
        editor: WeakEntity<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;

        match item {
            OwnedMenuItem::Separator => div()
                .id(("app-menu-separator", item_index))
                .flex_shrink_0()
                .mx(px(d.menu_separator_margin_x))
                .my(px(d.menu_separator_margin_y))
                .h(px(d.menu_separator_height))
                .bg(c.dialog_border)
                .into_any_element(),
            OwnedMenuItem::Action { name, action, .. } => {
                let is_disabled = action.as_ref().as_any().is::<NoRecentFiles>();
                let click_editor = editor.clone();
                let hover_editor = editor.clone();
                let base = div()
                    .id(("app-menu-item", item_index))
                    .w_full()
                    .h(px(d.menu_item_height))
                    .flex_shrink_0()
                    .px(px(d.menu_item_padding_x))
                    .flex()
                    .items_center()
                    .rounded(px(d.menu_item_radius))
                    .bg(c.dialog_surface)
                    .text_size(px(d.menu_text_size))
                    .font_weight(t.dialog_body_weight.to_font_weight())
                    .text_color(if is_disabled {
                        c.dialog_muted
                    } else {
                        c.dialog_secondary_button_text
                    })
                    .child(name)
                    .on_hover(move |hovered, _window, cx| {
                        if *hovered {
                            let _ =
                                hover_editor.update(cx, |editor, cx| editor.close_menu_submenu(cx));
                        }
                    });

                if is_disabled {
                    base.into_any_element()
                } else {
                    base.hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .active(|this| this.opacity(0.92))
                        .cursor_pointer()
                        .on_click(move |_, window, cx| {
                            let _ = click_editor.update(cx, |editor, cx| editor.close_menu_bar(cx));
                            dispatch_menu_action_for_editor(
                                action.as_ref(),
                                &click_editor,
                                window,
                                cx,
                            );
                        })
                        .into_any_element()
                }
            }
            OwnedMenuItem::Submenu(submenu) => {
                let is_open = self.menu_submenu_open == Some(item_index);
                let hover_editor = editor.clone();
                div()
                    .id(("app-menu-submenu", item_index))
                    .w_full()
                    .h(px(d.menu_item_height))
                    .flex_shrink_0()
                    .px(px(d.menu_item_padding_x))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(d.menu_item_radius))
                    .bg(if is_open {
                        c.dialog_secondary_button_hover
                    } else {
                        c.dialog_surface
                    })
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .text_size(px(d.menu_text_size))
                    .font_weight(t.dialog_body_weight.to_font_weight())
                    .text_color(c.dialog_secondary_button_text)
                    .child(submenu.name.to_string())
                    .child(">")
                    .on_hover(move |hovered, _window, cx| {
                        if *hovered {
                            let _ = hover_editor
                                .update(cx, |editor, cx| editor.open_menu_submenu(item_index, cx));
                        }
                    })
                    .into_any_element()
            }
            OwnedMenuItem::SystemMenu(os_menu) => div()
                .id(("app-menu-system", item_index))
                .w_full()
                .h(px(d.menu_item_height))
                .flex_shrink_0()
                .px(px(d.menu_item_padding_x))
                .flex()
                .items_center()
                .rounded(px(d.menu_item_radius))
                .bg(c.dialog_surface)
                .text_size(px(d.menu_text_size))
                .text_color(c.dialog_muted)
                .child(os_menu.name.to_string())
                .into_any_element(),
        }
    }

    /// Renders the currently open in-window fallback menu as a floating
    /// panel. `menus` and `menu_labels` are fetched and computed once at
    /// the caller and shared with [`Self::render_in_window_menu_bar`].
    fn render_in_window_menu_panel(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
        menus: Option<&[gpui::OwnedMenu]>,
        origin: MenuPanelOrigin,
        viewport_height: f32,
    ) -> Option<AnyElement> {
        let open_index = self.menu_bar_open?;
        let menus = menus?;
        let menu = menus.get(open_index)?.clone();
        let menu_items = menu.items.clone();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let editor = cx.entity().downgrade();
        let menu_item_labels = owned_menu_item_labels(&menu_items);
        let menu_panel_width = menu_panel_width_for_labels(&menu_item_labels, d);
        let submenu_bridge = self.menu_submenu_open.and_then(|submenu_index| {
            match menu_items.get(submenu_index)? {
                OwnedMenuItem::Submenu(submenu) => {
                    let submenu_labels = owned_menu_item_labels(&submenu.items);
                    let geometry = submenu_bridge_geometry(
                        origin.panel_left,
                        &menu_items,
                        submenu_index,
                        &submenu_labels,
                        d,
                    )?;
                    Some(
                        div()
                            .id(("app-submenu-bridge", open_index * 1000 + submenu_index))
                            .absolute()
                            .occlude()
                            .top(px(origin.panel_top + geometry.top))
                            .left(px(geometry.left))
                            .w(px(geometry.width))
                            .h(px(geometry.height))
                            .bg(hsla(0.0, 0.0, 0.0, 0.0))
                            .on_hover(cx.listener(Self::on_menu_submenu_bridge_hover))
                            .into_any_element(),
                    )
                }
                _ => None,
            }
        });
        let submenu_panel =
            self.menu_submenu_open.and_then(|submenu_index| {
                match menu_items.get(submenu_index)? {
                    OwnedMenuItem::Submenu(submenu) => {
                        let submenu_labels = owned_menu_item_labels(&submenu.items);
                        let left = origin.panel_left
                            + menu_panel_width
                            + d.menu_panel_gap;
                        let top = submenu_panel_top(&menu_items, submenu_index, d);
                        let submenu_width = menu_panel_width_for_labels(&submenu_labels, d);
                        let submenu_items = submenu.items.clone().into_iter().enumerate().map(
                            |(item_index, item)| match item {
                                OwnedMenuItem::Separator => div()
                                    .id((
                                        "app-submenu-separator",
                                        submenu_index * 1000 + item_index,
                                    ))
                                    .mx(px(d.menu_separator_margin_x))
                                    .my(px(d.menu_separator_margin_y))
                                    .h(px(d.menu_separator_height))
                                    .bg(c.dialog_border)
                                    .into_any_element(),
                                OwnedMenuItem::Action { name, action, .. } => {
                                    let is_disabled =
                                        action.as_ref().as_any().is::<NoRecentFiles>();
                                    let editor = editor.clone();
                                    let base = div()
                                        .id(("app-submenu-item", submenu_index * 1000 + item_index))
                                        .w_full()
                                        .h(px(d.menu_item_height))
                                        .px(px(d.menu_item_padding_x))
                                        .flex()
                                        .items_center()
                                        .rounded(px(d.menu_item_radius))
                                        .bg(c.dialog_surface)
                                        .text_size(px(d.menu_text_size))
                                        .font_weight(t.dialog_body_weight.to_font_weight())
                                        .text_color(if is_disabled {
                                            c.dialog_muted
                                        } else {
                                            c.dialog_secondary_button_text
                                        })
                                        .child(name);

                                    if is_disabled {
                                        base.into_any_element()
                                    } else {
                                        base.hover(|this| this.bg(c.dialog_secondary_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .on_click(move |_, window, cx| {
                                                let _ = editor.update(cx, |editor, cx| {
                                                    editor.close_menu_bar(cx)
                                                });
                                                dispatch_menu_action_for_editor(
                                                    action.as_ref(),
                                                    &editor,
                                                    window,
                                                    cx,
                                                );
                                            })
                                            .into_any_element()
                                    }
                                }
                                OwnedMenuItem::Submenu(submenu) => div()
                                    .id(("app-submenu-nested", submenu_index * 1000 + item_index))
                                    .w_full()
                                    .h(px(d.menu_item_height))
                                    .px(px(d.menu_item_padding_x))
                                    .flex()
                                    .items_center()
                                    .rounded(px(d.menu_item_radius))
                                    .bg(c.dialog_surface)
                                    .text_size(px(d.menu_text_size))
                                    .text_color(c.dialog_muted)
                                    .child(submenu.name.to_string())
                                    .into_any_element(),
                                OwnedMenuItem::SystemMenu(os_menu) => div()
                                    .id(("app-submenu-system", submenu_index * 1000 + item_index))
                                    .w_full()
                                    .h(px(d.menu_item_height))
                                    .px(px(d.menu_item_padding_x))
                                    .flex()
                                    .items_center()
                                    .rounded(px(d.menu_item_radius))
                                    .bg(c.dialog_surface)
                                    .text_size(px(d.menu_text_size))
                                    .text_color(c.dialog_muted)
                                    .child(os_menu.name.to_string())
                                    .into_any_element(),
                            },
                        );

                        Some(
                            div()
                                .id(("app-submenu-panel", open_index * 1000 + submenu_index))
                                .absolute()
                                .occlude()
                                .top(px(origin.panel_top + top))
                                .left(px(left))
                                .w(px(submenu_width))
                                .p(px(d.menu_panel_padding))
                                .flex()
                                .flex_col()
                                .gap(px(d.menu_panel_gap))
                                .bg(c.dialog_surface)
                                .border(px(d.dialog_border_width))
                                .border_color(c.dialog_border)
                                .rounded(px(d.menu_panel_radius))
                                .shadow_lg()
                                .on_hover(cx.listener(Self::on_menu_submenu_panel_hover))
                                .children(submenu_items)
                                .into_any_element(),
                        )
                    }
                    _ => None,
                }
            });

        let main_panel = div()
            .id(("app-menu-panel", open_index))
            .absolute()
            .occlude()
            .top(px(origin.panel_top + d.menu_panel_top))
            .left(px(origin.panel_left))
            .w(px(menu_panel_width))
            .p(px(d.menu_panel_padding))
            .flex()
            .flex_col()
            .gap(px(d.menu_panel_gap))
            .bg(c.dialog_surface)
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .rounded(px(d.menu_panel_radius))
            .shadow_lg()
            .on_hover(cx.listener(Self::on_menu_panel_hover));
        let main_panel = if let Some(split_index) = import_menu_split_index(&menu_items) {
            let scroll_items = &menu_items[..split_index];
            let footer_items = &menu_items[split_index..];
            let scroll_height = scrollable_import_menu_scroll_height(
                scroll_items,
                footer_items,
                viewport_height,
                origin.panel_top,
                d,
            );
            let scroll_area = (!scroll_items.is_empty()).then(|| {
                div()
                    .id(("app-menu-scroll-area", open_index))
                    .w_full()
                    .h(px(scroll_height))
                    .flex_shrink_0()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(d.menu_panel_gap))
                            .children(scroll_items.iter().cloned().enumerate().map(
                                |(item_index, item)| {
                                    self.render_in_window_menu_item(
                                        item,
                                        item_index,
                                        theme,
                                        editor.clone(),
                                    )
                                },
                            )),
                    )
                    .into_any_element()
            });
            let footer_elements =
                footer_items
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(footer_index, item)| {
                        self.render_in_window_menu_item(
                            item,
                            split_index + footer_index,
                            theme,
                            editor.clone(),
                        )
                    });

            main_panel
                .children(scroll_area)
                .children(footer_elements)
                .into_any_element()
        } else {
            let items = menu_items
                .iter()
                .cloned()
                .enumerate()
                .map(|(item_index, item)| {
                    self.render_in_window_menu_item(item, item_index, theme, editor.clone())
                });

            main_panel.children(items).into_any_element()
        };

        let layer = div()
            .id(("app-menu-panel-layer", open_index))
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .child(main_panel);
        let layer = if let Some(submenu_bridge) = submenu_bridge {
            layer.child(submenu_bridge)
        } else {
            layer
        };
        let layer = if let Some(submenu_panel) = submenu_panel {
            layer.child(submenu_panel)
        } else {
            layer
        };

        Some(layer.into_any_element())
    }

    /// Builds the unsaved-changes dialog with backdrop, message, and three
    /// action buttons (cancel, discard, save-and-close).
    pub(crate) fn on_folder_choice_cancel(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_folder_choice.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn on_folder_choice_backdrop(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_folder_choice.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn on_folder_choice_new_window(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(folder) = self.pending_folder_choice.take() {
            let _ = crate::app_menu::open_workspace_window(cx, folder);
        }
        cx.notify();
    }

    pub(crate) fn on_folder_choice_replace(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(folder) = self.pending_folder_choice.take() {
            self.set_workspace_root(folder, cx);
        }
    }

    pub(crate) fn on_copy_as_html(
        &mut self,
        _: &crate::components::CopyAsHtml,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_as_html(cx);
    }

    pub(crate) fn on_zoom_in(
        &mut self,
        _: &crate::components::ZoomIn,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_by(10, cx);
    }

    pub(crate) fn on_zoom_out(
        &mut self,
        _: &crate::components::ZoomOut,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_by(-10, cx);
    }

    pub(crate) fn on_zoom_reset(
        &mut self,
        _: &crate::components::ZoomReset,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_reset(cx);
    }

    /// 按百分比步进调整界面缩放（roadmap H5：菜单与命令面板共用）。
    pub(crate) fn zoom_by(&mut self, delta: i64, cx: &mut Context<Self>) {
        let current = crate::config::EditorSettings::zoom_percent(cx);
        let next = (current + delta).clamp(60, 200);
        if next != current {
            crate::config::EditorSettings::set_zoom_percent(cx, next);
            cx.refresh_windows();
        }
    }

    /// 缩放回到 100%。
    pub(crate) fn zoom_reset(&mut self, cx: &mut Context<Self>) {
        crate::config::EditorSettings::set_zoom_percent(cx, 100);
        cx.refresh_windows();
    }

    /// Full-area welcome page for windows opened without a document: brand
    /// mark, quick actions, and recent entries.
    fn render_welcome_page(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;

        let recent = welcome_recent_entries();
        let primary_button = div()
            .id("welcome-new-document")
            .h(px(d.dialog_button_height))
            .px(px(d.dialog_button_padding_x + 8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
            .bg(c.dialog_primary_button_bg)
            .hover(|this| this.bg(c.dialog_primary_button_hover))
            .active(|this| this.opacity(0.92))
            .cursor_pointer()
            .text_size(px(t.dialog_button_size))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(c.dialog_primary_button_text)
            .child(strings.welcome_new_document.clone())
            .on_click(cx.listener(Self::on_welcome_new_document));
        let open_button = div()
            .id("welcome-open")
            .h(px(d.dialog_button_height))
            .px(px(d.dialog_button_padding_x + 8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .bg(c.dialog_secondary_button_bg)
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .active(|this| this.opacity(0.92))
            .cursor_pointer()
            .text_size(px(t.dialog_button_size))
            .font_weight(t.dialog_button_weight.to_font_weight())
            .text_color(c.dialog_secondary_button_text)
            .child(strings.welcome_open.clone())
            .on_click(cx.listener(Self::on_welcome_open));

        let mut column = div()
            .w(px(360.0))
            .max_w(relative(1.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.0))
            .child(
                img("icon/velora.png")
                    .size(px(88.0))
                    .object_fit(ObjectFit::Contain),
            )
            .child(
                div()
                    .text_size(px(24.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(c.text_default)
                    .child("Velora"),
            )
            .child(
                div()
                    .text_size(px(t.text_size * 0.95))
                    .text_color(c.dialog_muted)
                    .child(strings.welcome_tagline.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .mt(px(6.0))
                    .child(primary_button)
                    .child(open_button),
            );

        if !recent.is_empty() {
            let mut list = div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .mt(px(10.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c.dialog_muted)
                        .child(strings.welcome_recent.clone()),
                );
            for (index, path) in recent.iter().enumerate() {
                let label = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                let folder_marker = path.is_dir();
                let entry_path = path.clone();
                list = list.child(
                    div()
                        .id(("welcome-recent-entry", index))
                        .w_full()
                        .px(px(10.0))
                        .h(px(28.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .rounded(px(5.0))
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .text_size(px(12.0))
                        .text_color(c.dialog_body)
                        .child(
                            div()
                                .max_w(px(240.0))
                                .min_w(px(0.0))
                                .truncate()
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(10.5))
                                .text_color(c.dialog_muted)
                                .child(if folder_marker {
                                    strings.workspace_folder_entry_label.clone()
                                } else {
                                    path.to_string_lossy().into_owned()
                                }),
                        )
                        .on_click(cx.listener(move |editor, _event, window, cx| {
                            editor.open_recent_entry(&entry_path, window, cx);
                        })),
                );
            }
            column = column.child(list);
        }

        div()
            .id("welcome-page")
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(c.editor_background)
            .child(column)
            .child(
                div()
                    .mt(px(18.0))
                    .text_size(px(11.0))
                    .text_color(c.dialog_muted)
                    .child(strings.welcome_shortcut_hint.clone()),
            )
            .into_any_element()
    }

    pub(crate) fn on_welcome_new_document(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_welcome = false;
        self.replace_document_from_markdown(String::new(), None, cx);
        self.pending_focus = self.first_focusable_entity_id(cx);
        self.active_entity_id = self.pending_focus;
        cx.notify();
    }

    pub(crate) fn on_welcome_open(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::app_menu::dispatch_menu_action(&crate::components::OpenFile, cx);
    }

    /// In-app dialog asking whether a picked folder should replace this
    /// window's working set or open in a new window.
    fn render_folder_choice_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let strings = cx.global::<I18nManager>().strings();

        div()
            .id("folder-choice-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(c.dialog_backdrop)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_folder_choice_backdrop))
            .child(
                div()
                    .id("folder-choice-dialog")
                    .w(px(d.dialog_width))
                    .max_w(relative(1.0))
                    .flex()
                    .flex_col()
                    .gap(px(d.dialog_gap))
                    .p(px(d.dialog_padding))
                    .bg(c.dialog_surface)
                    .border(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .rounded(px(d.dialog_radius))
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(px(t.dialog_title_size))
                            .font_weight(t.dialog_title_weight.to_font_weight())
                            .text_color(c.dialog_title)
                            .child(strings.workspace_folder_choice_title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(d.dialog_button_gap))
                            .child(
                                div()
                                    .id("folder-choice-cancel")
                                    .h(px(d.dialog_button_height))
                                    .px(px(d.dialog_button_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_secondary_button_bg)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .active(|this| this.opacity(0.92))
                                    .cursor_pointer()
                                    .text_size(px(t.dialog_button_size))
                                    .font_weight(t.dialog_button_weight.to_font_weight())
                                    .text_color(c.dialog_secondary_button_text)
                                    .child(strings.open_link_cancel.clone())
                                    .on_click(cx.listener(Self::on_folder_choice_cancel)),
                            )
                            .child(
                                div()
                                    .id("folder-choice-new-window")
                                    .h(px(d.dialog_button_height))
                                    .px(px(d.dialog_button_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                    .border(px(d.dialog_border_width))
                                    .border_color(c.dialog_border)
                                    .bg(c.dialog_secondary_button_bg)
                                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                    .active(|this| this.opacity(0.92))
                                    .cursor_pointer()
                                    .text_size(px(t.dialog_button_size))
                                    .font_weight(t.dialog_button_weight.to_font_weight())
                                    .text_color(c.dialog_secondary_button_text)
                                    .child(strings.workspace_open_new_window_button.clone())
                                    .on_click(cx.listener(Self::on_folder_choice_new_window)),
                            )
                            .child(
                                div()
                                    .id("folder-choice-replace")
                                    .h(px(d.dialog_button_height))
                                    .px(px(d.dialog_button_padding_x))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                    .bg(c.dialog_primary_button_bg)
                                    .hover(|this| this.bg(c.dialog_primary_button_hover))
                                    .active(|this| this.opacity(0.92))
                                    .cursor_pointer()
                                    .text_size(px(t.dialog_button_size))
                                    .font_weight(t.dialog_button_weight.to_font_weight())
                                    .text_color(c.dialog_primary_button_text)
                                    .child(strings.workspace_replace_current_button.clone())
                                    .on_click(cx.listener(Self::on_folder_choice_replace)),
                            ),
                    ),
            )
    }

    fn render_unsaved_changes_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let strings = cx.global::<I18nManager>().strings();

        div()
            .id("unsaved-changes-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(c.dialog_backdrop)
            .child(
                div()
                    .w_full()
                    .px(px(d.editor_padding))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("unsaved-changes-dialog")
                            .w(px(d.dialog_width))
                            .max_w(relative(1.0))
                            .flex()
                            .flex_col()
                            .gap(px(d.dialog_gap))
                            .p(px(d.dialog_padding))
                            .bg(c.dialog_surface)
                            .border(px(d.dialog_border_width))
                            .border_color(c.dialog_border)
                            .rounded(px(d.dialog_radius))
                            .shadow_lg()
                            .child(
                                div()
                                    .text_size(px(t.dialog_title_size))
                                    .font_weight(t.dialog_title_weight.to_font_weight())
                                    .text_color(c.dialog_title)
                                    .child(strings.unsaved_changes_title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(t.dialog_body_size))
                                    .font_weight(t.dialog_body_weight.to_font_weight())
                                    .line_height(relative(t.text_line_height))
                                    .text_color(c.dialog_body)
                                    .child(strings.unsaved_changes_message.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(d.dialog_button_gap))
                                    .child(
                                        div()
                                            .id("cancel-close-dialog")
                                            .h(px(d.dialog_button_height))
                                            .px(px(d.dialog_button_padding_x))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                            .border(px(d.dialog_border_width))
                                            .border_color(c.dialog_border)
                                            .bg(c.dialog_secondary_button_bg)
                                            .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .text_size(px(t.dialog_button_size))
                                            .font_weight(t.dialog_button_weight.to_font_weight())
                                            .text_color(c.dialog_secondary_button_text)
                                            .child(strings.unsaved_changes_cancel.clone())
                                            .on_click(cx.listener(Self::on_cancel_close_dialog)),
                                    )
                                    .child(
                                        div()
                                            .id("discard-and-close-dialog")
                                            .h(px(d.dialog_button_height))
                                            .px(px(d.dialog_button_padding_x))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                            .border(px(d.dialog_border_width))
                                            .border_color(c.dialog_border)
                                            .bg(c.dialog_danger_button_bg)
                                            .hover(|this| this.bg(c.dialog_danger_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .text_size(px(t.dialog_button_size))
                                            .font_weight(t.dialog_button_weight.to_font_weight())
                                            .text_color(c.dialog_danger_button_text)
                                            .child(
                                                strings.unsaved_changes_discard_and_close.clone(),
                                            )
                                            .on_click(cx.listener(Self::on_discard_and_close)),
                                    )
                                    .child(
                                        div()
                                            .id("save-and-close-dialog")
                                            .h(px(d.dialog_button_height))
                                            .px(px(d.dialog_button_padding_x))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                            .bg(c.dialog_primary_button_bg)
                                            .hover(|this| this.bg(c.dialog_primary_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .text_size(px(t.dialog_button_size))
                                            .font_weight(t.dialog_button_weight.to_font_weight())
                                            .text_color(c.dialog_primary_button_text)
                                            .child(strings.unsaved_changes_save_and_close.clone())
                                            .on_click(cx.listener(Self::on_save_and_close)),
                                    ),
                            ),
                    ),
            )
    }

    /// Builds the dropped-file replacement dialog shown when the current
    /// document has unsaved changes.
    fn render_drop_replace_overlay(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let strings = cx.global::<I18nManager>().strings();

        div()
            .id("drop-replace-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(c.dialog_backdrop)
            .child(
                div()
                    .w_full()
                    .px(px(d.editor_padding))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("drop-replace-dialog")
                            .w(px(d.dialog_width))
                            .max_w(relative(1.0))
                            .flex()
                            .flex_col()
                            .gap(px(d.dialog_gap))
                            .p(px(d.dialog_padding))
                            .bg(c.dialog_surface)
                            .border(px(d.dialog_border_width))
                            .border_color(c.dialog_border)
                            .rounded(px(d.dialog_radius))
                            .shadow_lg()
                            .child(
                                div()
                                    .text_size(px(t.dialog_title_size))
                                    .font_weight(t.dialog_title_weight.to_font_weight())
                                    .text_color(c.dialog_title)
                                    .child(strings.drop_replace_title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(t.dialog_body_size))
                                    .font_weight(t.dialog_body_weight.to_font_weight())
                                    .line_height(relative(t.text_line_height))
                                    .text_color(c.dialog_body)
                                    .child(strings.drop_replace_message.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(d.dialog_button_gap))
                                    .child(
                                        div()
                                            .id("cancel-drop-replace-dialog")
                                            .h(px(d.dialog_button_height))
                                            .px(px(d.dialog_button_padding_x))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                            .border(px(d.dialog_border_width))
                                            .border_color(c.dialog_border)
                                            .bg(c.dialog_secondary_button_bg)
                                            .hover(|this| this.bg(c.dialog_secondary_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .text_size(px(t.dialog_button_size))
                                            .font_weight(t.dialog_button_weight.to_font_weight())
                                            .text_color(c.dialog_secondary_button_text)
                                            .child(strings.drop_replace_cancel.clone())
                                            .on_click(
                                                cx.listener(Self::on_cancel_drop_replace_dialog),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("discard-and-replace-drop-dialog")
                                            .h(px(d.dialog_button_height))
                                            .px(px(d.dialog_button_padding_x))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                            .border(px(d.dialog_border_width))
                                            .border_color(c.dialog_border)
                                            .bg(c.dialog_danger_button_bg)
                                            .hover(|this| this.bg(c.dialog_danger_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .text_size(px(t.dialog_button_size))
                                            .font_weight(t.dialog_button_weight.to_font_weight())
                                            .text_color(c.dialog_danger_button_text)
                                            .child(strings.drop_replace_discard_and_replace.clone())
                                            .on_click(
                                                cx.listener(Self::on_discard_and_replace_drop),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("save-and-replace-drop-dialog")
                                            .h(px(d.dialog_button_height))
                                            .px(px(d.dialog_button_padding_x))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                            .bg(c.dialog_primary_button_bg)
                                            .hover(|this| this.bg(c.dialog_primary_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .text_size(px(t.dialog_button_size))
                                            .font_weight(t.dialog_button_weight.to_font_weight())
                                            .text_color(c.dialog_primary_button_text)
                                            .child(strings.drop_replace_save_and_replace.clone())
                                            .on_click(cx.listener(Self::on_save_and_replace_drop)),
                                    ),
                            ),
                    ),
            )
    }

    fn info_dialog_title<'a>(&self, strings: &'a I18nStrings, kind: InfoDialogKind) -> &'a str {
        match kind {
            InfoDialogKind::CheckForUpdates => &strings.help_check_updates_title,
            InfoDialogKind::About => &strings.help_about_title,
        }
    }

    pub(crate) fn about_dialog_body_lines(strings: &I18nStrings) -> Vec<String> {
        vec![
            format!("Velora {}", env!("CARGO_PKG_VERSION")),
            strings.help_about_message.clone(),
            format!("{}: {}", strings.help_about_github_label, ABOUT_GITHUB_URL),
            strings.help_about_star_message.clone(),
        ]
    }

    fn info_dialog_body(&self, strings: &I18nStrings, kind: InfoDialogKind) -> String {
        match kind {
            InfoDialogKind::CheckForUpdates => strings.help_check_updates_message.clone(),
            InfoDialogKind::About => Self::about_dialog_body_lines(strings).join("\n"),
        }
    }

    fn render_info_dialog_body(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        kind: InfoDialogKind,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let body_style = |this: Div| {
            this.text_size(px(t.dialog_body_size))
                .font_weight(t.dialog_body_weight.to_font_weight())
                .line_height(relative(t.text_line_height))
                .text_color(c.dialog_body)
        };

        match kind {
            InfoDialogKind::CheckForUpdates => div()
                .flex()
                .flex_col()
                .gap(px(d.dialog_gap * 0.5))
                .child(
                    body_style(div()).children(
                        self.info_dialog_body(strings, kind)
                            .lines()
                            .map(|line| div().child(line.to_string())),
                    ),
                )
                .into_any_element(),
            InfoDialogKind::About => div()
                .flex()
                .flex_col()
                .gap(px(d.dialog_gap * 0.5))
                .child(body_style(div()).child(format!("Velora {}", env!("CARGO_PKG_VERSION"))))
                .child(body_style(div()).child(strings.help_about_message.clone()))
                .child(
                    body_style(div())
                        .flex()
                        .flex_wrap()
                        .gap(px(4.0))
                        .child(format!("{}:", strings.help_about_github_label))
                        .child(
                            div()
                                .id("about-github-link")
                                .cursor_pointer()
                                .text_color(c.text_link)
                                .underline()
                                .child(ABOUT_GITHUB_URL)
                                .on_click(move |_, _, cx| {
                                    open_about_github_url(cx);
                                }),
                        ),
                )
                .child(body_style(div()).child(strings.help_about_star_message.clone()))
                .into_any_element(),
        }
    }

    fn on_dismiss_info_dialog(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.hide_info_dialog(cx);
    }

    fn render_info_dialog_overlay(
        &self,
        theme: &Theme,
        kind: InfoDialogKind,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let strings = cx.global::<I18nManager>().strings();

        div()
            .id("info-dialog-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(c.dialog_backdrop)
            .child(
                div()
                    .w_full()
                    .px(px(d.editor_padding))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("info-dialog")
                            .w(px(d.dialog_width))
                            .max_w(relative(1.0))
                            .flex()
                            .flex_col()
                            .gap(px(d.dialog_gap))
                            .p(px(d.dialog_padding))
                            .bg(c.dialog_surface)
                            .border(px(d.dialog_border_width))
                            .border_color(c.dialog_border)
                            .rounded(px(d.dialog_radius))
                            .shadow_lg()
                            .child(
                                div()
                                    .text_size(px(t.dialog_title_size))
                                    .font_weight(t.dialog_title_weight.to_font_weight())
                                    .text_color(c.dialog_title)
                                    .child(self.info_dialog_title(strings, kind).to_string()),
                            )
                            .child(self.render_info_dialog_body(theme, strings, kind))
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .gap(px(d.dialog_button_gap))
                                    .child(
                                        div()
                                            .id("dismiss-info-dialog")
                                            .h(px(d.dialog_button_height))
                                            .px(px(d.dialog_button_padding_x))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                                            .bg(c.dialog_primary_button_bg)
                                            .hover(|this| this.bg(c.dialog_primary_button_hover))
                                            .active(|this| this.opacity(0.92))
                                            .cursor_pointer()
                                            .text_size(px(t.dialog_button_size))
                                            .font_weight(t.dialog_button_weight.to_font_weight())
                                            .text_color(c.dialog_primary_button_text)
                                            .child(strings.info_dialog_ok.clone())
                                            .on_click(cx.listener(Self::on_dismiss_info_dialog)),
                                    ),
                            ),
                    ),
            )
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {

        self.window_handle = Some(window.window_handle());
        if self.system_appearance_subscription.is_none() {
            self.system_appearance_subscription =
                Some(cx.observe_window_appearance(window, |_editor, window, cx| {
                    let appearance = window.appearance();
                    cx.update_global::<ThemeManager, _>(|manager, _cx| {
                        manager.set_system_appearance(appearance)
                    });
                    cx.refresh_windows();
                }));
        }
        self.install_close_guard(cx, window);
        self.apply_pending_focus(window, cx);
        self.apply_pending_scroll_into_view(window, cx);
        self.apply_pending_workspace_search_focus(window, cx);
        self.last_selection_snapshot = self.capture_source_selection_snapshot(cx);
        self.sync_pending_save(window, cx);
        self.sync_pending_save_as(window, cx);
        self.sync_pending_workspace_tab_activation(window, cx);
        self.sync_window_edited_state(window);

        let viewport_bounds = self.scroll_handle.bounds();
        let viewport_size = viewport_bounds.size;
        self.sync_scroll_viewport(viewport_size, cx);
        self.sync_outline_follow_scroll(
            self.scroll_handle.bounds().top(),
            cx,
        );

        let mut theme = cx.global::<ThemeManager>().current_arc().as_ref().clone();
        let fonts = crate::config::EditorSettings::fonts(cx);
        let writing_width = crate::config::EditorSettings::writing_width(cx);
        theme.typography.text_size = fonts.markdown_size as f32;
        theme.typography.code_size = fonts.code_size as f32;
        // Session-wide zoom (⌘+/⌘-/⌘0): scales the whole typographic scale,
        // not just body text, so hierarchy stays consistent.
        let zoom = crate::config::EditorSettings::zoom_percent(cx) as f32 / 100.0;
        if (zoom - 1.0).abs() > f32::EPSILON {
            let t = &mut theme.typography;
            t.text_size *= zoom;
            t.code_size *= zoom;
            t.h1_size *= zoom;
            t.h2_size *= zoom;
            t.h3_size *= zoom;
            t.h4_size *= zoom;
            t.h5_size *= zoom;
            t.h6_size *= zoom;
        }
        let strings = cx.global::<I18nManager>().strings_arc();
        self.sync_window_title(window, &strings);

        let d = &theme.dimensions;
        // P4b：键命中时整帧复用行结构计划；未命中（编辑/折叠/大纲/模式
        // 切换后的第一帧）才做一次全文档扫描。
        let rendered_mode = self.view_mode == super::ViewMode::Rendered;
        let plan_key = (
            self.document_revision,
            self.fold_state_version,
            self.toc_state_version,
            rendered_mode,
            d.block_gap,
            self.document.visible_blocks().len(),
        );
        let cached_plan = self
            .rendered_row_plan
            .clone()
            .filter(|plan| {
                plan.revision == plan_key.0
                    && plan.fold_version == plan_key.1
                    && plan.toc_version == plan_key.2
                    && plan.rendered_mode == plan_key.3
                    && plan.block_gap == plan_key.4
                    && plan.visible_len == plan_key.5
            });
        let plan_rebuilt = cached_plan.is_none();
        let rendered_row_plan = match cached_plan {
            Some(plan) => plan,
            None => {
                let visible_blocks =
                    self.apply_heading_fold_filter(self.document.visible_blocks().to_vec(), cx);
                let plan = std::sync::Arc::new(self.build_rendered_row_plan(
                    &visible_blocks,
                    plan_key.0,
                    plan_key.1,
                    plan_key.2,
                    rendered_mode,
                    plan_key.4,
                    plan_key.5,
                    cx,
                ));
                self.rendered_row_plan = Some(plan.clone());
                plan
            }
        };
        let rows = &rendered_row_plan.rows;
        let focused_visible_index = self
            .focused_edit_target_entity_id(window, cx)
            .and_then(|id| {
                self.document.visible_index_for_entity_id(id).or_else(|| {
                    self.table_cell_binding(id).and_then(|binding| {
                        self.document
                            .visible_index_for_entity_id(binding.table_block.entity_id())
                    })
                })
            });
        let focus_mode_active = self.focus_mode
            && self.view_mode == super::ViewMode::Rendered
            && !self.code_tab_active()
            && self.cross_block_selection.is_none();
        let editor = cx.entity().downgrade();
        let has_menus = cx
            .get_menus()
            .map(|menus| !menus.is_empty())
            .unwrap_or(false);
        let titlebar_height = custom_titlebar_height(window, d);
        let menu_bar_height =
            in_window_menu_bar_height_for_target_os(std::env::consts::OS, has_menus, d);
        let scroll_trigger_padding = (d.block_min_height * 0.75).max(16.0);
        let max_scroll_y = f32::from(self.scroll_handle.max_offset().height.max(px(0.0)));
        let viewport_height = f32::from(viewport_bounds.size.height.max(px(1.0)));
        // Extra room below the last block so the lowest line can be scrolled up
        // to the viewport center instead of being pinned to the bottom edge.
        let scroll_beyond_bottom = viewport_height * 0.5;
        let viewport_width = f32::from(viewport_bounds.size.width.max(px(1.0)));
        let has_overflow = max_scroll_y > 0.5;

        let centered_width = if self.code_tab_active() {
            (viewport_width - 72.0).max(1.0)
        } else {
            Self::centered_column_width(viewport_width, &theme.dimensions)
                .min(writing_width.max_width(theme.dimensions.writing_max_width))
        };
        let current_scroll_y = (-f32::from(self.scroll_handle.offset().y)).clamp(0.0, max_scroll_y);
        let scrollbar_geometry =
            Self::scrollbar_geometry(viewport_height, max_scroll_y, current_scroll_y);
        let track_height = scrollbar_geometry.track_height;
        let thumb_height = scrollbar_geometry.thumb_height;
        let thumb_top = scrollbar_geometry.thumb_top;

        let show_custom_scrollbar = has_overflow
            && (self.scrollbar_drag.is_some()
                || self.scrollbar_hovered
                || Instant::now() <= self.scrollbar_visible_until);

        // Spacing metadata is read on demand instead of pre-collected into a
        // Vec<RenderedRowSpacingInfo> sized to all visible blocks. For long
        // documents this skips a ~tens-of-KB allocation per frame; per-block
        // entity.read_with is a cheap immutable lock + 7-field struct copy.
        // P7：行元数据（起始下标/行距/行首 id/footprint）全部来自计划，
        // 未变更帧零重算。
        let row_starts = &rendered_row_plan.visible_starts;
        let row_top_gaps = &rendered_row_plan.gaps;
        let row_first_ids = &rendered_row_plan.first_ids;
        // The focused row is always kept mounted so its caret is not blurred; a
        // table cell maps to its containing table block's row.
        let focus_row = focused_visible_index.map(|visible_index| {
            row_starts
                .partition_point(|&start| start <= visible_index)
                .saturating_sub(1)
        });

        // A row's first block keys its cached footprint.

        // On a structural edit the row indices no longer match last frame, so the
        // cache refresh below is skipped; its block-keyed entries still hold.
        // P4b：只有计划重建的帧才需要比较（其余帧 id 序列必然一致）。
        let structural_change = plan_rebuilt
            && (rows.len() != self.prev_visible_block_ids.len()
                || rows
                    .iter()
                    .zip(&self.prev_visible_block_ids)
                    .any(|(row, prev)| row.first_id != *prev));
        if structural_change {
            self.prev_visible_block_ids = rows.iter().map(|row| row.first_id).collect();
        }

        // A footprint only holds for the column it was measured at. The first
        // frame has no scroll bounds yet, so the column collapses to its 1px
        // floor and every block wraps a character per line; keeping those
        // measurements would leave the document permanently mis-sized.
        let width_changed = self.row_stride_width != Some(centered_width);
        if width_changed {
            self.row_stride_cache.clear();
            self.row_stride_width = Some(centered_width);
            // 长行折叠块用定值 min_size 撑出横向滚动后，available 变成自身
            // 宽度，容器宽只能在探针帧读到：宽度变化时给全部根块置脏，
            // 下一帧重新学习换行参照宽。
            for block in self.document.root_blocks() {
                block.update(cx, |block, _block_cx| {
                    block.wrap_container_width_dirty = true;
                });
            }
        }

        // The scroll container records every mounted child's layout bounds, so
        // adjacent tops differ by exactly one row's footprint whatever the row
        // holds. Caching those differences, not raw positions, keeps the window
        // stable while scrolling.
        if !structural_change && !width_changed {
            if let Some(prev) = self
                .prev_mounted_run
                .filter(|prev| self.mounted_run_is_addressable(*prev))
            {
                let prev_end = prev.row_end.min(row_first_ids.len());
                for row in prev.row_start..prev_end.saturating_sub(1) {
                    let child = prev.child_base + row - prev.row_start;
                    if let (Some(bounds), Some(next_bounds)) = (
                        self.scroll_handle.bounds_for_item(child),
                        self.scroll_handle.bounds_for_item(child + 1),
                    ) {
                        let stride = f32::from(next_bounds.top() - bounds.top());
                        if stride > 0.0 && stride.is_finite() {
                            self.row_stride_cache.insert(row_first_ids[row], stride);
                            if let Some(slot) = rendered_row_plan.strides.borrow_mut().get_mut(row)
                            {
                                *slot = stride;
                            }
                        }
                    }
                }
            }
        }

        // Unmeasured rows use the minimum block height: a lower bound, so the
        // window over-mounts rather than ever landing on a spacer.
        let estimate = d.block_min_height.max(1.0);
        let strides = rendered_row_plan.strides.borrow();

        // Bound the cache against block churn, only when it outgrows the live rows.
        if self.row_stride_cache.len() > row_first_ids.len().saturating_mul(2) {
            let live: std::collections::HashSet<EntityId> = row_first_ids.iter().copied().collect();
            self.row_stride_cache.retain(|id, _| live.contains(id));
        }

        let render_window = Self::rendered_window(
            &strides,
            current_scroll_y,
            viewport_height,
            RENDER_OVERDRAW_PX,
            focus_row,
            estimate,
        );

        // 冷启动续挂：行高仍被低估时一帧铺不满视口，立刻排下一帧继续补，
        // 而不是把整屏 spacer 留给读者、等到下一次输入才补上。
        if render_window.needs_fill && self.cold_fill_frames < COLD_FILL_MAX_FRAMES {
            self.cold_fill_frames += 1;
            self.schedule_followup_frame(cx);
        } else {
            self.cold_fill_frames = 0;
        }

        let island = render_window.focus_island;
        let island_before_run = island.is_some_and(|island| island.row < render_window.run_start);
        // A mounted row re-applies its own `mt`, which the preceding stride
        // already covered, so every spacer sheds the gap of the row it precedes.
        let spacer_before = |row: usize, height: f32| -> f32 {
            match row_top_gaps.get(row) {
                Some(gap) => (height - gap).max(0.0),
                None => height,
            }
        };
        let mut block_rows: Vec<AnyElement> =
            Vec::with_capacity(render_window.run_end - render_window.run_start + 4);
        let push_spacer = |rows: &mut Vec<AnyElement>, height: f32| {
            if height > 0.5 {
                rows.push(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .h(px(height))
                        .into_any_element(),
                );
            }
        };

        // P4b：行元素只在挂载时从计划构建（旧实现在扫描期为所有组行
        // 预构建元素再丢弃，是超大文档的每帧浪费）。
        let build_row_element = |row: usize| -> AnyElement {
            match &rows[row].body {
                RenderedRowBody::Ordinary { entity, .. } => {
                    let index = rows[row].visible_start;
                    let element = div()
                        .w(px(centered_width))
                        .max_w(relative(1.0))
                        .flex_shrink_0()
                        .mt(px(row_top_gaps[row]))
                        .opacity(focus_mode_row_opacity(
                            focus_mode_active,
                            focused_visible_index,
                            index,
                            index + 1,
                        ))
                        .child(entity.clone());
                    let element = if rendered_mode {
                        let row_editor = editor.clone();
                        let entity_id = entity.entity_id();
                        element.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                            let _ = row_editor.update(cx, |editor, cx| {
                                editor
                                    .on_block_context_menu_mouse_down(entity_id, event, window, cx);
                            });
                        })
                    } else {
                        element
                    };
                    element.into_any_element()
                }
                RenderedRowBody::Group {
                    callout_variant,
                    members,
                } => {
                    let index = rows[row].visible_start;
                    let group_end = index + members.len();
                    if let Some(variant) = callout_variant {
                        let mut group_children: Vec<AnyElement> = Vec::new();
                        let mut member_index = 0usize;
                        let mut previous_callout_row: Option<RenderedRowSpacingInfo> = None;
                        while member_index < members.len() {
                            let member = &members[member_index];
                            if let Some(footnote_anchor) = member.spacing.footnote_anchor {
                                let mut footnote_children: Vec<AnyElement> = Vec::new();
                                let mut previous_footnote_row: Option<RenderedRowSpacingInfo> =
                                    None;
                                let footnote_start = member_index;
                                while member_index < members.len()
                                    && members[member_index].spacing.footnote_anchor
                                        == Some(footnote_anchor)
                                {
                                    let inner = &members[member_index];
                                    let row = div()
                                        .w_full()
                                        .flex_shrink_0()
                                        .mt(px(footnote_row_top_gap(
                                            previous_footnote_row,
                                            d.block_gap,
                                        )))
                                        .child(inner.entity.clone());
                                    let row = if rendered_mode {
                                        let row_editor = editor.clone();
                                        let entity_id = inner.entity.entity_id();
                                        row.on_mouse_down(
                                            MouseButton::Right,
                                            move |event, window, cx| {
                                                let _ = row_editor.update(cx, |editor, cx| {
                                                    editor.on_block_context_menu_mouse_down(
                                                        entity_id, event, window, cx,
                                                    );
                                                });
                                            },
                                        )
                                    } else {
                                        row
                                    };
                                    footnote_children.push(row.into_any_element());
                                    previous_footnote_row = Some(inner.spacing);
                                    member_index += 1;
                                }

                                group_children.push(
                                    div()
                                        .w_full()
                                        .flex_shrink_0()
                                        .mt(px(callout_row_top_gap(
                                            previous_callout_row,
                                            members[footnote_start].spacing,
                                            d,
                                        )))
                                        .child(footnote_group_shell(
                                            footnote_children,
                                            &theme,
                                            d,
                                        ))
                                        .into_any_element(),
                                );
                                previous_callout_row = Some(members[member_index - 1].spacing);
                                continue;
                            }

                            let row = div()
                                .w_full()
                                .flex_shrink_0()
                                .mt(px(callout_row_top_gap(
                                    previous_callout_row,
                                    member.spacing,
                                    d,
                                )))
                                .child(member.entity.clone());
                            let row = if rendered_mode {
                                let row_editor = editor.clone();
                                let entity_id = member.entity.entity_id();
                                row.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                    let _ = row_editor.update(cx, |editor, cx| {
                                        editor.on_block_context_menu_mouse_down(
                                            entity_id, event, window, cx,
                                        );
                                    });
                                })
                            } else {
                                row
                            };
                            group_children.push(row.into_any_element());
                            previous_callout_row = Some(member.spacing);
                            member_index += 1;
                        }

                        let (accent, background) = callout_colors(*variant, &theme);
                        div()
                            .w(px(centered_width))
                            .max_w(relative(1.0))
                            .flex_shrink_0()
                            .mt(px(row_top_gaps[row]))
                            .flex()
                            .flex_col()
                            .gap(px(0.0))
                            .px(px(d.callout_padding_x))
                            .py(px(d.callout_padding_y))
                            .rounded(px(d.callout_radius))
                            .border_l(px(d.callout_border_width))
                            .border_color(accent)
                            .bg(background)
                            .opacity(focus_mode_row_opacity(
                                focus_mode_active,
                                focused_visible_index,
                                index,
                                group_end,
                            ))
                            .children(group_children)
                            .into_any_element()
                    } else {
                        let mut group_children: Vec<AnyElement> = Vec::new();
                        let mut previous_footnote_row: Option<RenderedRowSpacingInfo> = None;
                        for member in members {
                            let row = div()
                                .w_full()
                                .flex_shrink_0()
                                .mt(px(footnote_row_top_gap(
                                    previous_footnote_row,
                                    d.block_gap,
                                )))
                                .child(member.entity.clone());
                            let row = if rendered_mode {
                                let row_editor = editor.clone();
                                let entity_id = member.entity.entity_id();
                                row.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                    let _ = row_editor.update(cx, |editor, cx| {
                                        editor.on_block_context_menu_mouse_down(
                                            entity_id, event, window, cx,
                                        );
                                    });
                                })
                            } else {
                                row
                            };
                            group_children.push(row.into_any_element());
                            previous_footnote_row = Some(member.spacing);
                        }

                        div()
                            .w(px(centered_width))
                            .max_w(relative(1.0))
                            .flex_shrink_0()
                            .mt(px(row_top_gaps[row]))
                            .opacity(focus_mode_row_opacity(
                                focus_mode_active,
                                focused_visible_index,
                                index,
                                group_end,
                            ))
                            .child(footnote_group_shell(group_children, &theme, d))
                            .into_any_element()
                    }
                }
            }
        };
        let take_row = |rows: &mut Vec<AnyElement>, row: usize| {
            rows.push(build_row_element(row));
        };

        if let Some(island) = island.filter(|_| island_before_run) {
            push_spacer(&mut block_rows, spacer_before(island.row, island.lead_h));
            take_row(&mut block_rows, island.row);
        }
        push_spacer(
            &mut block_rows,
            spacer_before(render_window.run_start, render_window.top_h),
        );
        let run_child_base = block_rows.len();
        for row in render_window.run_start..render_window.run_end {
            take_row(&mut block_rows, row);
        }
        if let Some(island) = island.filter(|_| !island_before_run) {
            push_spacer(&mut block_rows, spacer_before(island.row, island.lead_h));
            take_row(&mut block_rows, island.row);
        }
        push_spacer(&mut block_rows, render_window.bottom_h);
        // Next frame reads the run's footprints back at these child indices, and
        // re-checks `child_count` before trusting them.
        self.prev_mounted_run = Some(MountedRun {
            row_start: render_window.run_start,
            row_end: render_window.run_end,
            child_base: run_child_base,
            child_count: block_rows.len(),
        });

        let scroll_content = div()
            .id("editor-scroll-inner")
            .flex()
            .flex_col()
            .flex_grow()
            .h_full()
            .items_center()
            .bg(theme.colors.editor_background)
            .overflow_y_scroll()
            .scrollbar_width(px(0.0))
            .track_scroll(&self.scroll_handle)
            .on_hover(cx.listener(Self::on_editor_hover))
            .capture_any_mouse_down(cx.listener(Self::on_editor_capture_mouse_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_editor_mouse_down))
            .on_mouse_move(cx.listener(Self::on_editor_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_editor_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_editor_mouse_up))
            .on_scroll_wheel(cx.listener(Self::on_editor_scroll_wheel))
            .p(px(d.editor_padding))
            .pt(px(if self.code_tab_active() {
                24.0
            } else if self.typewriter_mode && self.view_mode == super::ViewMode::Rendered {
                (viewport_height * 0.5).max(52.0)
            } else {
                52.0
            }))
            .pb(px(d.editor_padding
                + scroll_trigger_padding
                + scroll_beyond_bottom))
            .children(block_rows);
        let scroll_content = if self.view_mode == super::ViewMode::Rendered {
            scroll_content.on_mouse_down(
                MouseButton::Right,
                cx.listener(Self::on_editor_context_menu_mouse_down),
            )
        } else {
            scroll_content
        };

        let content_area = div()
            .id("editor-scroll")
            .w_full()
            .h_full()
            .flex_1()
            .min_w(px(0.0))
            .bg(theme.colors.editor_background)
            .relative()
            .child(scroll_content);

        let content_area = if show_custom_scrollbar {
            let scrollbar_editor = editor.clone();
            let track_origin_y = f32::from(viewport_bounds.origin.y);
            content_area.child(
                div()
                    .id("editor-scrollbar-thumb")
                    .absolute()
                    .occlude()
                    .top(px(thumb_top))
                    .right(px(d.scrollbar_right))
                    .w(px(d.scrollbar_width))
                    .h(px(thumb_height))
                    .rounded(px(999.0))
                    .bg(theme.colors.scrollbar_thumb)
                    .cursor_pointer()
                    .on_hover(cx.listener(Self::on_editor_hover))
                    .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
                        let pointer_offset_y =
                            f32::from(event.position.y) - track_origin_y - thumb_top;
                        let _ = scrollbar_editor.update(cx, |editor, cx| {
                            cx.stop_propagation();
                            editor.start_scrollbar_drag(
                                pointer_offset_y,
                                track_height,
                                thumb_height,
                                max_scroll_y,
                                cx,
                            );
                        });
                    })
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |_thumb_bounds, _, window, _| {
                                window.on_mouse_event({
                                    let editor = editor.clone();
                                    move |_event: &MouseUpEvent, phase, _window, cx| {
                                        if !phase.bubble() {
                                            return;
                                        }
                                        let _ = editor.update(cx, |editor, cx| {
                                            editor.end_scrollbar_drag(cx);
                                        });
                                    }
                                });

                                window.on_mouse_event({
                                    let editor = editor.clone();
                                    move |event: &MouseMoveEvent, phase, _window, cx| {
                                        if !phase.bubble() || !event.dragging() {
                                            return;
                                        }

                                        let pointer_y_in_track =
                                            f32::from(event.position.y) - track_origin_y;
                                        let _ = editor.update(cx, |editor, cx| {
                                            editor.update_scrollbar_drag(pointer_y_in_track, cx);
                                        });
                                    }
                                });
                            },
                        )
                        .size_full(),
                    ),
            )
        } else {
            content_area
        };

        let content_area = content_area.into_any_element();
        let content_area = if self.quick_open.is_some() {
            self.render_quick_open_overlay(&theme, cx)
        } else if self.command_palette.is_some() {
            super::command_palette::render_command_palette_overlay(self, &theme, cx)
        } else {
            content_area
        };
        // A tab whose file the text editor can't preview replaces the whole
        // content area with a centered notice, VS Code style.
        let content_area = if self.show_welcome {
            self.render_welcome_page(&theme, &strings, cx)
        } else if let Some(path) = self.unsupported_preview_path.as_ref() {
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            div()
                .id("unsupported-preview")
                .w_full()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .text_color(theme.colors.dialog_muted)
                .child(
                    div()
                        .w(px(44.0))
                        .h(px(44.0))
                        .rounded(px(22.0))
                        .border_1()
                        .border_color(theme.colors.dialog_border)
                        .bg(theme.colors.dialog_secondary_button_bg)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(26.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.colors.dialog_muted)
                        .child("!"),
                )
                .child(
                    div()
                        .text_size(px(theme.typography.text_size))
                        .text_color(theme.colors.text_default)
                        .child(file_name),
                )
                .child(
                    div()
                        .text_size(px(theme.typography.text_size * 0.9))
                        .child(strings.workspace_preview_unavailable_message.clone()),
                )
                .children(self.unsupported_preview_detail.as_ref().map(|detail| {
                    div()
                        .px(px(12.0))
                        .text_size(px(theme.typography.text_size * 0.8))
                        .text_color(theme.colors.dialog_muted)
                        .text_align(TextAlign::Center)
                        .child(detail.clone())
                }))
                .into_any_element()
        } else {
            content_area
        };
        let document_tabs = self.render_document_tabs(&theme, cx);
        // Document tabs live inside the custom titlebar when it is visible;
        // without one (macOS fullscreen, server-side decorations) they fall
        // back to a standalone row above the editor column.
        let (titlebar_tabs, column_tabs) = if titlebar_height > 0.0 {
            (document_tabs, None)
        } else {
            (None, document_tabs)
        };
        let content_area = div()
            .id("editor-column")
            .w_full()
            .h_full()
            .flex_1()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .children(column_tabs.map(|tabs| {
                div()
                    .id("document-tabs-fallback")
                    .w_full()
                    .h(px(36.0))
                    .flex_shrink_0()
                    .flex()
                    .bg(theme.colors.dialog_surface)
                    .border_b(px(theme.dimensions.dialog_border_width))
                    .border_color(theme.colors.dialog_border)
                    .child(tabs)
                    .into_any_element()
            }))
            .child(content_area)
            .into_any_element();
        let content_area = if self.source_mode_fallback_required && !self.code_tab_active() {
            div()
                .id("source-mode-fallback-container")
                .w_full()
                .h_full()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("source-mode-fallback-notice")
                        .w_full()
                        .flex_shrink_0()
                        .px(px(16.0))
                        .py(px(9.0))
                        .border_b(px(1.0))
                        .border_color(theme.colors.callout_warning_border)
                        .bg(theme.colors.callout_warning_bg)
                        .text_size(px(theme.typography.text_size * 0.82))
                        .text_color(theme.colors.text_default)
                        .child(strings.source_mode_fallback_message.clone()),
                )
                .child(div().w_full().flex_1().min_h(px(0.0)).child(content_area))
                .into_any_element()
        } else {
            content_area
        };

        let body_font_family = if fonts.markdown_family == "theme" {
            &theme.typography.body_font_family
        } else {
            &fonts.markdown_family
        };
        let base = div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .relative()
            .bg(theme.colors.editor_background)
            .font(editor_text_font(body_font_family))
            .on_mouse_move(cx.listener(Self::on_workspace_resize_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(Self::on_workspace_resize_mouse_up),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(Self::on_workspace_resize_mouse_up),
            )
            .capture_action(cx.listener(Self::on_copy_capture))
            .capture_action(cx.listener(Self::on_cut_capture))
            .capture_action(cx.listener(Self::on_delete_capture))
            .capture_action(cx.listener(Self::on_delete_back_capture))
            .capture_key_down(cx.listener(Self::on_editor_key_down_capture))
            .can_drop(|dragged, _window, _cx| dragged.is::<ExternalPaths>())
            .on_drop::<ExternalPaths>(cx.listener(Self::on_external_paths_drop))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_save_document))
            .on_action(cx.listener(Self::on_save_document_as))
            .on_action(cx.listener(Self::on_export_html))
            .on_action(cx.listener(Self::on_export_pdf))
            .on_action(cx.listener(Self::on_export_png))
            .on_action(cx.listener(Self::on_quit_application))
            .on_action(cx.listener(Self::on_close_window))
            .on_action(cx.listener(Self::on_toggle_view_mode_action))
            .on_action(cx.listener(Self::on_find_in_document))
            .on_action(cx.listener(Self::on_find_next_match))
            .on_action(cx.listener(Self::on_find_previous_match))
            .on_action(cx.listener(Self::on_toggle_workspace_action))
            .on_action(cx.listener(Self::on_select_tab_index))
            .on_action(cx.listener(Self::on_quick_open_action))
            .on_action(cx.listener(Self::on_open_command_palette))
            .on_action(cx.listener(Self::on_cursor_history_back))
            .on_action(cx.listener(Self::on_cursor_history_forward))
            .on_action(cx.listener(Self::on_copy_as_html))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::on_page_up))
            .on_action(cx.listener(Self::on_page_down))
            .on_action(cx.listener(Self::on_jump_to_top))
            .on_action(cx.listener(Self::on_jump_to_bottom))
            .on_action(cx.listener(Self::on_dismiss_transient_ui))
            .on_action(cx.listener(Self::on_install_cli_tool))
            .on_action(cx.listener(Self::on_uninstall_cli_tool));
        // Fetch menus + collect labels once for both renderers; previously each
        // of render_in_window_menu_bar / render_in_window_menu_panel called
        // cx.get_menus() and walked menus.iter().map(|m| m.name.to_string())
        // independently — two redundant Vec<OwnedMenu> + two redundant
        // Vec<String>-of-N-allocations per frame.
        let menus = supports_in_window_menu()
            .then(|| cx.get_menus())
            .flatten()
            .filter(|m| !m.is_empty());
        let menu_labels: Vec<SharedString> = menus
            .as_ref()
            .map(|m| m.iter().map(|menu| menu.name.clone()).collect())
            .unwrap_or_default();
        // Windows：一级菜单入口是标题栏左侧的汉堡按钮，不再单占一行。
        let hamburger_menu = (supports_hamburger_menu() && menus.is_some())
            .then(|| self.render_hamburger_menu_button(&theme, cx));
        let base = if let Some(titlebar) = render_custom_titlebar(
            "editor-titlebar",
            format!("Velora - {}", self.workspace_breadcrumb()).into(),
            hamburger_menu,
            titlebar_tabs,
            &theme,
            window,
            cx,
            Self::on_titlebar_close,
        ) {
            base.child(titlebar)
        } else {
            base
        };
        let base = if supports_menu_bar_row() {
            if let Some(menu_bar) = self.render_in_window_menu_bar(
                &theme,
                cx,
                menus.as_deref(),
                &menu_labels,
                titlebar_height,
            ) {
                base.child(menu_bar)
            } else {
                base
            }
        } else {
            base
        };
        let workspace_width =
            self.current_workspace_panel_width(f32::from(window.viewport_size().width), cx);
        let workspace_panel =
            self.render_workspace_panel(&theme, &strings, workspace_width, window, cx);
        let mut main_content = div()
            .w_full()
            .flex_1()
            .min_h(px(0.0))
            .pt(px(titlebar_height + menu_bar_height))
            .flex()
            .min_w(px(0.0))
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                // 指针跑到浮层右边的正文里就收回（浮层宽度 = 窄条 + 面板）。
                if event.position.x > px(SIDEBAR_RAIL_WIDTH_PX + workspace_width) {
                    this.set_sidebar_peek(false, cx);
                }
            }));
        if self.workspace.is_open {
            // 展开：窄条与面板占位，正文被挤到右边。
            main_content = main_content.child(self.render_activity_rail(&theme, cx));
            if let Some(workspace_panel) = workspace_panel {
                main_content = main_content.child(workspace_panel);
            }
            main_content = main_content.child(content_area);
        } else {
            // 收起：整条侧边栏不占布局，正文占满整宽。指针贴到左边缘时整条侧边栏作为
            // 浮层滑出、盖在正文上（不挤压排版），移开带动画收回。顶边和展开时对齐
            // （标题栏 + 菜单栏之下），否则浮层会从窗口最顶上冒出来，盖住红绿灯和
            // 标签栏。
            let sidebar_top = px(titlebar_height + menu_bar_height);
            main_content = main_content.child(content_area);
            main_content = main_content.child(
                div()
                    .id("sidebar-auto-hide-edge")
                    .debug_selector(|| "sidebar-auto-hide-edge".to_string())
                    .absolute()
                    .left_0()
                    .top(sidebar_top)
                    .bottom_0()
                    .w(px(SIDEBAR_AUTO_HIDE_EDGE_PX))
                    .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                        // 进出贴边区都先递增 generation 作废挂着的停留定时器：
                        // 进入时换发新定时器，停留满才唤出；离开/再进入则让旧
                        // 定时器到点也不生效（防误触，见 SIDEBAR_PEEK_DWELL）。
                        this.sidebar_edge_dwell_generation =
                            this.sidebar_edge_dwell_generation.wrapping_add(1);
                        if *hovered {
                            let generation = this.sidebar_edge_dwell_generation;
                            let dwell = super::render::SIDEBAR_PEEK_DWELL;
                            cx.spawn(async move |editor, cx| {
                                cx.background_executor().timer(dwell).await;
                                _ = editor.update(cx, |editor, cx| {
                                    if editor.sidebar_edge_dwell_generation == generation {
                                        editor.set_sidebar_peek(true, cx);
                                    }
                                });
                            })
                            .detach();
                        }
                    })),
            );
            if let Some(workspace_panel) = workspace_panel {
                let overlay_width = px(SIDEBAR_RAIL_WIDTH_PX + workspace_width);
                let overlay = div()
                    .id("sidebar-auto-hide-overlay")
                    .debug_selector(|| "sidebar-auto-hide-overlay".to_string())
                    .absolute()
                    .left_0()
                    .top(sidebar_top)
                    .bottom_0()
                    // 显式宽度：绝对定位下不给宽度会按父级拉伸，鼠标移到正文时仍算
                    // 「在浮层内」，退出事件永远不触发。宽度 = 窄条 + 面板。
                    .w(overlay_width)
                    .flex()
                    .border_r(px(1.0))
                    .border_color(theme.colors.dialog_border)
                    .child(self.render_activity_rail(&theme, cx))
                    .child(workspace_panel);
                // 唤出滑入 / 收回滑出都用负 left 把浮层整体推到左边界外：
                // `with_animation` 在元素每次挂载时从头播放（滑入/滑出是两个
                // 不同 id 的包装，切换状态即重播），动画结束后每帧按 delta=1
                // 收敛在终态。收回动画期间（sidebar_overlay_closing）浮层仍
                // 挂载，播完由 workspace.rs 的定时器卸载。
                let layer = if self.sidebar_peek {
                    overlay
                        .with_animation(
                            "sidebar-overlay-slide-in",
                            Animation::new(SIDEBAR_SLIDE_DURATION).with_easing(ease_out_quint()),
                            move |slide, delta| slide.left(overlay_width * (delta - 1.0)),
                        )
                        .into_any_element()
                } else {
                    debug_assert!(self.sidebar_overlay_closing, "面板此时只应随动画挂载");
                    overlay
                        .with_animation(
                            "sidebar-overlay-slide-out",
                            Animation::new(SIDEBAR_SLIDE_DURATION).with_easing(quadratic),
                            move |slide, delta| slide.left(-overlay_width * delta),
                        )
                        .into_any_element()
                };
                main_content = main_content.child(layer);
            }
        }
        let base = base.child(main_content);
        let base = if let Some(status_bar) = self.render_status_bar(&theme, &strings, window, cx) {
            base.child(status_bar)
        } else {
            base
        };
        let base = if let Some(hamburger_list) = menus.as_deref().and_then(|menus| {
            self.render_hamburger_menu_panel(&theme, cx, menus, titlebar_height)
        }) {
            base.child(hamburger_list)
        } else {
            base
        };
        let base = if let Some(open_index) = self.menu_bar_open {
            let dimensions = &theme.dimensions;
            let origin = if self.hamburger_menu_open && supports_hamburger_menu() {
                hamburger_menu_item_panel_origin(
                    open_index,
                    titlebar_height,
                    &menu_labels,
                    dimensions,
                )
            } else {
                MenuPanelOrigin {
                    panel_left: menu_panel_left(open_index, &menu_labels, dimensions),
                    panel_top: titlebar_height,
                }
            };
            if let Some(menu_panel) = self.render_in_window_menu_panel(
                &theme,
                cx,
                menus.as_deref(),
                origin,
                f32::from(window.viewport_size().height.max(px(1.0))),
            ) {
                base.child(menu_panel)
            } else {
                base
            }
        } else {
            base
        };
        let base = if let Some(context_menu) = self.render_context_menu_overlay(&theme, cx) {
            base.child(context_menu)
        } else {
            base
        };
        let base =
            if let Some(menu) = self.render_workspace_context_menu_overlay(&theme, window, cx) {
                base.child(menu)
            } else {
                base
            };
        let base = if let Some(menu) = self.render_tab_context_menu_overlay(&theme, window, cx) {
            base.child(menu)
        } else {
            base
        };
        let base = if let Some(table_dialog) = self.render_table_insert_dialog_overlay(&theme, cx) {
            base.child(table_dialog)
        } else {
            base
        };
        if let Some(kind) = self.info_dialog {
            base.child(self.render_info_dialog_overlay(&theme, kind, cx))
        } else if self.modal_is_open() {
            match self.render_modal_overlay(&theme, cx) {
                Some(overlay) => base.child(overlay),
                None => base,
            }
        } else if self.show_drop_replace_dialog {
            base.child(self.render_drop_replace_overlay(&theme, cx))
        } else if self.show_unsaved_changes_dialog {
            base.child(self.render_unsaved_changes_overlay(&theme, cx))
        } else if self.pending_folder_choice.is_some() {
            base.child(self.render_folder_choice_overlay(&theme, cx).into_any_element())
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NoRecentFiles, RenderedRowSpacingInfo, callout_row_top_gap, editor_text_font,
        focus_mode_row_opacity, hamburger_menu_item_panel_origin, hamburger_menu_panel_top,
        hamburger_menu_row_top, import_menu_split_index, in_window_menu_bar_height_for_target_os,
        menu_bar_button_width, menu_items_visual_height_with_gaps, menu_panel_left,
        menu_panel_width_for_labels, owned_menu_item_labels, rendered_row_top_gap,
        scrollable_import_menu_scroll_height, submenu_bridge_geometry,
        supports_hamburger_menu_for_target_os, supports_in_window_menu_for_target_os,
        supports_menu_bar_row_for_target_os, tibetan_font_fallbacks_for_target_os,
        typewriter_target_scroll_offset,
    };
    use crate::components::{AddLanguageConfig, AddThemeConfig};
    use crate::theme::Theme;
    use gpui::{OwnedMenu, OwnedMenuItem};
    use uuid::Uuid;

    fn disabled_menu_action(name: &str) -> OwnedMenuItem {
        OwnedMenuItem::Action {
            name: name.into(),
            action: Box::new(NoRecentFiles),
            os_action: None,
        }
    }

    fn add_theme_menu_action() -> OwnedMenuItem {
        OwnedMenuItem::Action {
            name: "Add Theme Config".into(),
            action: Box::new(AddThemeConfig),
            os_action: None,
        }
    }

    fn add_language_menu_action() -> OwnedMenuItem {
        OwnedMenuItem::Action {
            name: "Add Language Config".into(),
            action: Box::new(AddLanguageConfig),
            os_action: None,
        }
    }

    #[test]
    fn contiguous_quote_rows_collapse_inter_row_gap() {
        let group = Uuid::new_v4();
        let gap = rendered_row_top_gap(
            Some(RenderedRowSpacingInfo {
                quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            },
            4.0,
            true,
        );
        assert_eq!(gap, 0.0);
    }

    #[test]
    fn focus_mode_fades_only_rows_outside_the_focused_group() {
        assert_eq!(focus_mode_row_opacity(true, Some(3), 2, 5), 1.0);
        assert_eq!(focus_mode_row_opacity(true, Some(3), 0, 2), 0.38);
        assert_eq!(focus_mode_row_opacity(false, Some(3), 0, 2), 1.0);
        assert_eq!(focus_mode_row_opacity(true, None, 0, 2), 1.0);
    }

    #[test]
    fn typewriter_scroll_centers_caret_within_document_limits() {
        assert_eq!(
            typewriter_target_scroll_offset(-120.0, 400.0, 600.0, 500.0),
            -320.0
        );
        assert_eq!(
            typewriter_target_scroll_offset(0.0, 400.0, 200.0, 500.0),
            0.0
        );
        assert_eq!(
            typewriter_target_scroll_offset(-200.0, 400.0, 700.0, 300.0),
            -300.0
        );
    }

    #[test]
    fn editor_text_font_keeps_system_ui_as_primary_family() {
        assert_eq!(
            editor_text_font(".SystemUIFont").family.to_string(),
            ".SystemUIFont"
        );
    }

    #[test]
    fn tibetan_font_fallbacks_prioritize_platform_defaults() {
        assert_eq!(
            tibetan_font_fallbacks_for_target_os("windows")
                .first()
                .map(String::as_str),
            Some("Microsoft Himalaya")
        );
        assert_eq!(
            tibetan_font_fallbacks_for_target_os("macos")
                .first()
                .map(String::as_str),
            Some("Kailasa")
        );
        assert_eq!(
            tibetan_font_fallbacks_for_target_os("linux")
                .first()
                .map(String::as_str),
            Some("Noto Serif Tibetan")
        );
        assert_eq!(
            tibetan_font_fallbacks_for_target_os("unknown")
                .first()
                .map(String::as_str),
            Some("Noto Serif Tibetan")
        );
    }

    #[test]
    fn nested_quote_separator_row_keeps_outer_group_gap_collapsed() {
        let group = Uuid::new_v4();
        let gap = rendered_row_top_gap(
            Some(RenderedRowSpacingInfo {
                quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            },
            4.0,
            true,
        );
        assert_eq!(gap, 0.0);
    }

    #[test]
    fn distinct_quote_groups_keep_default_gap() {
        let gap = rendered_row_top_gap(
            Some(RenderedRowSpacingInfo {
                quote_group_anchor: Some(Uuid::new_v4()),
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                quote_group_anchor: Some(Uuid::new_v4()),
                ..RenderedRowSpacingInfo::default()
            },
            4.0,
            true,
        );
        assert_eq!(gap, 4.0);
    }

    #[test]
    fn non_quote_rows_keep_default_gap() {
        let gap = rendered_row_top_gap(
            Some(RenderedRowSpacingInfo {
                quote_group_anchor: None,
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                quote_group_anchor: Some(Uuid::new_v4()),
                ..RenderedRowSpacingInfo::default()
            },
            4.0,
            true,
        );
        assert_eq!(gap, 4.0);
    }

    #[test]
    fn rendered_headings_and_lists_have_distinct_vertical_rhythm() {
        let paragraph = RenderedRowSpacingInfo::default();
        let heading = RenderedRowSpacingInfo {
            heading_level: Some(1),
            ..paragraph
        };
        let list_item = RenderedRowSpacingInfo {
            is_list_item: true,
            ..paragraph
        };
        assert_eq!(
            rendered_row_top_gap(Some(paragraph), heading, 8.0, true),
            19.2
        );
        assert_eq!(
            rendered_row_top_gap(Some(heading), paragraph, 8.0, true),
            6.0
        );
        assert_eq!(
            rendered_row_top_gap(Some(list_item), list_item, 8.0, true),
            4.0
        );
        assert_eq!(
            rendered_row_top_gap(Some(paragraph), heading, 8.0, false),
            8.0
        );
    }

    #[test]
    fn callout_inner_spacing_uses_header_and_body_tokens() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;

        let header_gap = callout_row_top_gap(
            Some(RenderedRowSpacingInfo {
                is_callout_header: true,
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo::default(),
            dimensions,
        );
        let body_gap = callout_row_top_gap(
            Some(RenderedRowSpacingInfo {
                is_callout_header: false,
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo::default(),
            dimensions,
        );

        assert_eq!(header_gap, dimensions.callout_header_margin_bottom);
        assert_eq!(body_gap, dimensions.callout_body_gap);
    }

    #[test]
    fn nested_quote_rows_inside_callout_collapse_body_gap() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let group = Uuid::new_v4();

        let gap = callout_row_top_gap(
            Some(RenderedRowSpacingInfo {
                is_callout_header: false,
                visible_quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            }),
            RenderedRowSpacingInfo {
                visible_quote_group_anchor: Some(group),
                ..RenderedRowSpacingInfo::default()
            },
            dimensions,
        );

        assert_eq!(gap, 0.0);
    }

    #[test]
    fn menu_button_width_expands_for_long_ascii_labels() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;

        assert_eq!(
            menu_bar_button_width("文件", dimensions),
            dimensions.menu_bar_button_width
        );
        assert!(menu_bar_button_width("Language", dimensions) > dimensions.menu_bar_button_width);
    }

    #[test]
    fn in_window_menu_is_enabled_for_every_target_except_macos() {
        for target_os in [
            "windows",
            "linux",
            "freebsd",
            "openbsd",
            "netbsd",
            "dragonfly",
            "solaris",
            "illumos",
            "android",
            "unknown",
        ] {
            assert!(
                supports_in_window_menu_for_target_os(target_os),
                "{target_os} should use the in-window fallback menu"
            );
        }
        assert!(!supports_in_window_menu_for_target_os("macos"));
    }

    #[test]
    fn windows_moves_the_menu_row_into_the_titlebar() {
        // 菜单栏那一行只在 Linux/FreeBSD 这类没有系统菜单栏的桌面保留；
        // Windows 改成标题栏里的汉堡按钮。
        assert!(supports_menu_bar_row_for_target_os("linux"));
        assert!(!supports_menu_bar_row_for_target_os("windows"));
        assert!(!supports_menu_bar_row_for_target_os("macos"));

        assert!(supports_hamburger_menu_for_target_os("windows"));
        assert!(!supports_hamburger_menu_for_target_os("linux"));
        assert!(!supports_hamburger_menu_for_target_os("macos"));
    }

    #[test]
    fn hamburger_item_panel_aligns_with_the_hovered_row() {
        let dimensions = Theme::default_theme().dimensions;
        let labels = vec!["File".to_string(), "Export".to_string()];
        let titlebar_height = 34.0;

        let origin = hamburger_menu_item_panel_origin(1, titlebar_height, &labels, &dimensions);        let list_width = menu_panel_width_for_labels(&labels, &dimensions);
        assert_eq!(
            origin.panel_left,
            dimensions.menu_bar_padding_x + list_width + dimensions.menu_panel_gap
        );

        // 条目面板第一行的 y 必须跟列表里被划过的第 1 行重合。
        let first_item_row_top =
            origin.panel_top + dimensions.menu_panel_top + dimensions.menu_panel_padding;
        assert_eq!(
            first_item_row_top,
            hamburger_menu_row_top(1, titlebar_height, &dimensions)
        );
        // 行越往下越高，且第二行比第一行低一行的高度。
        let row_delta = hamburger_menu_row_top(1, titlebar_height, &dimensions)
            - hamburger_menu_row_top(0, titlebar_height, &dimensions);
        assert_eq!(row_delta, dimensions.menu_item_height + dimensions.menu_panel_gap);
    }

    #[test]
    fn hamburger_list_hangs_right_below_the_button() {
        let dimensions = Theme::default_theme().dimensions;
        let titlebar_height = 36.0;
        let panel_top = hamburger_menu_panel_top(titlebar_height, &dimensions);
        let button_bottom = (titlebar_height + dimensions.menu_bar_button_height) / 2.0;

        // 缝就是 menu_bar_gap（默认 2px），不是几十像素。
        assert_eq!(panel_top, button_bottom + dimensions.menu_bar_gap);
        assert_eq!(panel_top, 32.0);

        // 与标题栏高度无关：标题栏再高，缝也不会跟着长。
        let tall_panel_top = hamburger_menu_panel_top(titlebar_height + 24.0, &dimensions);
        assert_eq!(
            tall_panel_top - panel_top,
            12.0,
            "标题栏每高 24px，面板只下移 12px（缝不变）"
        );

        // 旧写法（标题栏下沿 + menu_panel_top）会空出 30px：确认已经不再用它。
        assert!(dimensions.menu_panel_top > 10.0);
        assert!(panel_top < titlebar_height + dimensions.menu_panel_top);
    }

    #[test]
    fn in_window_menu_height_depends_on_platform_and_menu_presence() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;

        assert_eq!(
            in_window_menu_bar_height_for_target_os("linux", true, dimensions),
            dimensions.menu_bar_height
        );
        // Windows 不再单占一行菜单栏（改成标题栏里的汉堡按钮）。
        assert_eq!(
            in_window_menu_bar_height_for_target_os("windows", true, dimensions),
            0.0
        );
        assert_eq!(
            in_window_menu_bar_height_for_target_os("linux", false, dimensions),
            0.0
        );
        assert_eq!(
            in_window_menu_bar_height_for_target_os("macos", true, dimensions),
            0.0
        );
    }

    #[test]
    fn menu_panel_left_uses_accumulated_dynamic_button_widths() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let labels = vec![
            "File".to_string(),
            "Language".to_string(),
            "Theme".to_string(),
            "Help".to_string(),
        ];

        let left = menu_panel_left(2, &labels, dimensions);
        let expected = dimensions.menu_bar_padding_x
            + menu_bar_button_width("File", dimensions)
            + dimensions.menu_bar_gap
            + menu_bar_button_width("Language", dimensions)
            + dimensions.menu_bar_gap;
        let old_fixed_left = dimensions.menu_bar_padding_x
            + 2.0 * (dimensions.menu_bar_button_width + dimensions.menu_bar_gap);

        assert_eq!(left, expected);
        assert!(left > old_fixed_left);
    }

    #[test]
    fn menu_panel_width_expands_for_long_recent_paths() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let short_labels = vec!["Save".to_string()];
        let long_labels = vec![r"C:\Users\someone\Documents\Very Long Folder\notes.md".to_string()];

        assert_eq!(
            menu_panel_width_for_labels(&short_labels, dimensions),
            dimensions.menu_panel_width
        );
        assert!(
            menu_panel_width_for_labels(&long_labels, dimensions) > dimensions.menu_panel_width
        );
    }

    #[test]
    fn import_menu_split_detects_theme_and_language_import_tails() {
        let theme_items = vec![
            disabled_menu_action("velora"),
            OwnedMenuItem::Separator,
            add_theme_menu_action(),
        ];
        let language_items = vec![
            disabled_menu_action("English"),
            OwnedMenuItem::Separator,
            add_language_menu_action(),
        ];
        let regular_items = vec![
            disabled_menu_action("Open"),
            OwnedMenuItem::Separator,
            disabled_menu_action("Save"),
        ];
        let malformed_import_items = vec![disabled_menu_action("velora"), add_theme_menu_action()];

        assert_eq!(import_menu_split_index(&theme_items), Some(1));
        assert_eq!(import_menu_split_index(&language_items), Some(1));
        assert_eq!(import_menu_split_index(&regular_items), None);
        assert_eq!(import_menu_split_index(&malformed_import_items), None);
    }

    #[test]
    fn scrollable_import_menu_height_caps_visible_items_and_clamps_to_viewport() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let scroll_items = (0..20)
            .map(|index| disabled_menu_action(&format!("Custom Theme {index}")))
            .collect::<Vec<_>>();
        let footer_items = vec![OwnedMenuItem::Separator, add_theme_menu_action()];
        let expected_large_height =
            menu_items_visual_height_with_gaps(&scroll_items[..12], dimensions);
        let full_scroll_content_height =
            menu_items_visual_height_with_gaps(&scroll_items, dimensions);
        let footer_height = menu_items_visual_height_with_gaps(&footer_items, dimensions);

        let large_height = scrollable_import_menu_scroll_height(
            &scroll_items,
            &footer_items,
            2000.0,
            0.0,
            dimensions,
        );
        let small_height = scrollable_import_menu_scroll_height(
            &scroll_items,
            &footer_items,
            180.0,
            0.0,
            dimensions,
        );

        assert!((large_height - expected_large_height).abs() < f32::EPSILON);
        assert!(full_scroll_content_height > large_height);
        assert!(large_height < expected_large_height + footer_height);
        assert!(small_height < large_height);
        assert!(small_height >= dimensions.menu_item_height);
    }

    #[test]
    fn submenu_bridge_spans_parent_child_menu_gap() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let labels = vec!["File".to_string()];
        let items = vec![
            OwnedMenuItem::Separator,
            OwnedMenuItem::Submenu(OwnedMenu {
                name: "Recent".into(),
                items: vec![OwnedMenuItem::Action {
                    name: r"C:\Users\someone\Documents\notes.md".into(),
                    action: Box::new(NoRecentFiles),
                    os_action: None,
                }],
            }),
        ];
        let submenu_labels = match &items[1] {
            OwnedMenuItem::Submenu(submenu) => owned_menu_item_labels(&submenu.items),
            _ => Vec::new(),
        };

        let bridge = submenu_bridge_geometry(
            menu_panel_left(0, &labels, dimensions),
            &items,
            1,
            &submenu_labels,
            dimensions,
        )
            .expect("submenu bridge geometry should be available");
        let submenu_width = menu_panel_width_for_labels(&submenu_labels, dimensions);

        assert_eq!(
            bridge.left,
            dimensions.menu_bar_padding_x + dimensions.menu_panel_width
        );
        assert_eq!(bridge.width, dimensions.menu_panel_gap + submenu_width);
        assert!(bridge.height > dimensions.menu_item_height);
        let item_top = dimensions.menu_panel_top
            + dimensions.menu_panel_padding
            + dimensions.menu_separator_height
            + dimensions.menu_separator_margin_y * 2.0
            + dimensions.menu_panel_gap;
        assert!(bridge.top < item_top);
        assert!(bridge.top >= dimensions.menu_panel_top);
    }

    #[test]
    fn submenu_bridge_uses_dynamic_main_menu_width() {
        let theme = Theme::default_theme();
        let dimensions = &theme.dimensions;
        let labels = vec!["File".to_string()];
        let items = vec![OwnedMenuItem::Submenu(OwnedMenu {
            name: "Open Recently Used Markdown File".into(),
            items: vec![OwnedMenuItem::Action {
                name: r"C:\Users\someone\Documents\Very Long Folder\notes.md".into(),
                action: Box::new(NoRecentFiles),
                os_action: None,
            }],
        })];
        let submenu_labels = match &items[0] {
            OwnedMenuItem::Submenu(submenu) => owned_menu_item_labels(&submenu.items),
            _ => Vec::new(),
        };

        let bridge = submenu_bridge_geometry(
            menu_panel_left(0, &labels, dimensions),
            &items,
            0,
            &submenu_labels,
            dimensions,
        )
            .expect("submenu bridge geometry should be available");

        assert!(bridge.left > dimensions.menu_bar_padding_x + dimensions.menu_panel_width);
        assert!(bridge.width > dimensions.menu_panel_gap + dimensions.menu_panel_width);
    }
}
