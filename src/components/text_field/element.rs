use super::*;

pub(super) struct TextFieldElement {
    pub input: Entity<TextField>,
}
pub(super) struct FieldPaint {
    line: ShapedLine,
    origin: Point<Pixels>,
    line_height: Pixels,
    selection: Option<PaintQuad>,
    cursor: Option<PaintQuad>,
    marked: Option<PaintQuad>,
    hitbox: Hitbox,
}

impl IntoElement for TextFieldElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextFieldElement {
    type RequestLayoutState = ();
    type PrepaintState = FieldPaint;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = (window.text_style().font_size.to_pixels(window.rem_size()) * 1.5)
            .max(px(22.0))
            .into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> FieldPaint {
        let theme = cx.global::<crate::theme::ThemeManager>().derived_theme(cx);
        let (display, empty, focused, selected, caret, marked, old_scroll) = {
            let input = self.input.read(cx);
            let display = if input.value.is_empty() {
                input.placeholder.clone()
            } else if input.masked {
                "•".repeat(input.value.chars().count()).into()
            } else {
                input.value.replace(['\r', '\n'], " ").into()
            };
            let range = input.normalized_selection();
            let selected = input.display_offset(range.start)..input.display_offset(range.end);
            let caret = input.display_offset(input.selected_range.end);
            let marked = input
                .marked_range
                .as_ref()
                .map(|range| input.display_offset(range.start)..input.display_offset(range.end));
            (
                display,
                input.value.is_empty(),
                input.focus.is_focused(window),
                selected,
                caret,
                marked,
                input.scroll_x,
            )
        };
        let color = if empty {
            theme.colors.dialog_muted
        } else {
            theme.colors.text_default
        };
        let style = window.text_style();
        let run = TextRun {
            len: display.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
            font_size: None,
        };
        let line = window.text_system().shape_line(
            display,
            style.font_size.to_pixels(window.rem_size()),
            &[run],
            None,
        );
        let caret_x = line.x_for_index(if empty { 0 } else { caret });
        let mut scroll = old_scroll.max(0.0);
        let available = f32::from(bounds.size.width).max(2.0);
        if empty {
            scroll = 0.0;
        } else if f32::from(caret_x) < scroll {
            scroll = f32::from(caret_x);
        } else if f32::from(caret_x) > scroll + available - 2.0 {
            scroll = (f32::from(caret_x) - available + 2.0).max(0.0);
        }
        let origin = point(bounds.left() - px(scroll), bounds.top());
        let selection = if focused && !selected.is_empty() {
            let left = line.x_for_index(selected.start.min(selected.end));
            let right = line.x_for_index(selected.start.max(selected.end));
            Some(fill(
                Bounds::from_corners(
                    point(origin.x + left, bounds.top()),
                    point(origin.x + right, bounds.bottom()),
                ),
                theme.colors.selection,
            ))
        } else {
            None
        };
        let cursor = if focused && selected.is_empty() && window.is_window_active() {
            Some(fill(
                Bounds::new(
                    point(origin.x + caret_x, bounds.top()),
                    size(px(1.0), bounds.size.height),
                ),
                theme.colors.cursor,
            ))
        } else {
            None
        };
        let marked = marked.map(|range| {
            let left = line.x_for_index(range.start);
            let right = line.x_for_index(range.end);
            fill(
                Bounds::new(
                    point(origin.x + left, bounds.bottom() - px(1.0)),
                    size((right - left).max(px(1.0)), px(1.0)),
                ),
                theme.colors.text_default,
            )
        });
        self.input.update(cx, |input, _| {
            input.last_bounds = Some(bounds);
            input.last_line = Some(line.clone());
            input.scroll_x = scroll;
        });
        FieldPaint {
            line,
            origin,
            line_height: bounds.size.height,
            selection,
            cursor,
            marked,
            hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _layout: &mut (),
        state: &mut FieldPaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        if state.hitbox.is_hovered(window) {
            window.set_cursor_style(CursorStyle::IBeam, &state.hitbox);
        }
        let focus = self.input.read(cx).focus.clone();
        if focus.is_focused(window) {
            window.handle_input(
                &focus,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }
        #[cfg(test)]
        self.input.update(cx, |input, _| {
            input.painted_caret = state.cursor.is_some();
            input.painted_selection = state.selection.is_some();
            input.painted_marked = state.marked.is_some();
        });
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = state.selection.take() {
                window.paint_quad(selection);
            }
            if let Err(error) = state
                .line
                .paint(state.origin, state.line_height, window, cx)
            {
                eprintln!("绘制输入文字失败：{error}");
            }
            if let Some(marked) = state.marked.take() {
                window.paint_quad(marked);
            }
            if let Some(cursor) = state.cursor.take() {
                window.paint_quad(cursor);
            }
        });
    }
}
