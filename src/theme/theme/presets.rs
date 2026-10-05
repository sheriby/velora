use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub colors: ThemeColors,
    pub dimensions: ThemeDimensions,
    pub typography: ThemeTypography,
    pub placeholders: Placeholders,
}

#[derive(Clone, Copy)]
struct BuiltinPalette {
    window: u32,
    panel: u32,
    panel_hover: u32,
    text: u32,
    muted: u32,
    line: u32,
    accent: u32,
    selection: u32,
    code: u32,
    dark: bool,
}

fn recolor_builtin(mut theme: Theme, name: &str, palette: BuiltinPalette) -> Theme {
    let color = |value| Hsla::from(rgba(value));
    let c = &mut theme.colors;
    theme.name = name.into();
    c.editor_background = color(palette.window);
    c.source_mode_block_bg = color(palette.code);
    c.comment_bg = color(palette.code);
    c.text_default = color(palette.text);
    c.text_placeholder = color(palette.muted);
    c.text_link = color(palette.accent);
    // 当前行高亮跟主题走：取各色板的选区淡色（forest 即淡绿系）垫在光标行下，
    // alpha 压到 40%——高亮一旦接近不透明就会盖住正文（用户报修）。
    c.current_line_bg = color((palette.selection & 0xffffff00) | 0x66);
    // 活动搜索命中：固定橙色、更实，与普通命中的浅黄拉开层次（用户报修：
    // 原来绑主题选区色，forest 下是淡绿，跟正文贴在一起根本看不出来）。
    c.search_active_highlight_bg = color(0xff9500b4);
    c.text_h1 = color(palette.text);
    c.text_h2 = color(palette.text);
    c.text_h3 = color(palette.text);
    c.text_h4 = color(palette.text);
    c.text_h5 = color(palette.text);
    c.text_h6 = color(palette.text);
    c.text_quote = color(palette.muted);
    c.border_h1 = color(palette.line);
    c.border_h2 = color(palette.line);
    c.border_quote = color(palette.accent);
    c.callout_note_bg = color(palette.panel);
    c.callout_note_border = color(palette.accent);
    c.callout_tip_bg = color(palette.panel);
    c.callout_tip_border = color(palette.accent);
    c.footnote_bg = color(palette.panel);
    c.footnote_border = color(palette.line);
    c.footnote_badge_bg = color(palette.code);
    c.footnote_badge_text = color(palette.muted);
    c.footnote_backref = color(palette.accent);
    c.task_checkbox_checked_bg = color(palette.accent);
    c.task_checkbox_border = color(palette.muted);
    c.task_checkbox_bg = color(palette.window);
    c.task_checkbox_check = color(if palette.dark { 0x171b22ff } else { 0xffffffff });
    c.separator_color = color(palette.line);
    c.code_bg = color(palette.code);
    c.code_text = color(palette.text);
    c.code_language_input_bg = color(palette.panel);
    c.code_language_input_border = color(palette.line);
    c.code_language_input_text = color(palette.text);
    c.code_language_input_placeholder = color(palette.muted);
    c.table_border = color(palette.line);
    c.table_header_bg = color(palette.panel);
    c.matching_bracket_bg = color((palette.text & 0xffffff00) | 0x1a);
    c.table_cell_bg = color(palette.window);
    c.table_cell_active_outline = color(palette.accent);
    c.table_axis_preview_bg = color((palette.accent & 0xffffff00) | 0x0f);
    c.table_axis_selected_bg = color((palette.accent & 0xffffff00) | 0x19);
    c.table_append_button_bg = color(palette.panel);
    c.table_append_button_hover = color(palette.panel_hover);
    c.table_append_button_text = color(palette.text);
    c.image_placeholder_bg = color(palette.code);
    c.image_placeholder_border = color(palette.line);
    c.image_placeholder_text = color(palette.muted);
    c.image_caption_text = color(palette.muted);
    c.cursor = color(palette.text);
    c.selection = color(palette.selection);
    c.scrollbar_thumb = color((palette.muted & 0xffffff00) | 0xb8);
    c.dialog_surface = color(palette.window);
    c.dialog_border = color(palette.line);
    c.dialog_title = color(palette.text);
    c.dialog_body = color(palette.text);
    c.dialog_muted = color(palette.muted);
    c.dialog_primary_button_bg = color(palette.accent);
    c.dialog_primary_button_hover = color(palette.accent);
    c.dialog_primary_button_text = color(if palette.dark { 0x171b22ff } else { 0xffffffff });
    c.dialog_secondary_button_bg = color(palette.panel);
    c.dialog_secondary_button_hover = color(palette.panel_hover);
    c.dialog_secondary_button_text = color(palette.text);
    c.status_bar_background = color(palette.panel);
    c.status_bar_text = color(palette.text);
    c.status_bar_text_dim = color(palette.muted);
    c.status_bar_button_hover = color(palette.panel_hover);
    theme
}

