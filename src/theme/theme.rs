//! Theme data structures and defaults.
//! 内置主题的显示名称为 velora。
//!
//! The theme layer keeps visual tokens out of editor logic so rendering and
//! interaction code can depend on stable semantic names instead of hard-coded
//! values.

pub(super) use std::path::Path;
pub(super) use std::sync::Arc;

pub(super) use anyhow::{Context as _, bail};
pub(super) use gpui::{App, FontWeight, Global, Hsla, WindowAppearance, rgba};
pub(super) use serde::{Deserialize, Deserializer, Serialize};
pub(super) use serde_json::{Map, Value};

pub(super) use crate::config::{
    VeloraConfigDirs, merge_non_empty_json_values, object_without_empty_values,
    prune_empty_json_values, read_json_or_jsonc, sanitize_config_file_stem,
};

/// Serializable font weight that maps to GPUI's [`FontWeight`] constants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontWeightDef {
    /// Thin font weight.
    Thin,
    /// Light font weight.
    Light,
    /// Normal font weight.
    Normal,
    /// Medium font weight.
    Medium,
    /// Semibold font weight.
    Semibold,
    /// Bold font weight.
    Bold,
    /// Extra-bold font weight.
    Extrabold,
    /// Black font weight.
    Black,
}

impl FontWeightDef {
    /// Converts the serialized theme value into GPUI's runtime font weight.
    pub fn to_font_weight(&self) -> FontWeight {
        match self {
            FontWeightDef::Thin => FontWeight::THIN,
            FontWeightDef::Light => FontWeight::LIGHT,
            FontWeightDef::Normal => FontWeight::NORMAL,
            FontWeightDef::Medium => FontWeight::MEDIUM,
            FontWeightDef::Semibold => FontWeight::SEMIBOLD,
            FontWeightDef::Bold => FontWeight::BOLD,
            FontWeightDef::Extrabold => FontWeight::EXTRA_BOLD,
            FontWeightDef::Black => FontWeight::BLACK,
        }
    }
}

