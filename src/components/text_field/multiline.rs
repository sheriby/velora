use super::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};

pub(super) struct State {
    pub minimum_rows: usize,
    pub maximum_rows: usize,
    pub layout: Option<Arc<Layout>>,
    pub scroll_y: Pixels,
    pub reveal_caret: bool,
}

pub(super) struct Row {
    pub start: usize,
    pub y: Pixels,
    pub line: WrappedLine,
}

pub(super) struct Layout {
    pub rows: Vec<Row>,
    pub line_height: Pixels,
    pub width: Pixels,
    pub height: Pixels,
}

impl Layout {
    pub fn position(&self, index: usize) -> Point<Pixels> {
        let Some(row) = self
            .rows
            .iter()
            .find(|row| index >= row.start && index <= row.start + row.line.len())
        else {
            return point(px(0.0), px(0.0));
        };
        let local = index.saturating_sub(row.start).min(row.line.len());
        for (ordinal, boundary) in row.line.wrap_boundaries().iter().enumerate() {
            let offset = row
                .line
                .runs()
                .get(boundary.run_ix)
                .and_then(|run| run.glyphs.get(boundary.glyph_ix))
                .map(|glyph| glyph.index);
            if offset == Some(local) {
                return point(px(0.0), row.y + self.line_height * (ordinal + 1));
            }
        }
        let position = row
            .line
            .position_for_index(local, self.line_height)
            .unwrap_or_default();
        point(position.x, row.y + position.y)
    }

    pub fn index(&self, position: Point<Pixels>) -> usize {
        let Some(row) = self
            .rows
            .iter()
            .find(|row| position.y < row.y + row.line.size(self.line_height).height)
            .or_else(|| self.rows.last())
        else {
            return 0;
        };
        let offset = match row.line.closest_index_for_position(
            point(position.x, (position.y - row.y).max(px(0.0))),
            self.line_height,
        ) {
            Ok(index) | Err(index) => index,
        };
        row.start + offset.min(row.line.len())
    }

    fn rectangles(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let mut rectangles = Vec::new();
        if range.is_empty() {
            return rectangles;
        }
        for (ordinal, row) in self.rows.iter().enumerate() {
            let starts = std::iter::once(0)
                .chain(row.line.wrap_boundaries().iter().filter_map(|boundary| {
                    row.line
                        .runs()
                        .get(boundary.run_ix)
                        .and_then(|run| run.glyphs.get(boundary.glyph_ix))
                        .map(|glyph| glyph.index)
                }))
                .collect::<Vec<_>>();
            let ends = starts
                .iter()
                .copied()
                .skip(1)
                .chain(std::iter::once(row.line.len()));
            for (visual_row, (start, end)) in starts.iter().copied().zip(ends).enumerate() {
                let left = range.start.max(row.start + start);
                let right = range.end.min(row.start + end);
                let newline = self.rows.get(ordinal + 1).is_some()
                    && end == row.line.len()
                    && range.start <= row.start + end
                    && range.end > row.start + end;
                if left < right || newline {
                    let origin_x = row.line.unwrapped_layout.x_for_index(start);
                    let left_x = row
                        .line
                        .unwrapped_layout
                        .x_for_index(left.saturating_sub(row.start).min(end))
                        - origin_x;
                    let right_x = if newline {
                        self.width
                    } else {
                        row.line
                            .unwrapped_layout
                            .x_for_index(right.saturating_sub(row.start).min(end))
                            - origin_x
                    };
                    rectangles.push(Bounds::new(
                        point(left_x, row.y + self.line_height * visual_row),
                        size((right_x - left_x).max(px(1.0)), self.line_height),
                    ));
                }
            }
        }
        rectangles
    }
}

pub(super) struct TextAreaElement {
    pub input: Entity<TextField>,
}
type Cache = Rc<RefCell<Option<Arc<Layout>>>>;
pub(super) struct Paint {
    layout: Arc<Layout>,
    origin: Point<Pixels>,
    selection: Vec<PaintQuad>,
    marked: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
    hitbox: Hitbox,
}

fn shape(
    input: &TextField,
    width: Pixels,
    font_size: Pixels,
    font: Font,
    color: Hsla,
    window: &mut Window,
) -> Arc<Layout> {
    let text: SharedString = if input.value.is_empty() {
        input.placeholder.clone()
    } else {
        input.value.replace('\r', " ").into()
    };
    let run = TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
        font_size: None,
    };
    let lines = match window.text_system().shape_text(
        text,
        font_size,
        &[run],
        if input.value.is_empty() { None } else { Some(width.max(px(2.0))) },
        None,
    ) {
        Ok(lines) => lines,
        Err(error) => {
            eprintln!("绘制多行输入文字失败：{error}");
            Default::default()
        }
    };
    let line_height = (font_size * 1.5).max(px(22.0));
    let mut rows = Vec::new();
    let mut start = 0;
    let mut y = px(0.0);
    for line in lines {
        let length = line.len();
        let height = line.size(line_height).height;
        rows.push(Row { start, y, line });
        start += length + 1;
        y += height;
    }
    Arc::new(Layout {
        rows,
        line_height,
        width,
        height: y,
    })
}