#[allow(unused)]
impl Theme {
    /// Returns the built-in fallback theme used when no custom theme is loaded.
    pub fn default_theme() -> Self {
        Self {
            name: BUILTIN_THEME_DARK_NAME.into(),
            colors: ThemeColors {
                editor_background: Hsla::from(rgba(0x1b1d24ff)),
                source_mode_block_bg: Hsla::from(rgba(0x292929ff)),
                comment_bg: Hsla::from(rgba(0xfce10026)),
                text_default: Hsla::from(rgba(0xe8e9eeff)),
                text_link: Hsla::from(rgba(0x75beffff)),
                text_placeholder: Hsla::from(rgba(0x9c9c9cff)),
                text_h1: Hsla::from(rgba(0xf5f5f5ff)),
                text_h2: Hsla::from(rgba(0xf5f5f5ff)),
                text_h3: Hsla::from(rgba(0xf5f5f5ff)),
                text_h4: Hsla::from(rgba(0xf5f5f5ff)),
                text_h5: Hsla::from(rgba(0xf5f5f5ff)),
                text_h6: Hsla::from(rgba(0xf5f5f5ff)),
                border_h1: Hsla::from(rgba(0x484644ff)),
                border_h2: Hsla::from(rgba(0x3b3a39ff)),
                text_quote: Hsla::from(rgba(0xd6d6d6ff)),
                border_quote: Hsla::from(rgba(0xaaa0ffff)),
                callout_note_bg: Hsla::from(rgba(0x94a3b81f)),
                callout_note_border: Hsla::from(rgba(0x94a3b4ff)),
                callout_tip_bg: Hsla::from(rgba(0x4cc2ff1f)),
                callout_tip_border: Hsla::from(rgba(0x4cc2ffff)),
                callout_important_bg: Hsla::from(rgba(0xa78bfa1f)),
                callout_important_border: Hsla::from(rgba(0xa78bfaff)),
                callout_warning_bg: Hsla::from(rgba(0xfce1001f)),
                search_highlight_bg: Hsla::from(rgba(0xffe06638)),
                search_active_highlight_bg: Hsla::from(rgba(0xff9500b4)),
                callout_warning_border: Hsla::from(rgba(0xfce100ff)),
                callout_caution_bg: Hsla::from(rgba(0xd134381f)),
                callout_caution_border: Hsla::from(rgba(0xd13438ff)),
                footnote_bg: Hsla::from(rgba(0x292929ff)),
                footnote_border: Hsla::from(rgba(0x484644ff)),
                footnote_badge_bg: Hsla::from(rgba(0x3b3a39ff)),
                footnote_badge_text: Hsla::from(rgba(0xd6d6d6ff)),
                footnote_backref: Hsla::from(rgba(0x75beffff)),
                task_checkbox_border: Hsla::from(rgba(0x8a8886ff)),
                task_checkbox_bg: Hsla::from(rgba(0x00000000)),
                task_checkbox_checked_bg: Hsla::from(rgba(0x4cc2ffff)),
                task_checkbox_check: Hsla::from(rgba(0x1f1f1fff)),
                separator_color: Hsla::from(rgba(0x5a5a5aff)),
                code_bg: Hsla::from(rgba(0x252832ff)),
                code_text: Hsla::from(rgba(0xd6d6d6ff)),
                code_language_input_bg: Hsla::from(rgba(0x333333ff)),
                code_language_input_border: Hsla::from(rgba(0x484644ff)),
                code_language_input_text: Hsla::from(rgba(0xf5f5f5ff)),
                code_language_input_placeholder: Hsla::from(rgba(0x9c9c9cff)),
                code_syntax_comment: Hsla::from(rgba(0x858585ff)),
                code_syntax_keyword: Hsla::from(rgba(0xc586c0ff)),
                code_syntax_string: Hsla::from(rgba(0xce9178ff)),
                code_syntax_number: Hsla::from(rgba(0xb5cea8ff)),
                code_syntax_type: Hsla::from(rgba(0x4ec9b0ff)),
                code_syntax_function: Hsla::from(rgba(0xdcdcaaFF)),
                code_syntax_constant: Hsla::from(rgba(0x4fc1ffff)),
                code_syntax_variable: Hsla::from(rgba(0x9cdcfeff)),
                code_syntax_property: Hsla::from(rgba(0x9cdcfeff)),
                code_syntax_operator: Hsla::from(rgba(0xd4d4d4ff)),
                code_syntax_punctuation: Hsla::from(rgba(0xd4d4d4ff)),
                table_border: Hsla::from(rgba(0x484644ff)),
                table_header_bg: Hsla::from(rgba(0x333333ff)),
                current_line_bg: Hsla::from(rgba(0xffffff0d)),
                matching_bracket_bg: Hsla::from(rgba(0xffffff1a)),
                table_cell_bg: Hsla::from(rgba(0x292929ff)),
                table_cell_active_outline: Hsla::from(rgba(0x4cc2ffff)),
                table_axis_preview_bg: Hsla::from(rgba(0xf5f5f51a)),
                table_axis_selected_bg: Hsla::from(rgba(0xf5f5f533)),
                table_append_button_bg: Hsla::from(rgba(0x333333ff)),
                table_append_button_hover: Hsla::from(rgba(0x484644ff)),
                table_append_button_text: Hsla::from(rgba(0xf5f5f5ff)),
                image_placeholder_bg: Hsla::from(rgba(0x292929ff)),
                image_placeholder_border: Hsla::from(rgba(0x484644ff)),
                image_placeholder_text: Hsla::from(rgba(0xd6d6d6ff)),
                image_caption_text: Hsla::from(rgba(0xa19f9dff)),
                scrollbar_thumb: Hsla::from(rgba(0xa19f9dcc)),
                cursor: Hsla::from(rgba(0xf5f5f5ff)),
                selection: Hsla::from(rgba(0x403a61ff)),
                dialog_backdrop: Hsla::from(rgba(0x00000088)),
                dialog_surface: Hsla::from(rgba(0x1b1d24ff)),
                dialog_border: Hsla::from(rgba(0x333640ff)),
                dialog_title: Hsla::from(rgba(0xf5f5f5ff)),
                dialog_body: Hsla::from(rgba(0xd6d6d6ff)),
                dialog_muted: Hsla::from(rgba(0xa0a4afff)),
                dialog_primary_button_bg: Hsla::from(rgba(0xaaa0ffff)),
                dialog_primary_button_hover: Hsla::from(rgba(0xb9afffff)),
                dialog_primary_button_text: Hsla::from(rgba(0x1f1f1fff)),
                dialog_secondary_button_bg: Hsla::from(rgba(0x20232bff)),
                dialog_secondary_button_hover: Hsla::from(rgba(0x292c35ff)),
                dialog_secondary_button_text: Hsla::from(rgba(0xf5f5f5ff)),
                dialog_danger_button_bg: Hsla::from(rgba(0xd13438ff)),
                dialog_danger_button_hover: Hsla::from(rgba(0xa4262cff)),
                dialog_danger_button_text: Hsla::from(rgba(0xffffffff)),
                status_bar_background: Hsla::from(rgba(0x20232bff)),
                status_bar_text: Hsla::from(rgba(0xd6d6d6ff)),
                status_bar_text_dim: Hsla::from(rgba(0xa19f9dff)),
                status_bar_button_hover: Hsla::from(rgba(0x484644ff)),
            },
            dimensions: ThemeDimensions {
                editor_padding: 24.0,
                writing_max_width: 760.0,
                block_gap: 6.0,
                block_min_height: 28.0,
                block_padding_y: 4.0,
                block_padding_x: 12.0,
                nested_block_indent: 20.0,
                list_marker_gap: 8.0,
                list_marker_width: 12.0,
                ordered_list_marker_width: 20.0,
                task_checkbox_size: 14.0,
                task_checkbox_radius: 4.0,
                task_checkbox_border_width: 1.0,
                task_checkbox_check_size: 10.0,
                h1_padding_bottom: 4.0,
                h1_margin_bottom: 4.0,
                cursor_width: 2.0,
                underline_thickness: 1.0,
                h1_border_width: 1.0,
                quote_border_width: 3.0,
                quote_padding_left: 12.0,
                callout_padding_x: 14.0,
                callout_padding_y: 10.0,
                callout_body_gap: 8.0,
                callout_radius: 10.0,
                callout_border_width: 4.0,
                callout_header_gap: 6.0,
                callout_header_margin_bottom: 6.0,
                footnote_padding_x: 10.0,
                footnote_padding_y: 6.0,
                footnote_radius: 6.0,
                footnote_badge_padding_x: 4.0,
                footnote_badge_padding_y: 1.0,
                separator_thickness: 1.0,
                separator_inset_x: 40.0,
                separator_margin_y: 10.0,
                code_block_padding_y: 8.0,
                code_block_padding_x: 12.0,
                code_bg_pad_x: 3.0,
                code_bg_pad_y: 1.0,
                code_bg_radius: 4.0,
                code_language_input_width: 156.0,
                code_language_input_height: 18.0,
                code_language_input_padding_x: 8.0,
                code_language_input_padding_y: 3.0,
                code_language_input_radius: 6.0,
                code_language_input_border_width: 1.0,
                code_language_input_gap: 8.0,
                table_cell_padding_x: 10.0,
                table_cell_padding_y: 8.0,
                table_cell_min_height: 42.0,
                table_append_button_extent: 16.0,
                table_append_button_inset: 8.0,
                table_append_activation_band: 18.0,
                image_radius: 12.0,
                image_root_max_height: 420.0,
                image_root_max_width: 480.0,
                image_cell_max_height: 180.0,
                image_root_placeholder_height: 260.0,
                image_cell_placeholder_height: 120.0,
                image_caption_gap: 8.0,
                scrollbar_width: 6.0,
                scrollbar_right: 6.0,
                centered_shrink_start: 1100.0,
                centered_shrink_end: 2200.0,
                centered_min_ratio: 0.58,
                dialog_width: 460.0,
                dialog_padding: 20.0,
                dialog_gap: 14.0,
                dialog_radius: 14.0,
                dialog_border_width: 1.0,
                dialog_button_height: 36.0,
                dialog_button_gap: 10.0,
                dialog_button_padding_x: 14.0,
                menu_bar_height: 32.0,
                menu_bar_padding_x: 10.0,
                menu_bar_padding_y: 4.0,
                menu_bar_gap: 2.0,
                menu_bar_button_width: 48.0,
                menu_bar_button_height: 24.0,
                menu_bar_button_padding_x: 8.0,
                menu_bar_button_radius: 5.0,
                menu_text_size: 12.0,
                menu_panel_top: 30.0,
                menu_panel_width: 180.0,
                menu_panel_padding: 4.0,
                menu_panel_gap: 1.0,
                menu_panel_radius: 8.0,
                menu_item_height: 28.0,
                menu_item_padding_x: 8.0,
                menu_item_radius: 5.0,
                menu_separator_margin_x: 6.0,
                menu_separator_margin_y: 3.0,
                menu_separator_height: 1.0,
                context_menu_panel_width: 132.0,
                context_menu_submenu_width: 148.0,
                context_menu_submenu_gap: 2.0,
                context_menu_axis_panel_width: 164.0,
                table_insert_dialog_width: 380.0,
                table_insert_stepper_gap: 8.0,
                table_insert_stepper_button_size: 32.0,
                table_insert_stepper_value_min_width: 56.0,
                table_insert_stepper_value_padding_x: 10.0,
                table_insert_stepper_radius: 8.0,
                view_mode_toggle_left: 12.0,
                view_mode_toggle_bottom: 12.0,
                view_mode_toggle_padding_x: 8.0,
                view_mode_toggle_padding_y: 4.0,
                view_mode_toggle_min_width: 88.0,
                view_mode_toggle_radius: 999.0,
                view_mode_toggle_border_width: 1.0,
                view_mode_toggle_text_size: 11.0,
                status_bar_height: 28.0,
                status_bar_padding_x: 12.0,
                status_bar_item_gap: 12.0,
                status_bar_text_size: 11.0,
            },
            typography: ThemeTypography {
                body_font_family: default_body_font_family(),
                heading_font_family: default_heading_font_family(),
                text_size: 17.0,
                text_line_height: 1.6,
                text_letter_spacing: 0.0125,
                h1_size: 32.0,
                h1_weight: FontWeightDef::Bold,
                h2_size: 24.0,
                h2_weight: FontWeightDef::Bold,
                h3_size: 20.0,
                h3_weight: FontWeightDef::Semibold,
                h4_size: 18.0,
                h4_weight: FontWeightDef::Semibold,
                h5_size: 16.0,
                h5_weight: FontWeightDef::Semibold,
                h6_size: 14.0,
                h6_weight: FontWeightDef::Semibold,
                code_size: 15.0,
                dialog_title_size: 20.0,
                dialog_title_weight: FontWeightDef::Semibold,
                dialog_body_size: 14.0,
                dialog_body_weight: FontWeightDef::Normal,
                dialog_button_size: 14.0,
                dialog_button_weight: FontWeightDef::Medium,
            },
            placeholders: Placeholders {
                empty_editing: String::new(),
            },
        }
    }