/// All configurable colors for the editor UI.
#[derive(Debug, Clone, Serialize)]
pub struct ThemeColors {
    /// Background of the editor scroll area (behind all blocks).
    pub editor_background: Hsla,
    /// Background of the focused raw block in source-editing mode.
    pub source_mode_block_bg: Hsla,
    /// Background used for visible Markdown comment blocks.
    pub comment_bg: Hsla,
    /// Default paragraph / body text colour.
    pub text_default: Hsla,
    /// Inline link text colour in rendered mode.
    pub text_link: Hsla,
    /// Placeholder text shown in empty focused blocks.
    pub text_placeholder: Hsla,
    /// H1 heading text colour.
    pub text_h1: Hsla,
    /// H2 heading text colour.
    pub text_h2: Hsla,
    /// H3 heading text colour.
    pub text_h3: Hsla,
    /// H4 heading text colour.
    pub text_h4: Hsla,
    /// H5 heading text colour.
    pub text_h5: Hsla,
    /// H6 heading text colour.
    pub text_h6: Hsla,
    /// H1 bottom-border colour.
    pub border_h1: Hsla,
    /// H2 bottom-border colour.
    pub border_h2: Hsla,
    /// Quote block text colour.
    pub text_quote: Hsla,
    /// Quote block left-border colour.
    pub border_quote: Hsla,
    /// Note callout background.
    pub callout_note_bg: Hsla,
    /// Note callout accent border/text colour.
    pub callout_note_border: Hsla,
    /// Tip callout background.
    pub callout_tip_bg: Hsla,
    /// Tip callout accent border/text colour.
    pub callout_tip_border: Hsla,
    /// Important callout background.
    pub callout_important_bg: Hsla,
    /// Important callout accent border/text colour.
    pub callout_important_border: Hsla,
    /// Warning callout background.
    pub callout_warning_bg: Hsla,
    /// Translucent fill for in-document search matches (roadmap B2).
    pub search_highlight_bg: Hsla,
    /// Deeper fill for the ACTIVE search match (the one jumps select).
    pub search_active_highlight_bg: Hsla,
    /// Warning callout accent border/text colour.
    pub callout_warning_border: Hsla,
    /// Caution callout background.
    pub callout_caution_bg: Hsla,
    /// Caution callout accent border/text colour.
    pub callout_caution_border: Hsla,
    /// Background of footnote definition grouping shells.
    pub footnote_bg: Hsla,
    /// Border colour of footnote definition grouping shells.
    pub footnote_border: Hsla,
    /// Background of the footnote ordinal badge.
    pub footnote_badge_bg: Hsla,
    /// Text colour of the footnote ordinal badge.
    pub footnote_badge_text: Hsla,
    /// Back-reference colour inside footnote headers.
    pub footnote_backref: Hsla,
    /// Border colour of interactive task-list checkboxes.
    pub task_checkbox_border: Hsla,
    /// Background of unchecked task-list checkboxes.
    pub task_checkbox_bg: Hsla,
    /// Background of checked task-list checkboxes.
    pub task_checkbox_checked_bg: Hsla,
    /// Checkmark colour inside checked task-list checkboxes.
    pub task_checkbox_check: Hsla,
    /// Colour of the separator block line.
    pub separator_color: Hsla,
    /// Background of inline code and code-block quads.
    pub code_bg: Hsla,
    /// Text colour inside code blocks.
    pub code_text: Hsla,
    /// Background of the focused code-block language input.
    pub code_language_input_bg: Hsla,
    /// Border colour of the focused code-block language input.
    pub code_language_input_border: Hsla,
    /// Text colour of the focused code-block language input.
    pub code_language_input_text: Hsla,
    /// Placeholder colour of the focused code-block language input.
    pub code_language_input_placeholder: Hsla,
    /// Syntax colour for comments inside code blocks.
    pub code_syntax_comment: Hsla,
    /// Syntax colour for keywords inside code blocks.
    pub code_syntax_keyword: Hsla,
    /// Syntax colour for strings inside code blocks.
    pub code_syntax_string: Hsla,
    /// Syntax colour for numbers inside code blocks.
    pub code_syntax_number: Hsla,
    /// Syntax colour for types and modules inside code blocks.
    pub code_syntax_type: Hsla,
    /// Syntax colour for functions and constructors inside code blocks.
    pub code_syntax_function: Hsla,
    /// Syntax colour for constants inside code blocks.
    pub code_syntax_constant: Hsla,
    /// Syntax colour for variables and parameters inside code blocks.
    pub code_syntax_variable: Hsla,
    /// Syntax colour for properties and attributes inside code blocks.
    pub code_syntax_property: Hsla,
    /// Syntax colour for operators inside code blocks.
    pub code_syntax_operator: Hsla,
    /// Syntax colour for punctuation inside code blocks.
    pub code_syntax_punctuation: Hsla,
    /// Markdown 源码高亮：一级标题（六级各有自己的颜色，保证区分度）。
    pub md_syntax_heading1: Hsla,
    /// Markdown 源码高亮：二级标题。
    pub md_syntax_heading2: Hsla,
    /// Markdown 源码高亮：三级标题。
    pub md_syntax_heading3: Hsla,
    /// Markdown 源码高亮：四级标题。
    pub md_syntax_heading4: Hsla,
    /// Markdown 源码高亮：五级标题。
    pub md_syntax_heading5: Hsla,
    /// Markdown 源码高亮：六级标题。
    pub md_syntax_heading6: Hsla,
    /// Markdown 源码高亮：结构记号（列表、引用、围栏、分隔线、`$$`）。
    pub md_syntax_marker: Hsla,
    /// Markdown 源码高亮：加粗内容（粗体字重 + 主题色）。
    pub md_syntax_strong: Hsla,
    /// Markdown 源码高亮：行内代码、公式正文与转义。
    pub md_syntax_code: Hsla,
    /// Markdown 源码高亮：链接文字。
    pub md_syntax_link_text: Hsla,
    /// Markdown 源码高亮：链接地址。
    pub md_syntax_link_url: Hsla,
    /// Markdown 源码高亮：标签、标注标签与脚注引用。
    pub md_syntax_label: Hsla,
    /// Border colour of native table cells.
    pub table_border: Hsla,
    /// Background of native table header cells.
    pub table_header_bg: Hsla,
    /// Subtle full-width highlight of the line the caret is on.
    pub current_line_bg: Hsla,
    /// Background wash for the bracket pair highlight around the caret.
    pub matching_bracket_bg: Hsla,
    /// Background of native table body cells.
    pub table_cell_bg: Hsla,
    /// Outline colour of the active native table cell.
    pub table_cell_active_outline: Hsla,
    /// Preview highlight colour for row/column table-axis selection bands.
    pub table_axis_preview_bg: Hsla,
    /// Selected highlight colour for row/column table-axis selection bands.
    pub table_axis_selected_bg: Hsla,
    /// Background of rendered-mode native table append controls.
    pub table_append_button_bg: Hsla,
    /// Hover background of rendered-mode native table append controls.
    pub table_append_button_hover: Hsla,
    /// Text colour of rendered-mode native table append controls.
    pub table_append_button_text: Hsla,
    /// Background of image placeholders in rendered mode.
    pub image_placeholder_bg: Hsla,
    /// Border colour of image placeholders in rendered mode.
    pub image_placeholder_border: Hsla,
    /// Text colour of image placeholders in rendered mode.
    pub image_placeholder_text: Hsla,
    /// Caption text colour shown below rendered images.
    pub image_caption_text: Hsla,
    /// Scrollbar thumb colour (auto-fading overlay).
    pub scrollbar_thumb: Hsla,
    /// Text-editing cursor (caret) colour.
    pub cursor: Hsla,
    /// Text-selection highlight colour.
    pub selection: Hsla,
    /// Semi-transparent backdrop behind the unsaved-changes dialog.
    pub dialog_backdrop: Hsla,
    /// Background of the unsaved-changes dialog.
    pub dialog_surface: Hsla,
    /// Border colour of the unsaved-changes dialog.
    pub dialog_border: Hsla,
    /// Title text colour in the unsaved-changes dialog.
    pub dialog_title: Hsla,
    /// Body text colour in the unsaved-changes dialog.
    pub dialog_body: Hsla,
    /// Muted / hint text colour in the unsaved-changes dialog.
    pub dialog_muted: Hsla,
    /// Primary (save-and-close) button background.
    pub dialog_primary_button_bg: Hsla,
    /// Primary button hover background.
    pub dialog_primary_button_hover: Hsla,
    /// Primary button text colour.
    pub dialog_primary_button_text: Hsla,
    /// Secondary (cancel) button background.
    pub dialog_secondary_button_bg: Hsla,
    /// Secondary button hover background.
    pub dialog_secondary_button_hover: Hsla,
    /// Secondary button text colour.
    pub dialog_secondary_button_text: Hsla,
    /// Danger (discard-and-close) button background.
    pub dialog_danger_button_bg: Hsla,
    /// Danger button hover background.
    pub dialog_danger_button_hover: Hsla,
    /// Danger button text colour.
    pub dialog_danger_button_text: Hsla,
    /// Background of the editor status bar.
    pub status_bar_background: Hsla,
    /// Primary text colour in the status bar.
    pub status_bar_text: Hsla,
    /// Dimmed/secondary text colour in the status bar.
    pub status_bar_text_dim: Hsla,
    /// Hover background for clickable status bar items.
    pub status_bar_button_hover: Hsla,
}

