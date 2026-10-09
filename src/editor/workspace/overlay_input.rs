use super::*;

impl Editor {
    pub(crate) fn active_overlay_input(&self, window: &Window) -> OverlayInputKind {
        if self.workspace.name_edit.as_ref().is_some_and(|edit| edit.focus.is_focused(window)) {
            return OverlayInputKind::TreeName;
        }
        if self
            .formula_editor
            .as_ref()
            .and_then(|state| state.focus.as_ref())
            .is_some_and(|focus| focus.is_focused(window))
        {
            return OverlayInputKind::FormulaEditor;
        }
        if self
            .command_palette
            .as_ref()
            .and_then(|state| state.focus.as_ref())
            .is_some_and(|focus| focus.is_focused(window))
        {
            return OverlayInputKind::CommandPalette;
        }
        if self
            .quick_open
            .as_ref()
            .and_then(|state| state.focus.as_ref())
            .is_some_and(|focus| focus.is_focused(window))
        {
            return OverlayInputKind::QuickOpen;
        }
        if self
            .workspace
            .replace_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window))
        {
            OverlayInputKind::Replace
        } else {
            OverlayInputKind::Query
        }
    }

    pub(crate) fn input_text(&self, kind: OverlayInputKind) -> &str {
        match kind {
            OverlayInputKind::TreeName => self.workspace.name_edit.as_ref().map(|edit| edit.draft.as_str()).unwrap_or_default(),
            OverlayInputKind::Query => &self.workspace.search_query,
            OverlayInputKind::Replace => &self.workspace.replace_query,
            OverlayInputKind::QuickOpen => self
                .quick_open
                .as_ref()
                .map(|state| state.query.as_str())
                .unwrap_or_default(),
            OverlayInputKind::CommandPalette => self
                .command_palette
                .as_ref()
                .map(|state| state.query.as_str())
                .unwrap_or_default(),
            OverlayInputKind::FormulaEditor => self
                .formula_editor
                .as_ref()
                .map(|state| state.draft.as_str())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn input_selection(&self, kind: OverlayInputKind) -> Range<usize> {
        match kind {
            OverlayInputKind::TreeName => self.workspace.name_edit.as_ref().map(|edit| edit.selected_range.clone()).unwrap_or_default(),
            OverlayInputKind::Query => self.workspace.search_selected_range.clone(),
            OverlayInputKind::Replace => self.workspace.replace_selected_range.clone(),
            OverlayInputKind::QuickOpen => self
                .quick_open
                .as_ref()
                .map(|state| state.selected_range.clone())
                .unwrap_or_default(),
            OverlayInputKind::CommandPalette => self
                .command_palette
                .as_ref()
                .map(|state| state.selected_range.clone())
                .unwrap_or_default(),
            OverlayInputKind::FormulaEditor => self
                .formula_editor
                .as_ref()
                .map(|state| state.selected_range.clone())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn input_marked(&self, kind: OverlayInputKind) -> Option<Range<usize>> {
        match kind {
            OverlayInputKind::TreeName => self.workspace.name_edit.as_ref().and_then(|edit| edit.marked_range.clone()),
            OverlayInputKind::Query => self.workspace.search_marked_range.clone(),
            OverlayInputKind::Replace => self.workspace.replace_marked_range.clone(),
            OverlayInputKind::QuickOpen => self
                .quick_open
                .as_ref()
                .and_then(|state| state.marked_range.clone()),
            OverlayInputKind::CommandPalette => self
                .command_palette
                .as_ref()
                .and_then(|state| state.marked_range.clone()),
            OverlayInputKind::FormulaEditor => self
                .formula_editor
                .as_ref()
                .and_then(|state| state.marked_range.clone()),
        }
    }

    /// Applies an edit to the focused single-line overlay input (search query,
    /// search replace, or quick switcher) and refreshes what that input drives.
    pub(crate) fn replace_overlay_input_text(
        &mut self,
        kind: impl Into<OverlayInputKind>,
        range: Range<usize>,
        new_text: &str,
        selected_in_inserted: Option<Range<usize>>,
        marked: bool,
        cx: &mut Context<Self>,
    ) {
        let kind = kind.into();
        let (old, was_marked) = match kind {
            OverlayInputKind::TreeName => {
                let Some(edit) = self.workspace.name_edit.as_ref().filter(|edit| !edit.pending) else { return; };
                (edit.draft.clone(), edit.marked_range.is_some())
            }
            OverlayInputKind::Query => (
                self.workspace.search_query.clone(),
                self.workspace.search_marked_range.is_some(),
            ),
            OverlayInputKind::Replace => (
                self.workspace.replace_query.clone(),
                self.workspace.replace_marked_range.is_some(),
            ),
            OverlayInputKind::QuickOpen => (
                self.quick_open
                    .as_ref()
                    .map(|state| state.query.clone())
                    .unwrap_or_default(),
                self.quick_open
                    .as_ref()
                    .is_some_and(|state| state.marked_range.is_some()),
            ),
            OverlayInputKind::CommandPalette => (
                self.command_palette
                    .as_ref()
                    .map(|state| state.query.clone())
                    .unwrap_or_default(),
                self.command_palette
                    .as_ref()
                    .is_some_and(|state| state.marked_range.is_some()),
            ),
            OverlayInputKind::FormulaEditor => (
                self.formula_editor
                    .as_ref()
                    .map(|state| state.draft.clone())
                    .unwrap_or_default(),
                self.formula_editor
                    .as_ref()
                    .is_some_and(|state| state.marked_range.is_some()),
            ),
        };
        let start = range.start.min(old.len());
        let end = range.end.min(old.len()).max(start);
        if !old.is_char_boundary(start) || !old.is_char_boundary(end) {
            return;
        }
        // 公式草稿是多行输入，换行是合法内容；其余 overlay 输入仍是单行。
        let inserted = match kind {
            OverlayInputKind::FormulaEditor => new_text.replace('\r', ""),
            _ => new_text.replace(['\r', '\n'], " "),
        };
        let updated = {
            let mut updated = old.clone();
            updated.replace_range(start..end, &inserted);
            updated
        };
        let inserted_end = start + inserted.len();
        let selection = selected_in_inserted
            .map(|selection| {
                start + selection.start.min(inserted.len())
                    ..start + selection.end.min(inserted.len())
            })
            .unwrap_or(inserted_end..inserted_end);
        let marked_range = (marked && !inserted.is_empty()).then_some(start..inserted_end);
        match kind {
            OverlayInputKind::TreeName => {
                if let Some(edit) = self.workspace.name_edit.as_mut() {
                    edit.draft = updated;
                    edit.selected_range = selection;
                    edit.caret = edit.selected_range.end;
                    edit.selection_anchor = edit.selected_range.start;
                    edit.marked_range = marked_range;
                    edit.error = None;
                }
            }
            OverlayInputKind::Query => {
                self.workspace.search_query = updated;
                self.workspace.search_selected_range = selection;
                self.workspace.search_input_state.reversed = false;
                self.workspace.search_marked_range = marked_range;
                if !marked && (self.workspace.search_query != old || was_marked) {
                    self.schedule_workspace_search(cx);
                }
            }
            OverlayInputKind::Replace => {
                self.workspace.replace_query = updated;
                self.workspace.replace_selected_range = selection;
                self.workspace.replace_input_state.reversed = false;
                self.workspace.replace_marked_range = marked_range;
            }
            OverlayInputKind::QuickOpen => {
                if let Some(state) = self.quick_open.as_mut() {
                    state.query = updated;
                    state.selected_range = selection;
                    state.marked_range = marked_range;
                    state.selected = 0;
                }
                if !marked && (self.input_text(kind) != old.as_str() || was_marked) {
                    self.refresh_quick_open_results(cx);
                }
            }
            OverlayInputKind::CommandPalette => {
                if let Some(state) = self.command_palette.as_mut() {
                    state.query = updated;
                    state.selected_range = selection;
                    state.marked_range = marked_range;
                    state.selected = 0;
                }
            }
            OverlayInputKind::FormulaEditor => {
                if let Some(state) = self.formula_editor.as_mut() {
                    if updated != old {
                        // 敲字也要留撤销的底（与 replace_formula_draft 同一套
                        // 合并规则：连打一串字算一步）。
                        crate::editor::formula_editor::push_draft_undo(
                            state,
                            crate::editor::formula_editor::draft_edit_is_single_character(
                                &old,
                                &(start..end),
                                &inserted,
                            ),
                        );
                    }
                    state.draft = updated;
                    state.selected_range = selection;
                    // 打完字选区收起，锚点跟光标走；光标重新常亮（闪烁后半秒）。
                    state.selection_anchor = state.selected_range.start;
                    state.caret_epoch = std::time::Instant::now();
                    state.caret_preferred = None;
                    state.marked_range = marked_range;
                    // 敲字这条路径也得刷 \ 补全：只同步预览的话，第一个反斜杠
                    // 永远不弹，要再敲一个字删掉才弹（用户报修）。
                    crate::editor::Editor::refresh_formula_draft_completion(state);
                    crate::editor::Editor::sync_formula_preview(state, cx);
                }
            }
        }
        cx.notify();
    }

    /// Applies an edit to the quick switcher query (roadmap E9).
    pub(crate) fn replace_quick_open_input_text(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        selected_in_inserted: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        self.replace_overlay_input_text(
            OverlayInputKind::QuickOpen,
            range,
            new_text,
            selected_in_inserted,
            false,
            cx,
        );
    }
    pub(super) fn search_input_state(&self, kind: SearchInputKind) -> &SearchInputState {
        match kind {
            SearchInputKind::Query => &self.workspace.search_input_state,
            SearchInputKind::Replace => &self.workspace.replace_input_state,
        }
    }

    fn search_input_state_mut(&mut self, kind: SearchInputKind) -> &mut SearchInputState {
        match kind {
            SearchInputKind::Query => &mut self.workspace.search_input_state,
            SearchInputKind::Replace => &mut self.workspace.replace_input_state,
        }
    }

    fn select_search_input_at(
        &mut self,
        kind: SearchInputKind,
        position: Point<Pixels>,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        let state = self.search_input_state(kind);
        let Some((line, bounds)) = state.last_line.as_ref().zip(state.last_bounds) else {
            return;
        };
        let index = line.closest_index_for_x(position.x - bounds.left() + state.scroll_x);
        let anchor = if extend {
            state.drag_anchor.unwrap_or(index)
        } else {
            index
        };
        let state = self.search_input_state_mut(kind);
        state.drag_anchor = Some(anchor);
        state.reversed = index < anchor;
        let selection = anchor.min(index)..anchor.max(index);
        match kind {
            SearchInputKind::Query => self.workspace.search_selected_range = selection,
            SearchInputKind::Replace => self.workspace.replace_selected_range = selection,
        }
        cx.notify();
    }

    /// Shared single-line input used by the query and replace fields. Clicks
    /// focus the field; key handling and IME route through `SearchInputKind`.
    pub(crate) fn render_search_input(
        &mut self,
        id: &'static str,
        value: String,
        placeholder: String,
        kind: SearchInputKind,
        focused: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = &theme.colors;
        let focus = match kind {
            SearchInputKind::Query => self
                .workspace
                .search_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone(),
            SearchInputKind::Replace => self
                .workspace
                .replace_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone(),
        };
        let focus_for_click = focus.clone();
        let focus_for_input = focus.clone();
        let input_editor = cx.entity();
        let editor = cx.entity().downgrade();
        let selection = match kind {
            SearchInputKind::Query => self.workspace.search_selected_range.clone(),
            SearchInputKind::Replace => self.workspace.replace_selected_range.clone(),
        };
        let input_state = self.search_input_state(kind);
        let caret = if input_state.reversed {
            selection.start
        } else {
            selection.end
        };
        let previous_scroll = input_state.scroll_x;
        let marked = match kind {
            SearchInputKind::Query => self.workspace.search_marked_range.clone(),
            SearchInputKind::Replace => self.workspace.replace_marked_range.clone(),
        };
        let show_placeholder = value.is_empty() && !focused;
        let text: SharedString = value.into();
        let colors = c.clone();
        let paint_colors = c.clone();

        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .relative()
            .cursor(CursorStyle::IBeam)
            .overflow_hidden()
            .track_focus(&focus)
            .flex_1()
            .min_w(px(0.0))
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(if focused {
                c.dialog_primary_button_bg
            } else {
                c.dialog_border
            })
            .bg(c.editor_background)
            .text_size(px(12.0))
            .child(
                canvas(
                    move |_, window, _| {
                        let run = |len, color| TextRun {
                            len,
                            font: window.text_style().font(),
                            color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                            font_size: None,
                        };
                        let line = window.text_system().shape_line(
                            text.clone(),
                            px(12.0),
                            &[run(text.len(), colors.text_default)],
                            None,
                        );
                        let placeholder_line = show_placeholder.then(|| {
                            let label: SharedString = placeholder.clone().into();
                            window.text_system().shape_line(
                                label.clone(),
                                px(12.0),
                                &[run(label.len(), colors.dialog_muted)],
                                None,
                            )
                        });
                        (line, placeholder_line)
                    },
                    move |bounds, (line, placeholder_line), window, cx| {
                        let width = (bounds.size.width - px(1.0)).max(px(1.0));
                        let caret_x = line.x_for_index(caret);
                        let scroll = if !focused {
                            px(0.0)
                        } else if caret_x < previous_scroll {
                            caret_x
                        } else if caret_x > previous_scroll + width {
                            caret_x - width
                        } else {
                            previous_scroll
                        };
                        let scroll = scroll.min((line.width - width).max(px(0.0)));
                        let origin = point(bounds.left() - scroll, bounds.top());
                        if focused && !selection.is_empty() {
                            let start = line.x_for_index(selection.start);
                            let end = line.x_for_index(selection.end);
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(origin.x + start, bounds.top()),
                                    size(end - start, bounds.size.height),
                                ),
                                paint_colors.selection,
                            ));
                        }
                        let display_line = placeholder_line.as_ref().unwrap_or(&line);
                        if let Err(error) =
                            display_line.paint(origin, bounds.size.height, window, cx)
                        {
                            eprintln!("绘制搜索输入框失败：{error}");
                        }
                        let caret_bounds = (focused && selection.is_empty()).then(|| {
                            Bounds::new(
                                point(origin.x + caret_x, bounds.top()),
                                size(px(1.0), bounds.size.height),
                            )
                        });
                        if let Some(caret_bounds) = caret_bounds {
                            window.paint_quad(fill(caret_bounds, paint_colors.cursor));
                        }
                        if focused && let Some(marked) = marked.as_ref() {
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        origin.x + line.x_for_index(marked.start),
                                        bounds.bottom() - px(1.0),
                                    ),
                                    size(
                                        line.x_for_index(marked.end)
                                            - line.x_for_index(marked.start),
                                        px(1.0),
                                    ),
                                ),
                                paint_colors.text_default,
                            ));
                        }
                        input_editor.update(cx, |editor, _| {
                            let state = editor.search_input_state_mut(kind);
                            state.last_line = Some(line.clone());
                            state.last_bounds = Some(bounds);
                            state.scroll_x = scroll;
                            state.caret_bounds = caret_bounds;
                        });
                        window.handle_input(
                            &focus_for_input,
                            ElementInputHandler::new(bounds, input_editor.clone()),
                            cx,
                        );
                        window.on_mouse_event({
                            let input_editor = input_editor.clone();
                            move |event: &MouseMoveEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture
                                    || event.pressed_button != Some(MouseButton::Left)
                                {
                                    return;
                                }
                                let handled = input_editor.update(cx, |editor, cx| {
                                    if editor.search_input_state(kind).drag_anchor.is_none() {
                                        return false;
                                    }
                                    editor.select_search_input_at(kind, event.position, true, cx);
                                    true
                                });
                                if handled {
                                    cx.stop_propagation();
                                }
                            }
                        });
                        window.on_mouse_event({
                            let input_editor = input_editor.clone();
                            move |event: &MouseUpEvent, phase, _, cx| {
                                if phase != DispatchPhase::Capture
                                    || event.button != MouseButton::Left
                                {
                                    return;
                                }
                                let handled = input_editor.update(cx, |editor, _| {
                                    editor
                                        .search_input_state_mut(kind)
                                        .drag_anchor
                                        .take()
                                        .is_some()
                                });
                                if handled {
                                    cx.stop_propagation();
                                }
                            }
                        });
                    },
                )
                .w_full()
                .h(px(16.0)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |editor, event: &MouseDownEvent, window, cx| {
                    // 用户点击的输入框优先于上一帧排队的搜索跳转焦点。
                    editor.workspace.search_focus_pending = false;
                    editor.pending_focus = None;
                    window.focus(&focus_for_click);
                    editor.select_search_input_at(kind, event.position, event.modifiers.shift, cx);
                    cx.stop_propagation();
                }),
            )
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.to_ascii_lowercase();
                let secondary = event.keystroke.modifiers.secondary();
                match key.as_str() {
                    "escape" => {
                        let _ = editor.update(cx, |editor, cx| match kind {
                            SearchInputKind::Query => {
                                editor.workspace.search_query.clear();
                                editor.workspace.search_selected_range = 0..0;
                                editor.workspace.search_marked_range = None;
                                editor.workspace.active_tab = WorkspaceTab::Files;
                                editor.workspace.search_focus_pending = false;
                                editor.schedule_workspace_search(cx);
                            }
                            SearchInputKind::Replace => {
                                editor.workspace.replace_visible = false;
                            }
                        });
                    }
                    "a" if secondary => {
                        let _ = editor.update(cx, |editor, cx| {
                            match kind {
                                SearchInputKind::Query => {
                                    editor.workspace.search_selected_range =
                                        0..editor.workspace.search_query.len();
                                }
                                SearchInputKind::Replace => {
                                    editor.workspace.replace_selected_range =
                                        0..editor.workspace.replace_query.len();
                                }
                            }
                            cx.notify();
                        });
                    }
                    "backspace" => {
                        let handled = editor.update(cx, |editor, cx| {
                            let (text, selected, marked) = match kind {
                                SearchInputKind::Query => (
                                    editor.workspace.search_query.clone(),
                                    editor.workspace.search_selected_range.clone(),
                                    editor.workspace.search_marked_range.clone(),
                                ),
                                SearchInputKind::Replace => (
                                    editor.workspace.replace_query.clone(),
                                    editor.workspace.replace_selected_range.clone(),
                                    editor.workspace.replace_marked_range.clone(),
                                ),
                            };
                            if marked.is_some() {
                                return false;
                            }
                            let range = if selected.start == selected.end {
                                let before = &text[..selected.start];
                                let start = before
                                    .grapheme_indices(true)
                                    .last()
                                    .map(|(start, _)| start)
                                    .unwrap_or(selected.start);
                                start..selected.start
                            } else {
                                selected
                            };
                            editor.replace_overlay_input_text(kind, range, "", None, false, cx);
                            true
                        });
                        if !matches!(handled, Ok(true)) {
                            return;
                        }
                    }
                    "v" if secondary => {
                        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                            let _ = editor.update(cx, |editor, cx| {
                                let selected = match kind {
                                    SearchInputKind::Query => {
                                        editor.workspace.search_selected_range.clone()
                                    }
                                    SearchInputKind::Replace => {
                                        editor.workspace.replace_selected_range.clone()
                                    }
                                };
                                editor.replace_overlay_input_text(
                                    kind, selected, &text, None, false, cx,
                                );
                            });
                        }
                    }
                    "enter" => {
                        let reverse = event.keystroke.modifiers.shift;
                        let _ = editor.update(cx, |editor, cx| match kind {
                            SearchInputKind::Query => {
                                editor.advance_search_match(reverse, window, cx);
                            }
                            SearchInputKind::Replace => {
                                editor.replace_current_search_match(window, cx);
                            }
                        });
                    }
                    _ => return,
                }
                cx.stop_propagation();
            })
            .into_any_element()
    }
}