    /// Returns the built-in light theme.
    ///
    /// The light theme intentionally reuses the default layout and typography
    /// tokens so it can focus on palette differences.
    pub fn light_theme() -> Self {
        let base = Self::default_theme();
        Self {
            name: BUILTIN_THEME_LIGHT_NAME.into(),
            colors: ThemeColors {
                editor_background: Hsla::from(rgba(0xffffffff)),
                source_mode_block_bg: Hsla::from(rgba(0xf3f2f1ff)),
                comment_bg: Hsla::from(rgba(0xfff4ce99)),
                text_default: Hsla::from(rgba(0x252832ff)),
                text_link: Hsla::from(rgba(0x6558d3ff)),
                text_placeholder: Hsla::from(rgba(0x8a8886cc)),
                text_h1: Hsla::from(rgba(0x201f1eff)),
                text_h2: Hsla::from(rgba(0x201f1eff)),
                text_h3: Hsla::from(rgba(0x201f1eff)),
                text_h4: Hsla::from(rgba(0x201f1eff)),
                text_h5: Hsla::from(rgba(0x201f1eff)),
                text_h6: Hsla::from(rgba(0x201f1eff)),
                border_h1: Hsla::from(rgba(0xd1d1d1ff)),
                border_h2: Hsla::from(rgba(0xedebe9ff)),
                text_quote: Hsla::from(rgba(0x484644ff)),
                border_quote: Hsla::from(rgba(0x6558d3ff)),
                callout_note_bg: Hsla::from(rgba(0x0078d414)),
                callout_note_border: Hsla::from(rgba(0x0078d4ff)),
                callout_tip_bg: Hsla::from(rgba(0x107c1014)),
                callout_tip_border: Hsla::from(rgba(0x107c10ff)),
                callout_important_bg: Hsla::from(rgba(0x8764b814)),
                callout_important_border: Hsla::from(rgba(0x8764b8ff)),
                callout_warning_bg: Hsla::from(rgba(0xca501014)),
                search_highlight_bg: Hsla::from(rgba(0xffd60a4d)),
                search_active_highlight_bg: Hsla::from(rgba(0xff9500b4)),
                callout_warning_border: Hsla::from(rgba(0xca5010ff)),
                callout_caution_bg: Hsla::from(rgba(0xd1343814)),
                callout_caution_border: Hsla::from(rgba(0xd13438ff)),
                footnote_bg: Hsla::from(rgba(0xffffffff)),
                footnote_border: Hsla::from(rgba(0xd1d1d1ff)),
                footnote_badge_bg: Hsla::from(rgba(0xf3f2f1ff)),
                footnote_badge_text: Hsla::from(rgba(0x484644ff)),
                footnote_backref: Hsla::from(rgba(0x0078d4ff)),
                task_checkbox_border: Hsla::from(rgba(0x8a8886ff)),
                task_checkbox_bg: Hsla::from(rgba(0xffffffff)),
                task_checkbox_checked_bg: Hsla::from(rgba(0x6558d3ff)),
                task_checkbox_check: Hsla::from(rgba(0xffffffff)),
                separator_color: Hsla::from(rgba(0xd1d1d1ff)),
                code_bg: Hsla::from(rgba(0xf2f3f6ff)),
                code_text: Hsla::from(rgba(0x242424ff)),
                code_language_input_bg: Hsla::from(rgba(0xffffffff)),
                code_language_input_border: Hsla::from(rgba(0xd1d1d1ff)),
                code_language_input_text: Hsla::from(rgba(0x242424ff)),
                code_language_input_placeholder: Hsla::from(rgba(0x8a8886cc)),
                code_syntax_comment: Hsla::from(rgba(0x6a6a6aff)),
                code_syntax_keyword: Hsla::from(rgba(0xaf00dbff)),
                code_syntax_string: Hsla::from(rgba(0x008000ff)),
                code_syntax_number: Hsla::from(rgba(0x098658ff)),
                code_syntax_type: Hsla::from(rgba(0x267f99ff)),
                code_syntax_function: Hsla::from(rgba(0x795e26ff)),
                code_syntax_constant: Hsla::from(rgba(0x0070c1ff)),
                code_syntax_variable: Hsla::from(rgba(0x001080ff)),
                code_syntax_property: Hsla::from(rgba(0x001080ff)),
                code_syntax_operator: Hsla::from(rgba(0x393a34ff)),
                code_syntax_punctuation: Hsla::from(rgba(0x393a34ff)),
                table_border: Hsla::from(rgba(0xd1d1d1ff)),
                table_header_bg: Hsla::from(rgba(0xf3f2f1ff)),
                current_line_bg: Hsla::from(rgba(0x0000000a)),
                matching_bracket_bg: Hsla::from(rgba(0x00000014)),
                table_cell_bg: Hsla::from(rgba(0xffffffff)),
                table_cell_active_outline: Hsla::from(rgba(0x6558d3ff)),
                table_axis_preview_bg: Hsla::from(rgba(0x0078d414)),
                table_axis_selected_bg: Hsla::from(rgba(0x0078d429)),
                table_append_button_bg: Hsla::from(rgba(0xf3f2f1ff)),
                table_append_button_hover: Hsla::from(rgba(0xedebe9ff)),
                table_append_button_text: Hsla::from(rgba(0x484644ff)),
                image_placeholder_bg: Hsla::from(rgba(0xf3f2f1ff)),
                image_placeholder_border: Hsla::from(rgba(0xd1d1d1ff)),
                image_placeholder_text: Hsla::from(rgba(0x484644ff)),
                image_caption_text: Hsla::from(rgba(0x605e5cff)),
                scrollbar_thumb: Hsla::from(rgba(0x8a8886b8)),
                cursor: Hsla::from(rgba(0x242424ff)),
                selection: Hsla::from(rgba(0xe8e5ffff)),
                dialog_backdrop: Hsla::from(rgba(0x00000066)),
                dialog_surface: Hsla::from(rgba(0xffffffff)),
                dialog_border: Hsla::from(rgba(0xe6e8edff)),
                dialog_title: Hsla::from(rgba(0x201f1eff)),
                dialog_body: Hsla::from(rgba(0x323130ff)),
                dialog_muted: Hsla::from(rgba(0x777d89ff)),
                dialog_primary_button_bg: Hsla::from(rgba(0x6558d3ff)),
                dialog_primary_button_hover: Hsla::from(rgba(0x574bc2ff)),
                dialog_primary_button_text: Hsla::from(rgba(0xffffffff)),
                dialog_secondary_button_bg: Hsla::from(rgba(0xf8f9fbff)),
                dialog_secondary_button_hover: Hsla::from(rgba(0xf1f3f6ff)),
                dialog_secondary_button_text: Hsla::from(rgba(0x242424ff)),
                dialog_danger_button_bg: Hsla::from(rgba(0xd13438ff)),
                dialog_danger_button_hover: Hsla::from(rgba(0xa4262cff)),
                dialog_danger_button_text: Hsla::from(rgba(0xffffffff)),
                status_bar_background: Hsla::from(rgba(0xf8f9fbff)),
                status_bar_text: Hsla::from(rgba(0x323130ff)),
                status_bar_text_dim: Hsla::from(rgba(0x605e5cff)),
                status_bar_button_hover: Hsla::from(rgba(0xedebe9ff)),
            },
            dimensions: base.dimensions,
            typography: base.typography,
            placeholders: base.placeholders,
        }
    }

