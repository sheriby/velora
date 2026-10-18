use super::*;
use gpui::prelude::FluentBuilder;

pub(super) struct ReviewDocument {
    id: String,
    edits: Vec<crate::ai::ProposedEdit>,
    original: Entity<Editor>,
    pub(super) proposed: Entity<Editor>,
    index: usize,
    compare: bool,
    versions: (u64, u64),
}

impl ReviewDocument {
    fn proposed_range(&self, index: usize) -> Option<std::ops::Range<usize>> {
        let edit = self.edits.get(index)?;
        let start = self
            .edits
            .iter()
            .take(index)
            .fold(edit.range.start, |offset, previous| {
                offset
                    .saturating_sub(previous.range.len())
                    .saturating_add(previous.replacement.len())
            });
        Some(start..start.saturating_add(edit.replacement.len()))
    }

    fn focus_change(&self, cx: &mut App) {
        if let Some(edit) = self.edits.get(self.index) {
            self.original.update(cx, |editor, cx| {
                editor.jump_to_document_search_range(edit.range.clone(), cx);
                editor.pending_focus = None;
            });
        }
        if let Some(range) = self.proposed_range(self.index) {
            self.proposed.update(cx, |editor, cx| {
                editor.jump_to_document_search_range(range, cx);
                editor.pending_focus = None;
            });
        }
    }
}

