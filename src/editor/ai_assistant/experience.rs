use super::inline_review::{MarkKind, ReviewBinding};
use super::*;
use gpui::prelude::FluentBuilder;

pub(super) struct ReviewDocument {
    pub(super) id: String,
    pub(super) edits: Vec<crate::ai::ProposedEdit>,
    pub(super) proposed: Entity<Editor>,
    pub(super) tracked: Entity<Editor>,
    pub(super) index: usize,
    pub(super) versions: (u64, u64),
    /// 基准版的块边界与它的内容版本：部分接受之外基准不变，重建投影时
    /// 复用这份缓存，省掉一次全文解析与物化。
    pub(super) before_blocks: Vec<std::ops::Range<usize>>,
    pub(super) base_version: u64,
}

impl ReviewDocument {
    fn focus_change(&self, cx: &mut App) {
        self.tracked.update(cx, |editor, cx| {
            let range = editor
                .ai_review_marks
                .iter()
                .filter(|mark| mark.edits.contains(&self.index))
                .find(|mark| mark.kind == MarkKind::Added)
                .or_else(|| {
                    editor
                        .ai_review_marks
                        .iter()
                        .find(|mark| mark.edits.contains(&self.index))
                })
                .map(|mark| mark.range.start..mark.range.start);
            if let Some(range) = range {
                editor.jump_to_document_search_range(range, cx);
                editor.pending_focus = None;
            }
        });
    }
}