    pub fn paper_theme() -> Self {
        let mut theme = recolor_builtin(
            Self::light_theme(),
            BUILTIN_THEME_PAPER_NAME,
            BuiltinPalette {
                window: 0xfffdf8ff,
                panel: 0xf7f2e9ff,
                panel_hover: 0xefe7dbff,
                text: 0x2c2924ff,
                muted: 0x756e65ff,
                line: 0xe4dacbff,
                accent: 0x9b603fff,
                selection: 0xf1e1d4ff,
                code: 0xf5efe6ff,
                dark: false,
            },
        );
        theme.typography.text_line_height = 1.72;
        let serif = if cfg!(target_os = "macos") {
            "Songti SC"
        } else {
            "Georgia"
        };
        theme.typography.body_font_family = serif.into();
        theme.typography.heading_font_family = serif.into();
        theme.typography.h1_size = 31.0;
        theme.typography.h1_weight = FontWeightDef::Semibold;
        theme.typography.h2_size = 23.0;
        theme.typography.h2_weight = FontWeightDef::Semibold;
        theme.dimensions.block_gap = 8.0;
        theme.dimensions.writing_max_width = 700.0;
        theme.dimensions.h1_border_width = 0.0;
        theme.dimensions.h1_margin_bottom = 10.0;
        theme.dimensions.table_cell_padding_y = 9.0;
        theme
    }

