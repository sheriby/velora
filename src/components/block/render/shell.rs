use super::*;

impl Block {
    /// 标题折叠 chevron（roadmap C7）：绝对定位在标题行左侧留白里，不改变
    /// 正文起始位置；只有含章节内容的标题（或已折叠的标题）才渲染它。
    /// 点击只发出事件，折叠状态由编辑器统一翻转，避免顺带移动光标。
    pub(crate) fn heading_fold_row(
        &self,
        row: Stateful<Div>,
        text: AnyElement,
        font_size: f32,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let row = row.line_height(relative(1.25));
        if !self.foldable && !self.folded {
            return row.child(text);
        }
        let c = &theme.colors;
        let folded = self.folded;
        let chevron = div()
            .id("heading-fold-chevron")
            .debug_selector(|| "heading-fold-chevron".to_string())
            .absolute()
            .left(px(-HEADING_FOLD_CHEVRON_GUTTER))
            .top(px(0.0))
            .w(px(HEADING_FOLD_CHEVRON_GUTTER))
            .h(px(font_size * 1.25))
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(c.dialog_secondary_button_hover))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_block, _event, _window, cx| {
                    cx.stop_propagation();
                    cx.emit(BlockEvent::RequestToggleFold);
                }),
            )
            .child(
                svg()
                    .path(if folded {
                        HEADING_FOLD_CHEVRON_RIGHT
                    } else {
                        HEADING_FOLD_CHEVRON_DOWN
                    })
                    .size(px(HEADING_FOLD_CHEVRON_ICON_SIZE))
                    .text_color(c.dialog_muted),
            );
        row.child(div().relative().w_full().child(chevron).child(text))
    }

    /// 空行块（空段落、未聚焦、非源码模式）渲染时收窄到只剩一个块间距。
    ///
    /// 松列表 `- xx` / 空行 / `- xx` 里，空行块原本占「一行高（约 27px）+ 上下
    /// padding（8px）」= 35px，加上相邻块间距就是 47px，比正文一行还高，看着就是
    /// 一个巨大的空行（用户报修）。收窄后空行只贡献一个 `block_gap`，松列表项目
    /// 之间是正常的段间距。聚焦后恢复整行高度，否则光标和输入框看不见。
    pub(crate) fn collapses_to_blank_gap(&self, focused: bool, source_mode: bool) -> bool {
        !focused
            && !source_mode
            && self.kind() == BlockKind::Paragraph
            && self.marked_range.is_none()
            && self.display_text().trim().is_empty()
    }

    pub(crate) fn render_shell(
        &self,
        block_id: ElementId,
        source_mode: bool,
        focused: bool,
        cursor_style: CursorStyle,
        padding_left: f32,
        padding_right: f32,
        dimensions: &ThemeDimensions,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let collapsed_blank_line = self.collapses_to_blank_gap(focused, source_mode);
        // 源码模式：块内不再吃上下内边距（相邻块之间的 8px 会让行距翻倍）。
        // 但最小高度必须留着 block_min_height——行计划的估算值就是它，两者不一致
        // 时每行差几像素、几十行累计成漂移，虚拟滚动就会漏挂载屏幕上那几行，
        // 表现成「空白行没有行号」。
        let min_height = if collapsed_blank_line {
            dimensions.block_gap
        } else {
            dimensions.block_min_height
        };
        let padding_y = if source_mode || collapsed_blank_line {
            0.0
        } else {
            dimensions.block_padding_y
        };
        let base = div()
            .id(block_id)
            .key_context(BLOCK_EDITOR_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_newline))
            .on_action(cx.listener(Self::on_delete_back))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_word_delete_back))
            .on_action(cx.listener(Self::on_word_delete_forward))
            .on_action(cx.listener(Self::on_focus_prev))
            .on_action(cx.listener(Self::on_focus_next))
            .on_action(cx.listener(Self::on_move_left))
            .on_action(cx.listener(Self::on_move_right))
            .on_action(cx.listener(Self::on_word_move_left))
            .on_action(cx.listener(Self::on_word_move_right))
            .on_action(cx.listener(Self::on_home))
            .on_action(cx.listener(Self::on_end))
            .on_action(cx.listener(Self::on_block_up))
            .on_action(cx.listener(Self::on_block_down))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_word_select_left))
            .on_action(cx.listener(Self::on_word_select_right))
            .on_action(cx.listener(Self::on_select_home))
            .on_action(cx.listener(Self::on_select_end))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_copy))
            .on_action(cx.listener(Self::on_cut))
            .on_action(cx.listener(Self::on_paste))
            .on_action(cx.listener(Self::on_paste_as_plain_text))
            .on_action(cx.listener(Self::on_exit_code_block))
            .on_key_down(cx.listener(Self::on_block_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .min_w(px(0.0))
            .flex_shrink_0()
            .min_h(px(min_height))
            .py(px(padding_y))
            .pl(px(padding_left))
            .pr(px(padding_right))
            .cursor(cursor_style);

        // 测试用的调试选择器：单元测试用 debug_bounds 断言空行块真的收窄了。
        // 只在带调试断言的构建里开着，release 不背这个开销。
        #[cfg(debug_assertions)]
        let base = if collapsed_blank_line {
            base.debug_selector(|| "block-blank-line".to_string())
        } else {
            base.debug_selector(|| "block-shell".to_string())
        };

        if source_mode {
            base
        } else {
            base.on_action(cx.listener(Self::on_indent_block))
                .on_action(cx.listener(Self::on_outdent_block))
                .on_action(cx.listener(Self::on_bold_selection))
                .on_action(cx.listener(Self::on_italic_selection))
                .on_action(cx.listener(Self::on_underline_selection))
                .on_action(cx.listener(Self::on_code_selection))
                .on_action(cx.listener(Self::on_strikethrough_selection))
                .on_action(cx.listener(Self::on_highlight_selection))
                .on_action(cx.listener(Self::on_superscript_selection))
                .on_action(cx.listener(Self::on_subscript_selection))
        }
    }
}
