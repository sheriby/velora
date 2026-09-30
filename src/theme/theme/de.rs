use super::*;

/// Deserialization adapter for `ThemeColors` with backward-compatible defaults.
#[derive(Deserialize)]
struct ThemeColorsDe {
    editor_background: Hsla,
    source_mode_block_bg: Option<Hsla>,
    block_focused_bg: Option<Hsla>,
    comment_bg: Option<Hsla>,
    text_default: Hsla,
    text_link: Option<Hsla>,
    text_placeholder: Hsla,
    text_h1: Hsla,
    text_h2: Hsla,
    text_h3: Hsla,
    text_h4: Hsla,
    text_h5: Hsla,
    text_h6: Hsla,
    border_h1: Hsla,
    border_h2: Option<Hsla>,
    text_quote: Hsla,
    border_quote: Hsla,
    callout_note_bg: Option<Hsla>,
    callout_note_border: Option<Hsla>,
    callout_tip_bg: Option<Hsla>,
    callout_tip_border: Option<Hsla>,
    callout_important_bg: Option<Hsla>,
    callout_important_border: Option<Hsla>,
    callout_warning_bg: Option<Hsla>,
    search_highlight_bg: Option<Hsla>,
    callout_warning_border: Option<Hsla>,
    callout_caution_bg: Option<Hsla>,
    callout_caution_border: Option<Hsla>,
    footnote_bg: Option<Hsla>,
    footnote_border: Option<Hsla>,
    footnote_badge_bg: Option<Hsla>,
    footnote_badge_text: Option<Hsla>,
    footnote_backref: Option<Hsla>,
    task_checkbox_border: Option<Hsla>,
    task_checkbox_bg: Option<Hsla>,
    task_checkbox_checked_bg: Option<Hsla>,
    task_checkbox_check: Option<Hsla>,
    separator_color: Option<Hsla>,
    code_bg: Option<Hsla>,
    code_text: Hsla,
    code_language_input_bg: Option<Hsla>,
    code_language_input_border: Option<Hsla>,
    code_language_input_text: Option<Hsla>,
    code_language_input_placeholder: Option<Hsla>,
    code_syntax_comment: Option<Hsla>,
    code_syntax_keyword: Option<Hsla>,
    code_syntax_string: Option<Hsla>,
    code_syntax_number: Option<Hsla>,
    code_syntax_type: Option<Hsla>,
    code_syntax_function: Option<Hsla>,
    code_syntax_constant: Option<Hsla>,
    code_syntax_variable: Option<Hsla>,
    code_syntax_property: Option<Hsla>,
    code_syntax_operator: Option<Hsla>,
    code_syntax_punctuation: Option<Hsla>,
    table_border: Option<Hsla>,
    table_header_bg: Option<Hsla>,
    current_line_bg: Option<Hsla>,
    matching_bracket_bg: Option<Hsla>,
    table_cell_bg: Option<Hsla>,
    table_cell_active_outline: Option<Hsla>,
    table_axis_preview_bg: Option<Hsla>,
    table_axis_selected_bg: Option<Hsla>,
    table_append_button_bg: Option<Hsla>,
    table_append_button_hover: Option<Hsla>,
    table_append_button_text: Option<Hsla>,
    image_placeholder_bg: Option<Hsla>,
    image_placeholder_border: Option<Hsla>,
    image_placeholder_text: Option<Hsla>,
    image_caption_text: Option<Hsla>,
    scrollbar_thumb: Hsla,
    cursor: Hsla,
    selection: Hsla,
    dialog_backdrop: Hsla,
    dialog_surface: Hsla,
    dialog_border: Hsla,
    dialog_title: Hsla,
    dialog_body: Hsla,
    dialog_muted: Hsla,
    dialog_primary_button_bg: Hsla,
    dialog_primary_button_hover: Hsla,
    dialog_primary_button_text: Hsla,
    dialog_secondary_button_bg: Hsla,
    dialog_secondary_button_hover: Hsla,
    dialog_secondary_button_text: Hsla,
    dialog_danger_button_bg: Hsla,
    dialog_danger_button_hover: Hsla,
    dialog_danger_button_text: Hsla,
    status_bar_background: Option<Hsla>,
    status_bar_text: Option<Hsla>,
    status_bar_text_dim: Option<Hsla>,
    status_bar_button_hover: Option<Hsla>,
}