    pub fn forest_theme() -> Self {
        let mut theme = recolor_builtin(
            Self::light_theme(),
            BUILTIN_THEME_FOREST_NAME,
            BuiltinPalette {
                window: 0xfdfffcff,
                panel: 0xf5f8f3ff,
                panel_hover: 0xe8f0e3ff,
                text: 0x303831ff,
                muted: 0x657269ff,
                line: 0xdde7d8ff,
                accent: 0x287a3dff,
                selection: 0xdcecd6ff,
                code: 0xeff5eaff,
                dark: false,
            },
        );
        theme.typography.text_line_height = 1.78;
        theme.typography.text_letter_spacing = 0.02;
        theme.colors.text_h1 = Hsla::from(rgba(0x245c35ff));
        theme.colors.code_text = Hsla::from(rgba(0x4a6352ff));
        theme.colors.text_h2 = Hsla::from(rgba(0x28663aff));
        theme.colors.text_h3 = Hsla::from(rgba(0x2c7040ff));
        theme.colors.text_h4 = theme.colors.text_h3;
        theme.colors.text_h5 = theme.colors.text_h3;
        theme.colors.text_h6 = theme.colors.text_h3;
        theme.typography.h1_size = 30.0;
        theme.typography.h2_size = 22.0;
        theme.dimensions.block_gap = 10.0;
        theme.dimensions.centered_min_ratio = 0.76;
        theme.dimensions.code_bg_pad_x = 2.0;
        theme.dimensions.code_block_padding_x = 14.0;
        theme.dimensions.code_block_padding_y = 12.0;
        theme.dimensions.table_cell_padding_y = 7.0;
        theme
    }