impl IntoElement for TextAreaElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for TextAreaElement {
    type RequestLayoutState = Cache;
    type PrepaintState = Paint;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Cache) {
        let cache: Cache = Default::default();
        let measured = cache.clone();
        let input = self.input.clone();
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let font = text_style.font();
        let theme = cx.global::<crate::theme::ThemeManager>().derived_theme(cx);
        let color = if input.read(cx).value.is_empty() {
            theme.colors.dialog_muted
        } else {
            theme.colors.text_default
        };
        let (minimum, maximum) = input
            .read(cx)
            .multiline
            .as_ref()
            .map_or((1, 1), |state| (state.minimum_rows, state.maximum_rows));
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.min_size.width = px(0.0).into();
        let id = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.unwrap_or_else(|| match available.width {
                AvailableSpace::Definite(width) => width,
                AvailableSpace::MinContent => px(1.0),
                AvailableSpace::MaxContent => window.viewport_size().width,
            });
            if width <= px(2.0) {
                return size(width, (font_size * 1.5).max(px(22.0)) * minimum);
            }
            let layout = shape(
                input.read(cx),
                width,
                font_size,
                font.clone(),
                color,
                window,
            );
            let height = layout
                .height
                .max(layout.line_height * minimum)
                .min(layout.line_height * maximum);
            *measured.borrow_mut() = Some(layout);
            size(width, height)
        });
        (id, cache)
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        cache: &mut Cache,
        window: &mut Window,
        cx: &mut App,
    ) -> Paint {
        let theme = cx.global::<crate::theme::ThemeManager>().derived_theme(cx);
        let focused = self.input.read(cx).focus.is_focused(window);
        let layout = cache
            .borrow()
            .as_ref()
            .filter(|layout| layout.width == bounds.size.width)
            .cloned()
            .unwrap_or_else(|| {
                let style = window.text_style();
                let input = self.input.read(cx);
                let color = if input.value.is_empty() {
                    theme.colors.dialog_muted
                } else {
                    theme.colors.text_default
                };
                shape(
                    input,
                    bounds.size.width,
                    style.font_size.to_pixels(window.rem_size()),
                    style.font(),
                    color,
                    window,
                )
            });
        let (selected, marked, caret) = {
            let input = self.input.read(cx);
            (
                input.normalized_selection(),
                input.marked_range.clone(),
                input.selected_range.end,
            )
        };
        let position = layout.position(caret);
        let scroll = self.input.update(cx, |input, _| {
            let Some(state) = input.multiline.as_mut() else {
                return px(0.0);
            };
            if focused && state.reveal_caret {
                if position.y < state.scroll_y {
                    state.scroll_y = position.y;
                }
                if position.y + layout.line_height > state.scroll_y + bounds.size.height {
                    state.scroll_y = position.y + layout.line_height - bounds.size.height;
                }
                state.reveal_caret = false;
            }
            state.scroll_y = state
                .scroll_y
                .max(px(0.0))
                .min((layout.height - bounds.size.height).max(px(0.0)));
            state.layout = Some(layout.clone());
            input.last_bounds = Some(bounds);
            state.scroll_y
        });
        let origin = point(bounds.left(), bounds.top() - scroll);
        let selection = if focused {
            layout
                .rectangles(selected.clone())
                .into_iter()
                .map(|rectangle| fill(rectangle + origin, theme.colors.selection))
                .collect()
        } else {
            Vec::new()
        };
        let marked = marked.map_or_else(Vec::new, |range| {
            layout
                .rectangles(range)
                .into_iter()
                .map(|rectangle| {
                    fill(
                        Bounds::new(
                            origin + rectangle.bottom_left() - point(px(0.0), px(1.0)),
                            size(rectangle.size.width, px(1.0)),
                        ),
                        theme.colors.text_default,
                    )
                })
                .collect()
        });
        let cursor = (focused && selected.is_empty() && window.is_window_active()).then(|| {
            fill(
                Bounds::new(origin + position, size(px(1.0), layout.line_height)),
                theme.colors.cursor,
            )
        });
        Paint {
            layout,
            origin,
            selection,
            marked,
            cursor,
            hitbox: window.insert_hitbox(bounds, HitboxBehavior::Normal),
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Cache,
        state: &mut Paint,
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
            input.painted_selection = !state.selection.is_empty();
        });
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for rectangle in state.selection.drain(..) {
                window.paint_quad(rectangle);
            }
            for row in &state.layout.rows {
                if let Err(error) = row.line.paint(
                    state.origin + point(px(0.0), row.y),
                    state.layout.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                ) {
                    eprintln!("绘制多行输入失败：{error}");
                }
            }
            for rectangle in state.marked.drain(..) {
                window.paint_quad(rectangle);
            }
            if let Some(cursor) = state.cursor.take() {
                window.paint_quad(cursor);
            }
        });
    }
}