impl<'de> Deserialize<'de> for ThemeColors {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = ThemeColorsDe::deserialize(deserializer)?;
        Ok(Self {
            editor_background: raw.editor_background,
            source_mode_block_bg: raw
                .source_mode_block_bg
                .or(raw.block_focused_bg)
                .unwrap_or_else(|| Hsla::from(rgba(0x313131ff))),
            comment_bg: raw
                .comment_bg
                .unwrap_or_else(|| Hsla::from(rgba(0xfbbf2426))),
            text_default: raw.text_default,
            text_link: raw
                .text_link
                .unwrap_or_else(|| Hsla::from(rgba(0x60a5faff))),
            text_placeholder: raw.text_placeholder,
            text_h1: raw.text_h1,
            text_h2: raw.text_h2,
            text_h3: raw.text_h3,
            text_h4: raw.text_h4,
            text_h5: raw.text_h5,
            text_h6: raw.text_h6,
            border_h1: raw.border_h1,
            border_h2: raw
                .border_h2
                .unwrap_or_else(|| Hsla::from(rgba(0xe0e0e0cc))),
            text_quote: raw.text_quote,
            border_quote: raw.border_quote,
            callout_note_bg: raw
                .callout_note_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x94a3b81f))),
            callout_note_border: raw
                .callout_note_border
                .unwrap_or_else(|| Hsla::from(rgba(0x94a3b4ff))),
            callout_tip_bg: raw
                .callout_tip_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x1d4ed81f))),
            callout_tip_border: raw
                .callout_tip_border
                .unwrap_or_else(|| Hsla::from(rgba(0x60a5faff))),
            callout_important_bg: raw
                .callout_important_bg
                .unwrap_or_else(|| Hsla::from(rgba(0xca8a041f))),
            callout_important_border: raw
                .callout_important_border
                .unwrap_or_else(|| Hsla::from(rgba(0xfbbf24ff))),
            callout_warning_bg: raw
                .callout_warning_bg
                .unwrap_or_else(|| Hsla::from(rgba(0xfb71851f))),
            search_highlight_bg: raw
                .search_highlight_bg
                .unwrap_or_else(|| Hsla::from(rgba(0xffe06638))),
            callout_warning_border: raw
                .callout_warning_border
                .unwrap_or_else(|| Hsla::from(rgba(0xfb7185ff))),
            callout_caution_bg: raw
                .callout_caution_bg
                .unwrap_or_else(|| Hsla::from(rgba(0xdc26261f))),
            callout_caution_border: raw
                .callout_caution_border
                .unwrap_or_else(|| Hsla::from(rgba(0xf87171ff))),
            footnote_bg: raw
                .footnote_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x292929ff))),
            footnote_border: raw
                .footnote_border
                .unwrap_or_else(|| Hsla::from(rgba(0x48464452))),
            footnote_badge_bg: raw
                .footnote_badge_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x3b3a3924))),
            footnote_badge_text: raw
                .footnote_badge_text
                .unwrap_or_else(|| Hsla::from(rgba(0xd6d6d6ff))),
            footnote_backref: raw
                .footnote_backref
                .unwrap_or_else(|| Hsla::from(rgba(0x75beffff))),
            task_checkbox_border: raw
                .task_checkbox_border
                .unwrap_or_else(|| Hsla::from(rgba(0x8a8886ff))),
            task_checkbox_bg: raw
                .task_checkbox_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x00000000))),
            task_checkbox_checked_bg: raw
                .task_checkbox_checked_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x4cc2ffff))),
            task_checkbox_check: raw
                .task_checkbox_check
                .unwrap_or_else(|| Hsla::from(rgba(0x1f1f1fff))),
            separator_color: raw
                .separator_color
                .unwrap_or_else(|| Hsla::from(rgba(0x5a5a5aff))),
            code_bg: raw.code_bg.unwrap_or_else(|| Hsla::from(rgba(0x252832ff))),
            code_text: raw.code_text,
            code_language_input_bg: raw
                .code_language_input_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x333333ff))),
            code_language_input_border: raw
                .code_language_input_border
                .unwrap_or_else(|| Hsla::from(rgba(0x484644ff))),
            code_language_input_text: raw
                .code_language_input_text
                .unwrap_or_else(|| Hsla::from(rgba(0xf5f5f5ff))),
            code_language_input_placeholder: raw
                .code_language_input_placeholder
                .unwrap_or_else(|| Hsla::from(rgba(0x9c9c9cff))),
            code_syntax_comment: raw
                .code_syntax_comment
                .unwrap_or_else(|| Hsla::from(rgba(0x858585ff))),
            code_syntax_keyword: raw
                .code_syntax_keyword
                .unwrap_or_else(|| Hsla::from(rgba(0xc586c0ff))),
            code_syntax_string: raw
                .code_syntax_string
                .unwrap_or_else(|| Hsla::from(rgba(0xce9178ff))),
            code_syntax_number: raw
                .code_syntax_number
                .unwrap_or_else(|| Hsla::from(rgba(0xb5cea8ff))),
            code_syntax_type: raw
                .code_syntax_type
                .unwrap_or_else(|| Hsla::from(rgba(0x4ec9b0ff))),
            code_syntax_function: raw
                .code_syntax_function
                .unwrap_or_else(|| Hsla::from(rgba(0xdcdcaaFF))),
            code_syntax_constant: raw
                .code_syntax_constant
                .unwrap_or_else(|| Hsla::from(rgba(0x4fc1ffff))),
            code_syntax_variable: raw
                .code_syntax_variable
                .unwrap_or_else(|| Hsla::from(rgba(0x9cdcfeff))),
            code_syntax_property: raw
                .code_syntax_property
                .unwrap_or_else(|| Hsla::from(rgba(0x9cdcfeff))),
            code_syntax_operator: raw
                .code_syntax_operator
                .unwrap_or_else(|| Hsla::from(rgba(0xd4d4d4ff))),
            code_syntax_punctuation: raw
                .code_syntax_punctuation
                .unwrap_or_else(|| Hsla::from(rgba(0xd4d4d4ff))),
            table_border: raw
                .table_border
                .unwrap_or_else(|| Hsla::from(rgba(0x484644ff))),
            table_header_bg: raw
                .table_header_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x333333ff))),
            current_line_bg: raw
                .current_line_bg
                .unwrap_or_else(|| Hsla::from(rgba(0xffffff0d))),
            matching_bracket_bg: raw
                .matching_bracket_bg
                .unwrap_or_else(|| Hsla::from(rgba(0xffffff1a))),
            table_cell_bg: raw
                .table_cell_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x292929ff))),
            table_cell_active_outline: raw
                .table_cell_active_outline
                .unwrap_or_else(|| Hsla::from(rgba(0x4cc2ffff))),
            table_axis_preview_bg: raw
                .table_axis_preview_bg
                .unwrap_or_else(|| Hsla::from(rgba(0xf5f5f51a))),
            table_axis_selected_bg: raw
                .table_axis_selected_bg
                .unwrap_or_else(|| Hsla::from(rgba(0xf5f5f533))),
            table_append_button_bg: raw
                .table_append_button_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x333333ff))),
            table_append_button_hover: raw
                .table_append_button_hover
                .unwrap_or_else(|| Hsla::from(rgba(0x484644ff))),
            table_append_button_text: raw
                .table_append_button_text
                .unwrap_or_else(|| Hsla::from(rgba(0xf5f5f5ff))),
            image_placeholder_bg: raw
                .image_placeholder_bg
                .unwrap_or_else(|| Hsla::from(rgba(0x292929ff))),
            image_placeholder_border: raw
                .image_placeholder_border
                .unwrap_or_else(|| Hsla::from(rgba(0x484644ff))),
            image_placeholder_text: raw
                .image_placeholder_text
                .unwrap_or_else(|| Hsla::from(rgba(0xd6d6d6ff))),
            image_caption_text: raw
                .image_caption_text
                .unwrap_or_else(|| Hsla::from(rgba(0xa19f9dff))),
            scrollbar_thumb: raw.scrollbar_thumb,
            cursor: raw.cursor,
            selection: raw.selection,
            dialog_backdrop: raw.dialog_backdrop,
            dialog_surface: raw.dialog_surface,
            dialog_border: raw.dialog_border,
            dialog_title: raw.dialog_title,
            dialog_body: raw.dialog_body,
            dialog_muted: raw.dialog_muted,
            dialog_primary_button_bg: raw.dialog_primary_button_bg,
            dialog_primary_button_hover: raw.dialog_primary_button_hover,
            dialog_primary_button_text: raw.dialog_primary_button_text,
            dialog_secondary_button_bg: raw.dialog_secondary_button_bg,
            dialog_secondary_button_hover: raw.dialog_secondary_button_hover,
            dialog_secondary_button_text: raw.dialog_secondary_button_text,
            dialog_danger_button_bg: raw.dialog_danger_button_bg,
            dialog_danger_button_hover: raw.dialog_danger_button_hover,
            dialog_danger_button_text: raw.dialog_danger_button_text,
            status_bar_background: raw
                .status_bar_background
                .unwrap_or_else(|| Hsla::from(rgba(0x292929ff))),
            status_bar_text: raw
                .status_bar_text
                .unwrap_or_else(|| Hsla::from(rgba(0xd6d6d6ff))),
            status_bar_text_dim: raw
                .status_bar_text_dim
                .unwrap_or_else(|| Hsla::from(rgba(0xa19f9dff))),
            status_bar_button_hover: raw
                .status_bar_button_hover
                .unwrap_or_else(|| Hsla::from(rgba(0x484644ff))),
        })
    }
}