/// All configurable dimensions (paddings, gaps, sizes) for the editor UI.
#[derive(Debug, Clone, Serialize)]
pub struct ThemeDimensions {
    /// Padding around the editor content area.
    pub editor_padding: f32,
    /// Maximum width of the rendered Markdown writing column.
    pub writing_max_width: f32,
    /// Vertical gap between adjacent blocks.
    pub block_gap: f32,
    /// Minimum height of every block.
    pub block_min_height: f32,
    /// Vertical padding inside each block.
    pub block_padding_y: f32,
    /// Horizontal padding inside each block.
    pub block_padding_x: f32,
    /// Extra horizontal indent per nesting level (list items).
    pub nested_block_indent: f32,
    /// Gap between list marker and its text content.
    pub list_marker_gap: f32,
    /// Minimum width of the bullet list marker column.
    pub list_marker_width: f32,
    /// Minimum width of the ordered-list marker column.
    pub ordered_list_marker_width: f32,
    /// Width and height of the interactive task-list checkbox.
    pub task_checkbox_size: f32,
    /// Corner radius of the task-list checkbox.
    pub task_checkbox_radius: f32,
    /// Border width of the task-list checkbox.
    pub task_checkbox_border_width: f32,
    /// Checkmark font size inside the task-list checkbox.
    pub task_checkbox_check_size: f32,
    /// Extra padding below H1 text.
    pub h1_padding_bottom: f32,
    /// Margin below the H1 bottom border.
    pub h1_margin_bottom: f32,
    /// Width of the text-editing cursor (caret).
    pub cursor_width: f32,
    /// Thickness of the underline decoration.
    pub underline_thickness: f32,
    /// H1 bottom-border thickness.
    pub h1_border_width: f32,
    /// Quote block left-border thickness.
    pub quote_border_width: f32,
    /// Extra left padding between quote border and text.
    pub quote_padding_left: f32,
    /// Horizontal padding inside editor-level callout shells.
    pub callout_padding_x: f32,
    /// Vertical padding inside editor-level callout shells.
    pub callout_padding_y: f32,
    /// Vertical gap between callout body rows.
    pub callout_body_gap: f32,
    /// Corner radius of editor-level callout shells.
    pub callout_radius: f32,
    /// Accent border width of editor-level callout shells.
    pub callout_border_width: f32,
    /// Gap between callout icon and header text.
    pub callout_header_gap: f32,
    /// Vertical margin between the callout header row and the first body row.
    pub callout_header_margin_bottom: f32,
    /// Horizontal padding inside footnote grouping shells.
    pub footnote_padding_x: f32,
    /// Vertical padding inside footnote grouping shells.
    pub footnote_padding_y: f32,
    /// Corner radius of footnote grouping shells.
    pub footnote_radius: f32,
    /// Horizontal padding inside the footnote ordinal badge.
    pub footnote_badge_padding_x: f32,
    /// Vertical padding inside the footnote ordinal badge.
    pub footnote_badge_padding_y: f32,
    /// Thickness of the separator block line.
    pub separator_thickness: f32,
    /// Extra horizontal inset applied to separator blocks.
    pub separator_inset_x: f32,
    /// Vertical margin around separator blocks.
    pub separator_margin_y: f32,
    /// Vertical padding inside a code block.
    pub code_block_padding_y: f32,
    /// Horizontal padding inside a code block.
    pub code_block_padding_x: f32,
    /// Horizontal padding around inline code background quads.
    pub code_bg_pad_x: f32,
    /// Vertical padding around inline code background quads.
    pub code_bg_pad_y: f32,
    /// Corner radius for inline code background quads.
    pub code_bg_radius: f32,
    /// Width of the code-block language input.
    pub code_language_input_width: f32,
    /// Text layout height inside the code-block language input.
    pub code_language_input_height: f32,
    /// Horizontal padding inside the code-block language input.
    pub code_language_input_padding_x: f32,
    /// Vertical padding inside the code-block language input.
    pub code_language_input_padding_y: f32,
    /// Corner radius of the code-block language input.
    pub code_language_input_radius: f32,
    /// Border width of the code-block language input.
    pub code_language_input_border_width: f32,
    /// Gap between code text and the language input.
    pub code_language_input_gap: f32,
    /// Horizontal padding inside native table cells.
    pub table_cell_padding_x: f32,
    /// Vertical padding inside native table cells.
    pub table_cell_padding_y: f32,
    /// Minimum height of native table cells.
    pub table_cell_min_height: f32,
    /// Width of the append-column control and height of the append-row control.
    pub table_append_button_extent: f32,
    /// Inset padding around rendered-mode native table append controls.
    pub table_append_button_inset: f32,
    /// Invisible activation overlap that keeps append controls easy to hover.
    pub table_append_activation_band: f32,
    /// Corner radius of rendered images and image placeholders.
    pub image_radius: f32,
    /// 图片最大高度；0 表示按原图比例排版，不设固定高度上限。
    pub image_root_max_height: f32,
    /// 图片最大宽度；0 表示以正文容器宽度的 95% 作为缩放 100% 的基准。
    pub image_root_max_width: f32,
    /// Maximum height of rendered table-cell images.
    pub image_cell_max_height: f32,
    /// Default placeholder height for rendered root-paragraph images.
    pub image_root_placeholder_height: f32,
    /// Default placeholder height for rendered table-cell images.
    pub image_cell_placeholder_height: f32,
    /// Vertical gap between a rendered image and its caption.
    pub image_caption_gap: f32,
    /// Width of the custom scrollbar thumb.
    pub scrollbar_width: f32,
    /// Distance of the scrollbar thumb from the right edge.
    pub scrollbar_right: f32,
    /// Viewport width at which the content column starts shrinking.
    pub centered_shrink_start: f32,
    /// Viewport width at which the content column reaches minimum ratio.
    pub centered_shrink_end: f32,
    /// Minimum content-column width as a fraction of available width.
    pub centered_min_ratio: f32,
    /// Width of the unsaved-changes dialog.
    pub dialog_width: f32,
    /// Padding inside the unsaved-changes dialog.
    pub dialog_padding: f32,
    /// Gap between dialog sections.
    pub dialog_gap: f32,
    /// Corner radius of the unsaved-changes dialog.
    pub dialog_radius: f32,
    /// Border width of the unsaved-changes dialog.
    pub dialog_border_width: f32,
    /// Height of dialog action buttons.
    pub dialog_button_height: f32,
    /// Gap between dialog action buttons.
    pub dialog_button_gap: f32,
    /// Horizontal padding inside dialog action buttons.
    pub dialog_button_padding_x: f32,
    /// Height reserved for the in-window fallback menu bar.
    pub menu_bar_height: f32,
    /// Horizontal padding inside the in-window fallback menu bar.
    pub menu_bar_padding_x: f32,
    /// Vertical padding inside the in-window fallback menu bar.
    pub menu_bar_padding_y: f32,
    /// Gap between top-level menu buttons.
    pub menu_bar_gap: f32,
    /// Minimum width of each top-level menu button.
    pub menu_bar_button_width: f32,
    /// Height of each top-level menu button.
    pub menu_bar_button_height: f32,
    /// Horizontal padding inside top-level menu buttons.
    pub menu_bar_button_padding_x: f32,
    /// Corner radius of top-level menu buttons.
    pub menu_bar_button_radius: f32,
    /// Text size used by menu labels.
    pub menu_text_size: f32,
    /// Top position of the in-window fallback floating menu panel.
    pub menu_panel_top: f32,
    /// Width of the in-window fallback floating menu panel.
    pub menu_panel_width: f32,
    /// Padding inside floating menu panels.
    pub menu_panel_padding: f32,
    /// Gap between items inside floating menu panels.
    pub menu_panel_gap: f32,
    /// Corner radius of floating menu panels.
    pub menu_panel_radius: f32,
    /// Height of each floating menu item.
    pub menu_item_height: f32,
    /// Horizontal padding inside floating menu items.
    pub menu_item_padding_x: f32,
    /// Corner radius of floating menu items.
    pub menu_item_radius: f32,
    /// Horizontal margin around menu separators.
    pub menu_separator_margin_x: f32,
    /// Vertical margin around menu separators.
    pub menu_separator_margin_y: f32,
    /// Height of menu separators.
    pub menu_separator_height: f32,
    /// Width of the root insert context menu panel.
    pub context_menu_panel_width: f32,
    /// Width of the insert-submenu panel.
    pub context_menu_submenu_width: f32,
    /// Horizontal gap between a context menu and its submenu.
    pub context_menu_submenu_gap: f32,
    /// Width of the table-axis context menu panel.
    pub context_menu_axis_panel_width: f32,
    /// Maximum width of the table-insert dialog.
    pub table_insert_dialog_width: f32,
    /// Gap between table-insert stepper label and controls.
    pub table_insert_stepper_gap: f32,
    /// Size of table-insert stepper buttons.
    pub table_insert_stepper_button_size: f32,
    /// Minimum width of the table-insert stepper value pill.
    pub table_insert_stepper_value_min_width: f32,
    /// Horizontal padding inside the table-insert stepper value pill.
    pub table_insert_stepper_value_padding_x: f32,
    /// Corner radius of table-insert stepper controls.
    pub table_insert_stepper_radius: f32,
    /// Left inset of the view-mode toggle.
    pub view_mode_toggle_left: f32,
    /// Bottom inset of the view-mode toggle.
    pub view_mode_toggle_bottom: f32,
    /// Horizontal padding inside the view-mode toggle.
    pub view_mode_toggle_padding_x: f32,
    /// Vertical padding inside the view-mode toggle.
    pub view_mode_toggle_padding_y: f32,
    /// Minimum width of the view-mode toggle.
    pub view_mode_toggle_min_width: f32,
    /// Corner radius of the view-mode toggle.
    pub view_mode_toggle_radius: f32,
    /// Border width of the view-mode toggle.
    pub view_mode_toggle_border_width: f32,
    /// Text size of the view-mode toggle.
    pub view_mode_toggle_text_size: f32,
    /// Height of the status bar.
    pub status_bar_height: f32,
    /// Horizontal padding inside the status bar.
    pub status_bar_padding_x: f32,
    /// Gap between items in the status bar.
    pub status_bar_item_gap: f32,
    /// Font size for status bar text.
    pub status_bar_text_size: f32,
}

