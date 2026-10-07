use super::*;

impl Editor {
    pub(crate) fn active_overlay_input(&self, window: &Window) -> OverlayInputKind {
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
            OverlayInputKind::Query => {
                self.workspace.search_query = updated;
                self.workspace.search_selected_range = selection;
                self.workspace.search_marked_range = marked_range;
                if !marked && (self.workspace.search_query != old || was_marked) {
                    self.schedule_workspace_search(cx);
                }
            }
            OverlayInputKind::Replace => {
                self.workspace.replace_query = updated;
                self.workspace.replace_selected_range = selection;
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
                    state.draft = updated;
                    state.selected_range = selection;
                    state.marked_range = marked_range;
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
}