/// Deserialization adapter for `ThemeDimensions` with backward-compatible defaults.
#[derive(Deserialize)]
struct ThemeDimensionsDe {
    editor_padding: f32,
    writing_max_width: Option<f32>,
    block_gap: f32,
    block_min_height: f32,
    block_padding_y: f32,
    block_padding_x: f32,
    nested_block_indent: f32,
    list_marker_gap: f32,
    list_marker_width: f32,
    ordered_list_marker_width: f32,
    task_checkbox_size: Option<f32>,
    task_checkbox_radius: Option<f32>,
    task_checkbox_border_width: Option<f32>,
    task_checkbox_check_size: Option<f32>,
    h1_padding_bottom: f32,
    h1_margin_bottom: f32,
    cursor_width: f32,
    underline_thickness: f32,
    h1_border_width: f32,
    quote_border_width: f32,
    quote_padding_left: f32,
    callout_padding_x: Option<f32>,
    callout_padding_y: Option<f32>,
    callout_body_gap: Option<f32>,
    callout_radius: Option<f32>,
    callout_border_width: Option<f32>,
    callout_header_gap: Option<f32>,
    callout_header_margin_bottom: Option<f32>,
    footnote_padding_x: Option<f32>,
    footnote_padding_y: Option<f32>,
    footnote_radius: Option<f32>,
    footnote_badge_padding_x: Option<f32>,
    footnote_badge_padding_y: Option<f32>,
    separator_thickness: Option<f32>,
    separator_inset_x: Option<f32>,
    separator_margin_y: Option<f32>,
    code_block_padding_y: f32,
    code_block_padding_x: f32,
    code_bg_pad_x: f32,
    code_bg_pad_y: f32,
    code_bg_radius: f32,
    code_language_input_width: Option<f32>,
    code_language_input_height: Option<f32>,
    code_language_input_padding_x: Option<f32>,
    code_language_input_padding_y: Option<f32>,
    code_language_input_radius: Option<f32>,
    code_language_input_border_width: Option<f32>,
    code_language_input_gap: Option<f32>,
    table_cell_padding_x: Option<f32>,
    table_cell_padding_y: Option<f32>,
    table_cell_min_height: Option<f32>,
    table_append_button_extent: Option<f32>,
    table_append_button_inset: Option<f32>,
    table_append_activation_band: Option<f32>,
    image_radius: Option<f32>,
    image_root_max_height: Option<f32>,
    image_root_max_width: Option<f32>,
    image_cell_max_height: Option<f32>,
    image_root_placeholder_height: Option<f32>,
    image_cell_placeholder_height: Option<f32>,
    image_caption_gap: Option<f32>,
    scrollbar_width: f32,
    scrollbar_right: f32,
    centered_shrink_start: f32,
    centered_shrink_end: f32,
    centered_min_ratio: f32,
    dialog_width: f32,
    dialog_padding: f32,
    dialog_gap: f32,
    dialog_radius: f32,
    dialog_border_width: f32,
    dialog_button_height: f32,
    dialog_button_gap: f32,
    dialog_button_padding_x: f32,
    menu_bar_height: Option<f32>,
    menu_bar_padding_x: Option<f32>,
    menu_bar_padding_y: Option<f32>,
    menu_bar_gap: Option<f32>,
    menu_bar_button_width: Option<f32>,
    menu_bar_button_height: Option<f32>,
    menu_bar_button_padding_x: Option<f32>,
    menu_bar_button_radius: Option<f32>,
    menu_text_size: Option<f32>,
    menu_panel_top: Option<f32>,
    menu_panel_width: Option<f32>,
    menu_panel_padding: Option<f32>,
    menu_panel_gap: Option<f32>,
    menu_panel_radius: Option<f32>,
    menu_item_height: Option<f32>,
    menu_item_padding_x: Option<f32>,
    menu_item_radius: Option<f32>,
    menu_separator_margin_x: Option<f32>,
    menu_separator_margin_y: Option<f32>,
    menu_separator_height: Option<f32>,
    context_menu_panel_width: Option<f32>,
    context_menu_submenu_width: Option<f32>,
    context_menu_submenu_gap: Option<f32>,
    context_menu_axis_panel_width: Option<f32>,
    table_insert_dialog_width: Option<f32>,
    table_insert_stepper_gap: Option<f32>,
    table_insert_stepper_button_size: Option<f32>,
    table_insert_stepper_value_min_width: Option<f32>,
    table_insert_stepper_value_padding_x: Option<f32>,
    table_insert_stepper_radius: Option<f32>,
    view_mode_toggle_left: Option<f32>,
    view_mode_toggle_bottom: Option<f32>,
    view_mode_toggle_padding_x: Option<f32>,
    view_mode_toggle_padding_y: Option<f32>,
    view_mode_toggle_min_width: Option<f32>,
    view_mode_toggle_radius: Option<f32>,
    view_mode_toggle_border_width: Option<f32>,
    view_mode_toggle_text_size: Option<f32>,
    status_bar_height: Option<f32>,
    status_bar_padding_x: Option<f32>,
    status_bar_item_gap: Option<f32>,
    status_bar_text_size: Option<f32>,
}