impl Editor {
    pub(super) fn ai_review_can_accept(&self, document: Option<&str>, partial: bool) -> bool {
        let Some(session) = self.ai_session.as_ref() else {
            return false;
        };
        if self.is_read_only() || session.saved.running || session.saved.workspace.read_only {
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
                session.review_open && session.review_focus.contains_focused(window, cx)
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
        if self.ai_result_preview || self.split_parent.is_some() {
            return;
        }
        let visible = self
            .ai_session
            .as_ref()
            .is_some_and(|session| session.visible || session.review_open);
        if self.ai_surface_visible.replace(visible) != visible {
            for group in self.split_groups() {
                group.update(cx, |_, cx| cx.notify());
            }
        }
    }

    pub(in crate::editor) fn ai_interaction_has_focus(&self, window: &Window, cx: &App) -> bool {
        self.ai_session.as_ref().is_some_and(|session| {
            (session.review_open && session.review_focus.contains_focused(window, cx))
                || (session.visible
                    && (session.panel_focus.contains_focused(window, cx)
                        || session.input.read(cx).focus_handle(cx).is_focused(window)
                        || session
                            .instruction_body
                            .read(cx)
                            .focus_handle(cx)
                            .is_focused(window)
                        || [
                            &session.language,
                            &session.instruction_name,
                            &session.context_input,
                        ]
                        .iter()
                        .any(|field| field.read(cx).focus_handle(cx).is_focused(window))))
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

    pub(super) fn select_ai_review_document(&mut self, id: &str, cx: &mut Context<Self>) {
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
        let session_id = session.saved.id.clone();
        let proposal = proposal.clone();
        let path = session
            .saved
            .target
            .as_ref()
            .filter(|target| target.document_id == id)
            .and_then(|target| target.path.clone())
            .or_else(|| {
                // untitled:/external: 前缀不是相对路径，拼进根目录只会得到
                // 不存在的伪路径，反而干扰投影内相对资源解析。
                (session.saved.workspace.root.is_some() && !id.contains(':'))
                    .then(|| session.saved.workspace.root.as_ref().unwrap().join(id))
            });
        let previous = session
            .review
            .as_ref()
            .filter(|review| review.id == id)
            .map_or(0, |review| review.index);
        let opening = session.review.is_none();
        let proposed = self.ai_document_preview(proposal.proposed.to_string(), path.clone(), cx);
        let spans = |editor: &mut Editor, cx: &mut Context<Editor>| {
            editor.flush_pending_materialization(cx);
            editor
                .document
                .root_blocks()
                .iter()
                .filter_map(|block| editor.document.source_span_of(block.entity_id()))
                .collect::<Vec<_>>()
        };
        // 基准版没变时复用上一份块边界：每个 checkpoint 都重建原文编辑器，
        // 大文档生成期间会反复整帧卡顿。
        let (before_blocks, base_version) = match session
            .review
            .as_ref()
            .filter(|review| review.id == id && review.base_version == versions.0)
        {
            Some(review) => (review.before_blocks.clone(), review.base_version),
            None => {
                let original =
                    self.ai_document_preview(proposal.base.source.to_string(), path.clone(), cx);
                (original.update(cx, spans), versions.0)
            }
        };
        let after_blocks = proposed.update(cx, spans);
        let edits = proposal.edits();
        let projected = super::inline_review::tracked_document(
            &proposal.base.source,
            &before_blocks,
            &proposal.proposed,
            &after_blocks,
            &edits,
        );
        let (source, marks) = match projected {
            Ok(projected) => projected,
            Err(error) => {
                // 不清脏标会在渲染入口每帧重试、每帧重弹模态。
                if let Some(session) = self.ai_session.as_mut() {
                    session.review_dirty = false;
                    session.error = Some(error.to_string());
                }
                cx.notify();
                return;
            }
        };
        let tracked = self.ai_document_preview(source, path, cx);
        let index = previous.min(edits.len().saturating_sub(1));
        let binding = ReviewBinding {
            session_id,
            owner: cx.entity().downgrade(),
            document_id: id.to_owned(),
            versions,
            index,
            enabled: !self
                .ai_session
                .as_ref()
                .is_some_and(|session| session.saved.running),
            can_accept: self.ai_review_can_accept(Some(id), true),
        };
        tracked.update(cx, |editor, _| {
            editor.ai_review_marks = marks;
            editor.ai_review_binding = Some(binding);
        });
        let review = ReviewDocument {
            id: id.to_owned(),
            proposed,
            tracked,
            index,
            edits,
            versions,
            before_blocks,
            base_version,
        };
        review.focus_change(cx);
        if let Some(session) = self.ai_session.as_mut() {
            session.review = Some(review);
            session.review_dirty = false;
            if opening {
                session.review_open = true;
            }
        }
        cx.notify();
    }

    pub(in crate::editor) fn sync_ai_review_document(&mut self, cx: &mut Context<Self>) {
        let origin = self.ai_review_navigation_origin(cx);
        let Some(session) = self.ai_session.as_ref() else {
            return;
        };
        if let Some(review) = &session.review {
            let enabled = self.ai_review_can_accept(Some(&review.id), true);
            review.tracked.update(cx, |editor, cx| {
                let ready = !session.saved.running;
                if let Some(binding) = editor.ai_review_binding.as_mut()
                    && (binding.can_accept != enabled
                        || binding.index != review.index
                        || binding.enabled != ready)
                {
                    binding.can_accept = enabled;
                    binding.enabled = ready;
                    binding.index = review.index;
                    cx.notify();
                }
            });
        }
        if !session.review_dirty {
            self.sync_ai_review_navigation(cx);
            return;
        }
        if !session.review_open && !session.visible && origin != session.document {
            // 任务的文档不在屏上（切走标签/面板收起）时不重建投影：脏标记
            // 留着，回到该文档或面板可见时再构建。
            self.sync_ai_review_navigation(cx);
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
                    .target
                    .as_ref()
                    .map(|target| target.document_id.as_str())
                    .filter(|id| session.saved.workspace.changes.proposals.contains_key(*id))
            })
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
            session.review_open = false;
        }
        self.sync_ai_review_navigation(cx);
    }

    pub(super) fn move_ai_review_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        if let Some(review) = self
            .ai_session
            .as_mut()
            .and_then(|session| session.review.as_mut())
            && !review.edits.is_empty()
        {
            review.index = if forward {
                (review.index + 1) % review.edits.len()
            } else {
                (review.index + review.edits.len() - 1) % review.edits.len()
            };
            review.focus_change(cx);
        }
        cx.notify();
    }

    pub(in crate::editor) fn render_ai_workspace(
        &self,
        document: AnyElement,
        _theme: &crate::theme::Theme,
        _window: &Window,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self
            .ai_session
            .as_ref()
            .filter(|session| session.review_open)
        else {
            return document;
        };
        let Some(review) = &session.review else {
            return document;
        };
        div()
            .id("ai-review-workspace")
            .debug_selector(|| "ai-review-workspace".into())
            .track_focus(&session.review_focus)
            .size_full()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                div()
                    .id("ai-review-document")
                    .debug_selector(|| "ai-review-document".into())
                    .size_full()
                    .child(
                        div()
                            .debug_selector({
                                let id = review.id.clone();
                                move || format!("ai-review-active-{id}")
                            })
                            .size_full()
                            .child(review.tracked.clone()),
                    ),
            )
            .into_any_element()
    }