    pub fn midnight_theme() -> Self {
        let mut theme = recolor_builtin(
            Self::default_theme(),
            BUILTIN_THEME_MIDNIGHT_NAME,
            BuiltinPalette {
                window: 0x171d27ff,
                panel: 0x1d2633ff,
                panel_hover: 0x263244ff,
                text: 0xe8edf4ff,
                muted: 0x9caabeff,
                line: 0x344254ff,
                accent: 0x8bb6e9ff,
                selection: 0x2d4868ff,
                code: 0x202b3aff,
                dark: true,
            },
        );
        theme.typography.text_line_height = 1.68;
        theme.typography.h1_size = 31.0;
        theme.typography.h2_size = 23.0;
        theme.dimensions.block_gap = 7.0;
        theme
    }

    pub fn ink_theme() -> Self {
        let mut theme = recolor_builtin(
            Self::default_theme(),
            BUILTIN_THEME_INK_NAME,
            BuiltinPalette {
                window: 0x171616ff,
                panel: 0x201d1dff,
                panel_hover: 0x2c2725ff,
                text: 0xeee8ddff,
                muted: 0xaea398ff,
                line: 0x3b3430ff,
                accent: 0xd6a079ff,
                selection: 0x503d31ff,
                code: 0x282422ff,
                dark: true,
            },
        );
        theme.typography.text_line_height = 1.7;
        let serif = if cfg!(target_os = "macos") {
            "Songti SC"
        } else {
            "Georgia"
        };
        theme.typography.body_font_family = serif.into();
        theme.typography.heading_font_family = serif.into();
        theme.typography.h1_size = 31.0;
        theme.typography.h1_weight = FontWeightDef::Semibold;
        theme.typography.h2_size = 23.0;
        theme.typography.h2_weight = FontWeightDef::Semibold;
        theme.dimensions.block_gap = 8.0;
        theme.dimensions.writing_max_width = 720.0;
        theme.dimensions.h1_border_width = 0.0;
        theme.dimensions.h1_margin_bottom = 10.0;
        theme
    }

    /// Parses a theme from JSON text.
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(json)?)
    }

    /// Loads a theme from a JSON file on disk.
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        Self::from_json(&json)
    }

    /// Serializes the theme into pretty-printed JSON.
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}