impl<'de> Deserialize<'de> for ThemeDimensions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = ThemeDimensionsDe::deserialize(deserializer)?;
        Ok(Self {
            editor_padding: raw.editor_padding,
            writing_max_width: raw.writing_max_width.unwrap_or(760.0),
            block_gap: raw.block_gap,
            block_min_height: raw.block_min_height,
            block_padding_y: raw.block_padding_y,
            block_padding_x: raw.block_padding_x,
            nested_block_indent: raw.nested_block_indent,
            list_marker_gap: raw.list_marker_gap,
            list_marker_width: raw.list_marker_width,
            ordered_list_marker_width: raw.ordered_list_marker_width,
            task_checkbox_size: raw.task_checkbox_size.unwrap_or(14.0),
            task_checkbox_radius: raw.task_checkbox_radius.unwrap_or(4.0),
            task_checkbox_border_width: raw.task_checkbox_border_width.unwrap_or(1.0),
            task_checkbox_check_size: raw.task_checkbox_check_size.unwrap_or(10.0),
            h1_padding_bottom: raw.h1_padding_bottom,
            h1_margin_bottom: raw.h1_margin_bottom,
            cursor_width: raw.cursor_width,
            underline_thickness: raw.underline_thickness,
            h1_border_width: raw.h1_border_width,
            quote_border_width: raw.quote_border_width,
            quote_padding_left: raw.quote_padding_left,
            callout_padding_x: raw.callout_padding_x.unwrap_or(14.0),
            callout_padding_y: raw.callout_padding_y.unwrap_or(10.0),
            callout_body_gap: raw.callout_body_gap.unwrap_or(8.0),
            callout_radius: raw.callout_radius.unwrap_or(10.0),
            callout_border_width: raw.callout_border_width.unwrap_or(4.0),
            callout_header_gap: raw.callout_header_gap.unwrap_or(6.0),
            callout_header_margin_bottom: raw.callout_header_margin_bottom.unwrap_or(6.0),
            footnote_padding_x: raw.footnote_padding_x.unwrap_or(10.0),
            footnote_padding_y: raw.footnote_padding_y.unwrap_or(6.0),
            footnote_radius: raw.footnote_radius.unwrap_or(6.0),
            footnote_badge_padding_x: raw.footnote_badge_padding_x.unwrap_or(4.0),
            footnote_badge_padding_y: raw.footnote_badge_padding_y.unwrap_or(1.0),
            separator_thickness: raw.separator_thickness.unwrap_or(1.0),
            separator_inset_x: raw.separator_inset_x.unwrap_or(40.0),
            separator_margin_y: raw.separator_margin_y.unwrap_or(10.0),
            code_block_padding_y: raw.code_block_padding_y,
            code_block_padding_x: raw.code_block_padding_x,
            code_bg_pad_x: raw.code_bg_pad_x,
            code_bg_pad_y: raw.code_bg_pad_y,
            code_bg_radius: raw.code_bg_radius,
            code_language_input_width: raw.code_language_input_width.unwrap_or(156.0),
            code_language_input_height: raw.code_language_input_height.unwrap_or(18.0),
            code_language_input_padding_x: raw.code_language_input_padding_x.unwrap_or(8.0),
            code_language_input_padding_y: raw.code_language_input_padding_y.unwrap_or(3.0),
            code_language_input_radius: raw.code_language_input_radius.unwrap_or(6.0),
            code_language_input_border_width: raw.code_language_input_border_width.unwrap_or(1.0),
            code_language_input_gap: raw.code_language_input_gap.unwrap_or(8.0),
            table_cell_padding_x: raw.table_cell_padding_x.unwrap_or(10.0),
            table_cell_padding_y: raw.table_cell_padding_y.unwrap_or(8.0),
            table_cell_min_height: raw.table_cell_min_height.unwrap_or(42.0),
            table_append_button_extent: raw.table_append_button_extent.unwrap_or(16.0),
            table_append_button_inset: raw.table_append_button_inset.unwrap_or(8.0),
            table_append_activation_band: raw.table_append_activation_band.unwrap_or(18.0),
            image_radius: raw.image_radius.unwrap_or(12.0),
            image_root_max_height: raw.image_root_max_height.unwrap_or(420.0),
            image_root_max_width: raw.image_root_max_width.unwrap_or(480.0),
            image_cell_max_height: raw.image_cell_max_height.unwrap_or(180.0),
            image_root_placeholder_height: raw.image_root_placeholder_height.unwrap_or(260.0),
            image_cell_placeholder_height: raw.image_cell_placeholder_height.unwrap_or(120.0),
            image_caption_gap: raw.image_caption_gap.unwrap_or(8.0),
            scrollbar_width: raw.scrollbar_width,
            scrollbar_right: raw.scrollbar_right,
            centered_shrink_start: raw.centered_shrink_start,
            centered_shrink_end: raw.centered_shrink_end,
            centered_min_ratio: raw.centered_min_ratio,
            dialog_width: raw.dialog_width,
            dialog_padding: raw.dialog_padding,
            dialog_gap: raw.dialog_gap,
            dialog_radius: raw.dialog_radius,
            dialog_border_width: raw.dialog_border_width,
            dialog_button_height: raw.dialog_button_height,
            dialog_button_gap: raw.dialog_button_gap,
            dialog_button_padding_x: raw.dialog_button_padding_x,
            menu_bar_height: raw.menu_bar_height.unwrap_or(32.0),
            menu_bar_padding_x: raw.menu_bar_padding_x.unwrap_or(10.0),
            menu_bar_padding_y: raw.menu_bar_padding_y.unwrap_or(4.0),
            menu_bar_gap: raw.menu_bar_gap.unwrap_or(2.0),
            menu_bar_button_width: raw.menu_bar_button_width.unwrap_or(48.0),
            menu_bar_button_height: raw.menu_bar_button_height.unwrap_or(24.0),
            menu_bar_button_padding_x: raw.menu_bar_button_padding_x.unwrap_or(8.0),
            menu_bar_button_radius: raw.menu_bar_button_radius.unwrap_or(5.0),
            menu_text_size: raw.menu_text_size.unwrap_or(12.0),
            menu_panel_top: raw.menu_panel_top.unwrap_or(30.0),
            menu_panel_width: raw.menu_panel_width.unwrap_or(180.0),
            menu_panel_padding: raw.menu_panel_padding.unwrap_or(4.0),
            menu_panel_gap: raw.menu_panel_gap.unwrap_or(1.0),
            menu_panel_radius: raw.menu_panel_radius.unwrap_or(8.0),
            menu_item_height: raw.menu_item_height.unwrap_or(28.0),
            menu_item_padding_x: raw.menu_item_padding_x.unwrap_or(8.0),
            menu_item_radius: raw.menu_item_radius.unwrap_or(5.0),
            menu_separator_margin_x: raw.menu_separator_margin_x.unwrap_or(6.0),
            menu_separator_margin_y: raw.menu_separator_margin_y.unwrap_or(3.0),
            menu_separator_height: raw.menu_separator_height.unwrap_or(1.0),
            context_menu_panel_width: raw.context_menu_panel_width.unwrap_or(132.0),
            context_menu_submenu_width: raw.context_menu_submenu_width.unwrap_or(148.0),
            context_menu_submenu_gap: raw.context_menu_submenu_gap.unwrap_or(2.0),
            context_menu_axis_panel_width: raw.context_menu_axis_panel_width.unwrap_or(164.0),
            table_insert_dialog_width: raw.table_insert_dialog_width.unwrap_or(380.0),
            table_insert_stepper_gap: raw.table_insert_stepper_gap.unwrap_or(8.0),
            table_insert_stepper_button_size: raw.table_insert_stepper_button_size.unwrap_or(32.0),
            table_insert_stepper_value_min_width: raw
                .table_insert_stepper_value_min_width
                .unwrap_or(56.0),
            table_insert_stepper_value_padding_x: raw
                .table_insert_stepper_value_padding_x
                .unwrap_or(10.0),
            table_insert_stepper_radius: raw.table_insert_stepper_radius.unwrap_or(8.0),
            view_mode_toggle_left: raw.view_mode_toggle_left.unwrap_or(12.0),
            view_mode_toggle_bottom: raw.view_mode_toggle_bottom.unwrap_or(12.0),
            view_mode_toggle_padding_x: raw.view_mode_toggle_padding_x.unwrap_or(8.0),
            view_mode_toggle_padding_y: raw.view_mode_toggle_padding_y.unwrap_or(4.0),
            view_mode_toggle_min_width: raw.view_mode_toggle_min_width.unwrap_or(88.0),
            view_mode_toggle_radius: raw.view_mode_toggle_radius.unwrap_or(999.0),
            view_mode_toggle_border_width: raw.view_mode_toggle_border_width.unwrap_or(1.0),
            view_mode_toggle_text_size: raw.view_mode_toggle_text_size.unwrap_or(11.0),
            status_bar_height: raw.status_bar_height.unwrap_or(28.0),
            status_bar_padding_x: raw.status_bar_padding_x.unwrap_or(12.0),
            status_bar_item_gap: raw.status_bar_item_gap.unwrap_or(12.0),
            status_bar_text_size: raw.status_bar_text_size.unwrap_or(11.0),
        })
    }
}