/// All configurable typography settings (font sizes, weights, line heights).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeTypography {
    /// Body font used when the user chooses to follow the theme.
    #[serde(default = "default_body_font_family")]
    pub body_font_family: String,
    /// Heading font used by the writing theme.
    #[serde(default = "default_heading_font_family")]
    pub heading_font_family: String,
    /// Default body text font size.
    pub text_size: f32,
    /// Default body text line height as a ratio of font size.
    pub text_line_height: f32,
    /// Tracking between prose characters, in em; code retains its fixed spacing.
    #[serde(default)]
    pub text_letter_spacing: f32,
    /// H1 heading font size.
    pub h1_size: f32,
    /// H1 heading font weight.
    pub h1_weight: FontWeightDef,
    /// H2 heading font size.
    pub h2_size: f32,
    /// H2 heading font weight.
    pub h2_weight: FontWeightDef,
    /// H3 heading font size.
    pub h3_size: f32,
    /// H3 heading font weight.
    pub h3_weight: FontWeightDef,
    /// H4 heading font size.
    pub h4_size: f32,
    /// H4 heading font weight.
    pub h4_weight: FontWeightDef,
    /// H5 heading font size.
    pub h5_size: f32,
    /// H5 heading font weight.
    pub h5_weight: FontWeightDef,
    /// H6 heading font size.
    pub h6_size: f32,
    /// H6 heading font weight.
    pub h6_weight: FontWeightDef,
    /// Code-block text font size.
    pub code_size: f32,
    /// Dialog title font size.
    pub dialog_title_size: f32,
    /// Dialog title font weight.
    pub dialog_title_weight: FontWeightDef,
    /// Dialog body font size.
    pub dialog_body_size: f32,
    /// Dialog body font weight.
    pub dialog_body_weight: FontWeightDef,
    /// Dialog button font size.
    pub dialog_button_size: f32,
    /// Dialog button font weight.
    pub dialog_button_weight: FontWeightDef,
}

fn default_body_font_family() -> String {
    ".SystemUIFont".into()
}

fn default_heading_font_family() -> String {
    ".SystemUIFont".into()
}

/// Placeholder text shown in empty interactive elements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placeholders {
    /// Text shown in an empty focused block.
    pub empty_editing: String,
}

pub use manager_parts::*;
pub use presets::*;


mod de;
mod manager_parts;
mod presets;

#[cfg(test)]
mod tests;