impl Editor {
    fn ai_review_can_accept(&self, document: Option<&str>, partial: bool) -> bool {
        let Some(session) = self.ai_session.as_ref() else {
            return false;
        };
        if self.is_read_only() || session.saved.running {
            return false;
        }
        let ids = session
            .saved
            .workspace
            .changes
            .proposals
            .keys()
            .filter(|id| document.is_none_or(|document| document == id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        !ids.is_empty()
            && session
                .saved
                .workspace
                .changes
                .validate_review(&ids, partial)
                .is_ok()
    }

    pub(in crate::editor) fn ai_review_key_down(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifier = if cfg!(target_os = "macos") {
            keystroke.modifiers.platform
        } else {
            keystroke.modifiers.control
        };
        if keystroke.key != "enter"
            || !modifier
            || keystroke.modifiers.alt
            || keystroke.modifiers.shift
            || self.modal_is_open()
            || self.info_dialog.is_some()
            || self.show_unsaved_changes_dialog
        {
            return false;
        }
        let change = self
            .ai_session
            .as_ref()
            .filter(|session| {
                session.visible
                    && session.mode == PanelMode::Task
                    && session.review_focus.contains_focused(window, cx)
            })
            .and_then(|session| session.review.as_ref())
            .map(|review| (review.id.clone(), review.index));
        if let Some((id, index)) = change
            && self.ai_review_can_accept(Some(&id), true)
        {
            self.apply_ai_review(Some(id), Some(index), window, cx);
            true
        } else {
            false
        }
    }

    pub(in crate::editor) fn sync_ai_surface_visibility(&mut self, cx: &mut Context<Self>) {
        if !self.ai_result_preview && self.split_parent.is_none() {
            let visible = self
                .ai_session
                .as_ref()
                .is_some_and(|session| session.visible);
            if self.ai_surface_visible.replace(visible) != visible {
                let owner = cx.entity_id();
                for group in self
                    .split_groups()
                    .iter()
                    .filter(|group| group.entity_id() != owner)
                {
                    group.update(cx, |_, cx| cx.notify());
                }
            }
        }
    }

    pub(in crate::editor) fn ai_interaction_has_focus(&self, window: &Window, cx: &App) -> bool {
        self.ai_session.as_ref().is_some_and(|session| {
            session.visible
                && (session.review_focus.contains_focused(window, cx)
                    || [
                        &session.input,
                        &session.language,
                        &session.instruction_name,
                        &session.instruction_body,
                        &session.excluded_input,
                        &session.context_input,
                    ]
                    .iter()
                    .any(|field| field.read(cx).is_focused(window))
                    || session.draft.as_ref().is_some_and(|draft| {
                        [
                            &draft.name,
                            &draft.address,
                            &draft.model,
                            &draft.secret,
                            &draft.context,
                        ]
                        .iter()
                        .any(|field| field.read(cx).is_focused(window))
                    }))
        })
    }

    pub(in crate::editor) fn document_listener<E: ?Sized + 'static>(
        cx: &Context<Self>,
        handler: fn(&mut Self, &E, &mut Window, &mut Context<Self>),
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        cx.listener(move |editor, event, window, cx| {
            if editor.ai_interaction_has_focus(window, cx) {
                cx.propagate();
            } else {
                handler(editor, event, window, cx);
            }
        })
    }

    fn ai_document_preview(
        &self,
        text: String,
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Entity<Editor> {
        let host = self.window_handle;
        cx.new(|cx| {
            let mut editor = Editor::from_markdown(cx, text, path);
            editor.reading_mode.set(true);
            editor.ai_result_preview = true;
            editor.window_handle = host;
            editor
        })
    }

    fn select_ai_review_document(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(session) = self.ai_session.as_ref() else {
            return;
        };
        let Some(proposal) = session.saved.workspace.changes.proposals.get(id) else {
            return;
        };
        let versions = (
            super::super::persistence::file_content_version(&proposal.base.source),
            super::super::persistence::file_content_version(&proposal.proposed),
        );
        if session
            .review
            .as_ref()
            .is_some_and(|review| review.id == id && review.versions == versions)
        {
            if let Some(session) = self.ai_session.as_mut() {
                session.review_dirty = false;
            }
            return;
        }
        let proposal = proposal.clone();
        let path = session
            .saved
            .target
            .as_ref()
            .filter(|target| target.document_id == id)
            .and_then(|target| target.path.clone())
            .or_else(|| {
                session
                    .saved
                    .workspace
                    .root
                    .as_ref()
                    .map(|root| root.join(id))
            });
        let previous = session.review.as_ref().filter(|review| review.id == id);
        let index = previous.map_or(0, |review| review.index);
        let compare = previous.is_some_and(|review| review.compare);
        let original = self.ai_document_preview(proposal.base.source.clone(), path.clone(), cx);
        let proposed = self.ai_document_preview(proposal.proposed.clone(), path, cx);
        let edits = proposal.edits();
        let review = ReviewDocument {
            id: id.to_owned(),
            original,
            proposed,
            index: index.min(edits.len().saturating_sub(1)),
            edits,
            compare,
            versions,
        };
        let ranges = review
            .edits
            .iter()
            .enumerate()
            .filter_map(|(index, _)| review.proposed_range(index))
            .collect();
        review
            .proposed
            .update(cx, |editor, _| editor.ai_review_ranges = ranges);
        review.original.update(cx, |editor, _| {
            editor.ai_review_ranges = review.edits.iter().map(|edit| edit.range.clone()).collect();
        });
        review.focus_change(cx);
        if let Some(session) = self.ai_session.as_mut() {
            session.review = Some(review);
            session.review_dirty = false;
        }
        cx.notify();
    }

    pub(in crate::editor) fn sync_ai_review_document(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self
            .ai_session
            .as_ref()
            .filter(|session| session.review_dirty)
        else {
            return;
        };
        if !session.visible && !session.saved.workspace.changes.proposals.is_empty() {
            return;
        }
        let selected = session
            .review
            .as_ref()
            .map(|review| review.id.as_str())
            .filter(|id| session.saved.workspace.changes.proposals.contains_key(*id))
            .or_else(|| {
                session
                    .saved
                    .workspace
                    .changes
                    .proposals
                    .keys()
                    .next()
                    .map(String::as_str)
            })
            .map(str::to_owned);
        if let Some(id) = selected {
            self.select_ai_review_document(&id, cx);
        } else if let Some(session) = self.ai_session.as_mut() {
            session.review = None;
            session.review_dirty = false;
        }
    }

    fn move_ai_review_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some(review) = self
            .ai_session
            .as_mut()
            .and_then(|session| session.review.as_mut())
        {
            if !review.edits.is_empty() {
                review.index = if forward {
                    (review.index + 1) % review.edits.len()
                } else {
                    (review.index + review.edits.len() - 1) % review.edits.len()
                };
                review.focus_change(cx);
            }
        }
        cx.notify();
    }

    pub(in crate::editor) fn render_ai_workspace(
        &self,
        document: AnyElement,
        theme: &crate::theme::Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.ai_session.as_ref().filter(|session| session.visible) else {
            return document;
        };
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        let mut body = document;
        if let Some(review) = &session.review {
            body = self.render_ai_document_review(review, theme, window, cx);
        } else if session.show_result {
            if let Some((index, message)) = session
                .saved
                .messages
                .iter()
                .enumerate()
                .rev()
                .find(|(_, message)| message.role == "assistant" && !message.text.is_empty())
                && let Some(preview) = session.previews.get(&index)
            {
                let inserted = message.text.clone();
                body = div()
                    .id("ai-result-document")
                    .debug_selector(|| "ai-result-document".into())
                    .track_focus(&session.review_focus)
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .h_full()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .px(px(24.0))
                            .h(px(48.0))
                            .flex_shrink_0()
                            .border_b_1()
                            .border_color(theme.colors.dialog_border)
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(13.0))
                                    .text_color(theme.colors.dialog_muted)
                                    .child(strings.ai_task_result.clone()),
                            )
                            .child(self.ai_button(
                                "ai-result-insert",
                                strings.ai_panel_insert.clone(),
                                false,
                                !self.is_read_only() && !session.saved.running,
                                theme,
                                cx,
                                move |editor, _, cx| editor.propose_ai_reply(&inserted, cx),
                            ))
                            .child(self.ai_button(
                                "ai-result-back",
                                strings.ai_task_return.clone(),
                                false,
                                true,
                                theme,
                                cx,
                                |editor, _, cx| {
                                    editor.hide_ai_panel(cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .child(preview.read(cx).editor.clone()),
                    )
                    .into_any_element();
            }
        }
        let active =
            !session.saved.messages.is_empty() || session.saved.running || session.review.is_some();
        if active {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .h_full()
                .child(div().flex_1().min_h_0().w_full().child(body))
                .child(self.render_ai_task_bar(theme, cx))
                .into_any_element()
        } else {
            body
        }
    }

    fn render_ai_document_review(
        &self,
        review: &ReviewDocument,
        theme: &crate::theme::Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let ready = !session.saved.running;
        let applicable = self.ai_review_can_accept(Some(&review.id), true);
        let id = review.id.clone();
        let reject_id = id.clone();
        let accept_id = id.clone();
        let file_accept_id = id.clone();
        let index = review.index;
        let count = review.edits.len();
        let caption = if id.starts_with("untitled:") {
            strings.ai_task_scope_document.clone()
        } else {
            id.clone()
        };
        let mut header = div()
            .w_full()
            .h(px(48.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(20.0))
            .border_b_1()
            .border_color(theme.colors.dialog_border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .child(caption),
            )
            .child(self.ai_button(
                "ai-review-compare",
                strings.ai_review_compare.clone(),
                review.compare,
                true,
                theme,
                cx,
                |editor, _, cx| {
                    if let Some(review) = editor
                        .ai_session
                        .as_mut()
                        .and_then(|session| session.review.as_mut())
                    {
                        review.compare = !review.compare;
                    }
                    cx.notify();
                },
            ));
        if session.saved.workspace.changes.proposals.len() > 1 {
            header = header.child(self.ai_button(
                "ai-review-accept-all",
                strings.ai_panel_accept_all.clone(),
                false,
                self.ai_review_can_accept(None, false),
                theme,
                cx,
                |editor, window, cx| editor.apply_ai_review(None, None, window, cx),
            ));
        }
        header = header.child(self.ai_button(
            "ai-review-file-accept",
            strings.ai_panel_accept_file.clone(),
            false,
            self.ai_review_can_accept(Some(&review.id), false),
            theme,
            cx,
            move |editor, window, cx| {
                editor.apply_ai_review(Some(file_accept_id.clone()), None, window, cx)
            },
        ));
        let narrow = f32::from(window.viewport_size().width) < 850.0;
        let mut files = div()
            .id("ai-review-files")
            .debug_selector(|| "ai-review-files".into())
            .w(px(if narrow { 140.0 } else { 200.0 }))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme.colors.dialog_border)
            .bg(theme.colors.dialog_surface)
            .px(px(8.0))
            .py(px(16.0))
            .gap(px(4.0))
            .child(
                div()
                    .px(px(8.0))
                    .pb(px(10.0))
                    .text_size(px(11.0))
                    .text_color(theme.colors.dialog_muted)
                    .child(strings.ai_review_title.clone()),
            );
        for (file_index, (path, proposal)) in
            session.saved.workspace.changes.proposals.iter().enumerate()
        {
            let chosen = path.clone();
            let active = path == &id;
            let label = Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            files = files.child(
                self.ai_button(
                    SharedString::from(format!("ai-review-file-{file_index}")),
                    label,
                    false,
                    true,
                    theme,
                    cx,
                    move |editor, _, cx| editor.select_ai_review_document(&chosen, cx),
                )
                .w_full()
                .py(px(9.0))
                .truncate()
                .when(active, |button| {
                    button.bg(theme.colors.dialog_secondary_button_hover)
                })
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme.colors.dialog_muted)
                        .child(format!(
                            "{} {}",
                            proposal.edit_count(),
                            strings.ai_review_changes
                        )),
                ),
            );
        }
        let mut documents = div().flex().flex_1().min_h_0().min_w_0().w_full();
        if review.compare {
            documents = documents.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .border_r_1()
                    .border_color(theme.colors.dialog_border)
                    .child(
                        div()
                            .h(px(32.0))
                            .flex_shrink_0()
                            .px(px(24.0))
                            .flex()
                            .items_center()
                            .text_size(px(11.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(strings.ai_review_original.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .child(review.original.clone()),
                    ),
            );
        }
        documents = documents.child(
            div()
                .id("ai-review-document")
                .debug_selector(|| "ai-review-document".into())
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .h_full()
                .debug_selector({
                    let id = id.clone();
                    move || format!("ai-review-active-{id}")
                })
                .when(review.compare, |column| {
                    column.child(
                        div()
                            .h(px(32.0))
                            .flex_shrink_0()
                            .px(px(24.0))
                            .flex()
                            .items_center()
                            .text_size(px(11.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(strings.ai_review_proposed.clone()),
                    )
                })
                .child(
                    div()
                        .id("ai-review-document-content")
                        .debug_selector(|| "ai-review-document".into())
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .child(review.proposed.clone()),
                ),
        );
        let navigation = div()
            .h(px(46.0))
            .flex_shrink_0()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .border_t_1()
            .border_color(theme.colors.dialog_border)
            .child(self.ai_button(
                "ai-review-previous",
                "↑",
                false,
                count > 1,
                theme,
                cx,
                |editor, _, cx| editor.move_ai_review_change(false, cx),
            ))
            .child(
                div()
                    .min_w(px(42.0))
                    .text_align(TextAlign::Center)
                    .text_size(px(12.0))
                    .text_color(theme.colors.dialog_muted)
                    .child(format!("{} / {count}", index + 1)),
            )
            .child(self.ai_button(
                "ai-review-next",
                "↓",
                false,
                count > 1,
                theme,
                cx,
                |editor, _, cx| editor.move_ai_review_change(true, cx),
            ))
            .child(div().w(px(12.0)))
            .child(self.ai_button(
                "ai-review-reject",
                strings.ai_panel_reject.clone(),
                false,
                ready,
                theme,
                cx,
                move |editor, _, cx| editor.reject_ai_review(&reject_id, Some(index), cx),
            ))
            .child(self.ai_button(
                "ai-review-accept",
                strings.ai_panel_accept_edit.clone(),
                true,
                applicable,
                theme,
                cx,
                move |editor, window, cx| {
                    editor.apply_ai_review(Some(accept_id.clone()), Some(index), window, cx)
                },
            ));
        let mut column = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_h_0()
            .min_w_0()
            .child(header);
        if session.saved.workspace.changes.invalid.contains(&id) {
            column = column.child(
                div()
                    .px(px(20.0))
                    .py(px(8.0))
                    .text_size(px(12.0))
                    .bg(theme.colors.callout_warning_bg)
                    .child(strings.ai_panel_stale.clone()),
            );
        } else if session
            .saved
            .workspace
            .changes
            .dependencies
            .get(&id)
            .is_some_and(|dependencies| !dependencies.is_empty())
        {
            column = column.child(
                div()
                    .px(px(20.0))
                    .py(px(8.0))
                    .text_size(px(12.0))
                    .text_color(theme.colors.dialog_muted)
                    .child(strings.ai_panel_related.clone()),
            );
        }
        column = column.child(documents).child(navigation);
        div()
            .id("ai-review-workspace")
            .debug_selector(|| "ai-review-workspace".into())
            .track_focus(&session.review_focus)
            .flex()
            .flex_1()
            .h_full()
            .min_h_0()
            .min_w_0()
            .bg(theme.colors.editor_background)
            .when(
                session.saved.workspace.changes.proposals.len() > 1,
                |surface| surface.child(files),
            )
            .child(column)
            .into_any_element()
    }

    fn render_ai_task_bar(
        &self,
        theme: &crate::theme::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        let activity = match session.activity.as_str() {
            "read_document" => &strings.ai_activity_read,
            "search_documents" => &strings.ai_activity_search,
            "get_outline" => &strings.ai_activity_outline,
            "list_documents" => &strings.ai_activity_list,
            "propose_edit" | "reset_proposal" => &strings.ai_activity_edit,
            "propose_document" => &strings.ai_activity_new,
            "read_skill" => &strings.ai_activity_skill,
            _ => &strings.ai_panel_running,
        };
        let status = session
            .error
            .as_deref()
            .unwrap_or(if session.saved.running {
                activity
            } else if session.review.is_some() {
                &strings.ai_panel_pending
            } else {
                &strings.ai_task_result
            });
        div()
            .id("ai-task-bar")
            .debug_selector(|| "ai-task-bar".into())
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .px(px(24.0))
            .py(px(10.0))
            .border_t_1()
            .border_color(theme.colors.dialog_border)
            .bg(theme.colors.dialog_surface)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(px(11.0))
                    .text_color(theme.colors.dialog_muted)
                    .child(div().flex_1().min_w_0().truncate().child(status.to_owned()))
                    .when(session.error.is_some(), |row| {
                        row.child(self.ai_button(
                            "ai-retry",
                            strings.ai_panel_retry.clone(),
                            false,
                            !session.saved.running,
                            theme,
                            cx,
                            |editor, _, cx| editor.retry_ai_task(cx),
                        ))
                    })
                    .when(session.saved.running, |row| {
                        row.child(self.ai_button(
                            "ai-stop",
                            "■",
                            false,
                            true,
                            theme,
                            cx,
                            |editor, _, cx| editor.stop_ai_task(cx),
                        ))
                    })
                    .child(self.ai_button(
                        "ai-task-details",
                        "⋯",
                        session.expanded,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.expanded = !session.expanded;
                            }
                            cx.notify();
                        },
                    ))
                    .child(self.ai_button(
                        "ai-task-hide",
                        "×",
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| editor.hide_ai_panel(cx),
                    )),
            )
            .child(self.render_ai_instruction_row(theme, cx))
            .into_any_element()
    }

    fn render_ai_instruction_row(
        &self,
        theme: &crate::theme::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(self.ai_button(
                "ai-context",
                "@",
                false,
                !session.saved.running,
                theme,
                cx,
                |editor, _, cx| editor.show_ai_context(cx),
            ))
            .child(div().flex_1().min_w_0().child(session.input.clone()))
            .child(
                self.ai_button("ai-send", "↑", true, true, theme, cx, |editor, _, cx| {
                    editor.send_ai_task(cx)
                })
                .tooltip(move |_, cx| {
                    cx.new(|_| crate::components::HoverPreviewTooltip {
                        label: strings.ai_panel_send.clone().into(),
                    })
                    .into()
                }),
            )
            .into_any_element()
    }

    pub(super) fn render_ai_command_bar(
        &self,
        theme: &crate::theme::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        let selected = session
            .saved
            .target
            .as_ref()
            .is_some_and(|target| !target.range.is_empty());
        let scope = if selected {
            &strings.ai_task_scope_selection
        } else {
            &strings.ai_task_scope_document
        };
        let mut commands = div().flex().items_center().gap(px(4.0));
        let actions = if selected {
            vec![
                AiCommand::Polish,
                AiCommand::Shorten,
                AiCommand::Translate,
                AiCommand::Explain,
            ]
        } else {
            vec![AiCommand::Summarize, AiCommand::Continue]
        };
        for command in actions {
            let label = command.label(&strings);
            commands = commands.child(self.ai_button(
                ("ai-command", command as usize),
                label.clone(),
                false,
                true,
                theme,
                cx,
                move |editor, window, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.command = command;
                        session.input.update(cx, |input, cx| {
                            input.set_value(&label, cx);
                            input.focus(window, cx);
                        });
                    }
                    if command != AiCommand::Translate {
                        editor.send_ai_task(cx);
                    }
                    cx.notify();
                },
            ));
        }
        let endpoint = session
            .settings
            .endpoints
            .iter()
            .find(|endpoint| endpoint.id == session.saved.endpoint_id)
            .or_else(|| session.settings.active_endpoint())
            .map(|endpoint| endpoint.name.clone())
            .unwrap_or_else(|| strings.ai_task_models.clone());
        div()
            .id("ai-assistant-panel")
            .debug_selector(|| "ai-assistant-panel".into())
            .w_full()
            .rounded(px(10.0))
            .shadow_lg()
            .border_1()
            .border_color(theme.colors.dialog_border)
            .bg(theme.colors.dialog_surface)
            .text_color(theme.colors.dialog_title)
            .text_size(px(12.0))
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("ai-command-bar")
                    .debug_selector(|| "ai-command-bar".into())
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(theme.colors.dialog_muted)
                                    .child(scope.clone()),
                            )
                            .child(div().flex_1())
                            .child(self.ai_button(
                                "ai-models",
                                endpoint,
                                false,
                                true,
                                theme,
                                cx,
                                |editor, _, cx| {
                                    if let Some(session) = editor.ai_session.as_mut() {
                                        session.mode = PanelMode::Models;
                                    }
                                    cx.notify();
                                },
                            ))
                            .child(self.ai_button(
                                "ai-options",
                                "⋯",
                                false,
                                true,
                                theme,
                                cx,
                                |editor, _, cx| {
                                    if let Some(session) = editor.ai_session.as_mut() {
                                        session.mode = PanelMode::Options;
                                    }
                                    cx.notify();
                                },
                            ))
                            .child(self.ai_button(
                                "ai-hide",
                                "×",
                                false,
                                true,
                                theme,
                                cx,
                                |editor, _, cx| editor.hide_ai_panel(cx),
                            )),
                    )
                    .when_some(session.error.as_ref(), |bar, error| {
                        bar.child(
                            div()
                                .id("ai-command-error")
                                .debug_selector(|| "ai-command-error".into())
                                .text_size(px(12.0))
                                .text_color(theme.colors.callout_warning_border)
                                .child(error.clone()),
                        )
                    })
                    .child(commands)
                    .when(session.command == AiCommand::Translate, |column| {
                        column.child(session.language.clone())
                    })
                    .child(self.render_ai_instruction_row(theme, cx)),
            )
            .into_any_element()
    }

    pub(super) fn render_ai_task_details(
        &self,
        theme: &crate::theme::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        let Some(session) = self.ai_session.as_ref() else {
            return div().into_any_element();
        };
        let mut content = div().flex().flex_col().gap(px(12.0));
        if let Some(error) = &session.error {
            content = content.child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.colors.callout_warning_border)
                    .child(error.clone()),
            );
        }
        if let Some(message) = session
            .saved
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
        {
            content = content.child(div().text_size(px(13.0)).child(message.text.clone()));
        }
        for (index, record) in session.saved.workspace.source_records.iter().enumerate() {
            let record = record.clone();
            let label = format!("{}:{}", record.path, record.line);
            content = content.child(self.ai_button(
                ("ai-source", index),
                label,
                false,
                true,
                theme,
                cx,
                move |editor, window, cx| editor.open_ai_source(record.clone(), window, cx),
            ));
        }
        content = content
            .child(self.ai_button(
                "ai-options-history",
                strings.ai_panel_history.clone(),
                false,
                true,
                theme,
                cx,
                |editor, _, cx| editor.show_ai_history(cx),
            ))
            .child(self.ai_button(
                "ai-task-options",
                strings.ai_panel_more.clone(),
                false,
                true,
                theme,
                cx,
                |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.mode = PanelMode::Options;
                    }
                    cx.notify();
                },
            ));
        if !session.saved.workspace.changes.proposals.is_empty() {
            content = content.child(self.ai_button(
                "ai-discard-all",
                strings.ai_panel_discard.clone(),
                false,
                !session.saved.running,
                theme,
                cx,
                |editor, _, cx| {
                    if let Some(session) = editor.ai_session.as_mut() {
                        session.saved.workspace.discard_proposals();
                        session.review_dirty = true;
                        session.show_result = false;
                    }
                    editor.persist_ai_session(cx);
                    cx.notify();
                },
            ));
        }
        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .p(px(16.0))
            .gap(px(18.0))
            .border_l_1()
            .border_color(theme.colors.dialog_border)
            .bg(theme.colors.dialog_surface)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.0))
                            .text_color(theme.colors.dialog_muted)
                            .child(strings.ai_task_details.clone()),
                    )
                    .child(self.ai_button(
                        "ai-details-hide",
                        "×",
                        false,
                        true,
                        theme,
                        cx,
                        |editor, _, cx| {
                            if let Some(session) = editor.ai_session.as_mut() {
                                session.expanded = false;
                            }
                            cx.notify();
                        },
                    )),
            )
            .child(
                div()
                    .id("ai-task-details-content")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .child(content),
            )
            .into_any_element()
    }
}