    pub(in crate::editor) fn render_ai_review_row(
        &self,
        element: AnyElement,
        visible: std::ops::Range<usize>,
        width: f32,
        theme: &crate::theme::Theme,
        cx: &App,
    ) -> AnyElement {
        if self.ai_review_marks.is_empty() {
            return element;
        }
        let next_start = self
            .document
            .visible_blocks()
            .get(visible.end)
            .and_then(|block| self.document.source_span_of(block.entity.entity_id()))
            .map(|span| span.start);
        let Some(blocks) = self.document.visible_blocks().get(visible) else {
            return element;
        };
        let spans = blocks
            .iter()
            .filter_map(|block| self.document.source_span_of(block.entity.entity_id()))
            .collect::<Vec<_>>();
        let Some(mark) = self.ai_review_marks.iter().find(|mark| {
            spans
                .iter()
                .any(|span| span.start < mark.range.end && mark.range.start < span.end)
        }) else {
            return element;
        };
        let color = if mark.kind == MarkKind::Removed {
            theme.colors.callout_caution_border
        } else {
            theme.colors.dialog_primary_button_bg
        };
        let mut row = div()
            .relative()
            .w(px(width))
            .max_w(relative(1.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(Hsla { a: 0.08, ..color })
            .child(element)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(-8.0))
                    .w(px(2.0))
                    .bg(color),
            );
        if let Some(binding) = &self.ai_review_binding {
            let preferred = self
                .ai_review_marks
                .iter()
                .filter(|mark| mark.edits.contains(&binding.index))
                .find(|mark| mark.kind == MarkKind::Added)
                .or_else(|| {
                    self.ai_review_marks
                        .iter()
                        .find(|mark| mark.edits.contains(&binding.index))
                });
            if preferred.is_some_and(|preferred| std::ptr::eq(mark, preferred))
                && next_start.is_none_or(|start| start >= mark.range.end)
            {
                row = row.child(self.render_ai_inline_controls(binding, theme, cx));
            }
        }
        row.into_any_element()
    }

    fn render_ai_inline_controls(
        &self,
        binding: &ReviewBinding,
        _theme: &crate::theme::Theme,
        cx: &App,
    ) -> AnyElement {
        let strings = cx.global::<crate::i18n::I18nManager>().strings_arc();
        let mut controls = div()
            .id("ai-inline-controls")
            .debug_selector(|| "ai-inline-controls".into())
            .w_full()
            .h(px(32.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(4.0))
            .px(px(8.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for (accept, id, label) in [
            (false, "ai-review-reject", strings.ai_panel_reject.clone()),
            (
                true,
                "ai-review-accept",
                strings.ai_panel_accept_edit.clone(),
            ),
        ] {
            let binding = binding.clone();
            let enabled = if accept {
                binding.can_accept
            } else {
                binding.enabled
            };
            let button = Button::new(id)
                .debug_selector(move || id.into())
                .small()
                .ghost()
                .when(accept, |button| button.primary())
                .label(label)
                .disabled(!enabled)
                .on_click(move |_, window, cx| {
                    if !enabled {
                        return;
                    }
                    let binding = binding.clone();
                    let window = window.window_handle();
                    cx.defer(move |cx| {
                        match window.update(cx, |_, window, cx| {
                            binding.owner.update(cx, |editor, cx| {
                                let current = editor
                                    .ai_session
                                    .as_ref()
                                    .filter(|session| session.saved.id == binding.session_id)
                                    .and_then(|session| session.review.as_ref())
                                    .is_some_and(|review| {
                                        review.id == binding.document_id
                                            && review.versions == binding.versions
                                    });
                                if !current {
                                    return;
                                }
                                if accept {
                                    editor.apply_ai_review(
                                        Some(binding.document_id.clone()),
                                        Some(binding.index),
                                        window,
                                        cx,
                                    );
                                } else {
                                    editor.reject_ai_review(
                                        &binding.document_id,
                                        Some(binding.index),
                                        cx,
                                    );
                                }
                            })
                        }) {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) | Err(error) => {
                                eprintln!("处理行内 AI 修改失败：{error}")
                            }
                        }
                    });
                });
            controls = controls.child(button);
        }
        controls.into_any_element()
    }
}
