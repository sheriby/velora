use super::*;

impl Block {
    pub(crate) fn clear_vertical_motion(&mut self) {
        self.vertical_motion_x = None;
    }

    pub(crate) fn sync_render_cache(&mut self) {
        let clean_selected = self.current_to_clean_range(self.selected_range.clone());
        let clean_marked = self
            .marked_range
            .clone()
            .map(|range| self.current_to_clean_range(range));
        let (clean_anchor, clean_focus) = self.clean_selection_anchor_focus();
        let (anchor_affinity, focus_affinity) = self.selection_endpoint_affinities();
        let collapsed_affinity = self.current_collapsed_caret_affinity();
        let keep_projection =
            self.projection.is_some() && self.edit_mode.supports_inline_projection();
        self.render_cache = self.record.title.render_cache();
        self.sync_code_highlight();
        self.sync_image_runtime();
        self.projection = None;
        self.projection_cache_key = None;
        if keep_projection {
            self.rebuild_inline_projection(clean_selected.clone(), clean_marked.clone());
            if clean_selected.is_empty() {
                let offset = self.clean_to_current_cursor_offset_with_affinity(
                    clean_selected.start,
                    collapsed_affinity,
                );
                self.assign_collapsed_selection_offset(offset, collapsed_affinity, None);
            } else {
                self.set_selection_from_clean_anchor_focus(
                    clean_anchor,
                    clean_focus,
                    anchor_affinity,
                    focus_affinity,
                );
                self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
            }
            self.marked_range = clean_marked.map(|range| self.clean_to_current_range(range));
        } else {
            self.set_selection_from_anchor_focus(clean_anchor, clean_focus);
            self.marked_range = clean_marked;
            self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
        }
        self.refresh_cached_display_text();
    }

    pub(crate) fn sync_link_reference_definitions(
        &mut self,
        link_reference_definitions: Arc<LinkReferenceDefinitions>,
    ) {
        if self.link_reference_definitions == link_reference_definitions {
            return;
        }

        let selected_markdown = (!self.uses_raw_text_editing())
            .then(|| self.current_range_to_markdown_range(self.selected_range.clone()));
        let marked_markdown = (!self.uses_raw_text_editing())
            .then(|| {
                self.marked_range
                    .clone()
                    .map(|range| self.current_range_to_markdown_range(range))
            })
            .flatten();
        let selection_reversed = self.selection_reversed;
        let collapsed_affinity = self.current_collapsed_caret_affinity();
        let had_projection = self.projection.is_some();

        self.link_reference_definitions = link_reference_definitions;
        if self.uses_raw_text_editing() {
            return;
        }

        let markdown = self.record.title.serialize_markdown();
        let next_title = InlineTextTree::from_markdown_with_link_references(
            &markdown,
            &self.link_reference_definitions,
        );
        if self.record.title == next_title {
            return;
        }

        self.record.set_title(next_title);
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();

        if let Some(selected_markdown) = selected_markdown {
            let restored = self.markdown_range_to_current_range(selected_markdown);
            if restored.is_empty() {
                self.assign_collapsed_selection_offset(
                    restored.start,
                    collapsed_affinity,
                    self.vertical_motion_x,
                );
            } else {
                self.selected_range = restored;
                self.selection_reversed = selection_reversed;
                self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
            }
        }

        self.marked_range =
            marked_markdown.map(|range| self.markdown_range_to_current_range(range));

        if had_projection {
            self.sync_inline_projection_for_focus(true);
        }
    }

    pub(crate) fn sync_footnote_registry(&mut self, footnote_registry: Arc<FootnoteRegistry>) {
        // 注册表没换人也要回填：编辑后的重解析按源码形状存片段（`[^1]`，序号留空），
        // 而段首打一个字不会换注册表。不在这里贴回序号，屏幕上就是 `[^1]`，
        // 可见长度还多出 3 个字节，光标与字数都跟着错。
        if self.footnote_registry == footnote_registry
            && !self.record.title.has_unresolved_footnote_references()
        {
            return;
        }

        let selected_markdown = (!self.uses_raw_text_editing())
            .then(|| self.current_range_to_markdown_range(self.selected_range.clone()));
        let marked_markdown = (!self.uses_raw_text_editing())
            .then(|| {
                self.marked_range
                    .clone()
                    .map(|range| self.current_range_to_markdown_range(range))
            })
            .flatten();
        let selection_reversed = self.selection_reversed;
        let collapsed_affinity = self.current_collapsed_caret_affinity();
        let had_projection = self.projection.is_some();

        self.footnote_registry = footnote_registry;
        if self.uses_raw_text_editing() || !self.record.title.has_footnote_references() {
            return;
        }

        let mut next_title = self.record.title.clone();
        let mut occurrence_iter = self
            .footnote_registry
            .occurrences_for_block(self.record.id)
            .unwrap_or(&[])
            .iter();
        next_title.apply_footnote_reference_state(|id| {
            let occurrence = occurrence_iter.next()?;
            if occurrence.id != id {
                return None;
            }
            Some((occurrence.ordinal?, occurrence.occurrence_index))
        });
        if self.record.title == next_title {
            return;
        }

        self.record.set_title(next_title);
        self.sync_edit_mode_from_kind();
        self.sync_render_cache();

        if let Some(selected_markdown) = selected_markdown {
            let restored = self.markdown_range_to_current_range(selected_markdown);
            if restored.is_empty() {
                self.assign_collapsed_selection_offset(
                    restored.start,
                    collapsed_affinity,
                    self.vertical_motion_x,
                );
            } else {
                self.selected_range = restored;
                self.selection_reversed = selection_reversed;
                self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
            }
        }

        self.marked_range =
            marked_markdown.map(|range| self.markdown_range_to_current_range(range));

        if had_projection {
            self.sync_inline_projection_for_focus(true);
        }
    }

    pub(crate) fn should_use_markdown_space_link_edit(&self) -> bool {
        !self.uses_raw_text_editing() && self.record.title.has_source_preserving_links()
    }

    pub(crate) fn apply_markdown_space_title_edit(
        &mut self,
        visible_range: Range<usize>,
        new_text: &str,
        selected_range_relative: Option<Range<usize>>,
        mark_inserted_text: bool,
        cx: &mut Context<Self>,
    ) {
        let old_visible_len = self.record.title.visible_text().len();
        let markdown_range = self.current_range_to_markdown_range(visible_range.clone());
        let mut markdown = self.record.title.serialize_markdown();
        let replaced_text = markdown[markdown_range.clone()].to_string();
        // 键入的文本按可见字符拼进 markdown，反斜杠要再转义一次：否则它会与
        // `serialize_markdown` 重新转义出来的旧反斜杠叠加，每按一次数量翻倍
        // （用户报修：行首是自动链接的块里按反斜杠，可见文本 1→3→7）。
        let inserted_markdown = escape_markdown_insertion(new_text);
        let inserted_markdown_len = inserted_markdown.len();
        markdown.replace_range(markdown_range.clone(), &inserted_markdown);

        let next_title = InlineTextTree::from_markdown_with_link_references(
            &markdown,
            &self.link_reference_definitions,
        );
        let map = next_title.markdown_offset_map();
        let selected_markdown = selected_range_relative.as_ref().map(|relative| {
            let start =
                markdown_range.start + markdown_insertion_offset(new_text, relative.start);
            let end = markdown_range.start + markdown_insertion_offset(new_text, relative.end);
            start..end
        });
        let cursor_markdown = selected_markdown
            .as_ref()
            .map(|range| range.end)
            .unwrap_or(markdown_range.start + inserted_markdown_len);
        let marked_markdown = if mark_inserted_text && !new_text.is_empty() {
            Some(markdown_range.start..markdown_range.start + inserted_markdown_len)
        } else {
            None
        };
        let selected_clean = selected_markdown
            .as_ref()
            .map(|range| map.markdown_to_visible_range(range.clone()));
        let marked_clean = marked_markdown
            .as_ref()
            .map(|range| map.markdown_to_visible_range(range.clone()));
        let cursor_clean = map.markdown_to_visible_offset(cursor_markdown);

        let quote_structure_edit = self.quote_depth > 0
            && (new_text.contains('\n')
                || replaced_text.contains('\n')
                || (self.kind() == BlockKind::Quote
                    && Self::multiline_quote_edit_requires_reparse(&next_title.visible_text())));
        if quote_structure_edit {
            self.quote_reparse_requested = true;
        }

        // 吸收记号只代表形成了样式，是否跨过结束记号要按输入后的源码落点判断。
        let span_markers_absorbed = !new_text.is_empty()
            && !mark_inserted_text
            && next_title.visible_text().len() < old_visible_len + new_text.len();

        let new_span_caret_affinity = span_markers_absorbed.then(|| {
            if cursor_markdown < map.markdown().len()
                && map.markdown_to_visible_offset(cursor_markdown + 1) == cursor_clean
            {
                CollapsedCaretAffinity::Default
            } else {
                CollapsedCaretAffinity::OuterEnd
            }
        });

        self.apply_title_edit(
            next_title,
            cursor_clean,
            marked_clean,
            selected_clean.clone(),
            selected_clean
                .as_ref()
                .and_then(|range| (!range.is_empty()).then_some(false)),
            new_span_caret_affinity,
            cx,
        );
    }

    pub(crate) fn current_cache(&self) -> &InlineRenderCache {
        self.projection
            .as_ref()
            .map(|projection| &projection.cache)
            .unwrap_or(&self.render_cache)
    }

    pub(crate) fn sync_inline_projection_for_focus(&mut self, focused: bool) {
        let supports_projection = self.edit_mode.supports_inline_projection();
        if !focused || !supports_projection {
            self.clear_inline_projection();
            return;
        }

        let had_projection = self.projection.is_some();
        let projected_link_selection = self.projection.as_ref().and_then(|projection| {
            projection
                .link_run_fully_covering_range(&self.selected_range)
                .map(|run| ProjectedLinkSelectionSnapshot {
                    clean_range: run.clean_range.clone(),
                    display_relative_range: self
                        .selected_range
                        .start
                        .saturating_sub(run.display_range.start)
                        ..self
                            .selected_range
                            .end
                            .saturating_sub(run.display_range.start),
                    selection_reversed: self.selection_reversed,
                })
        });
        let clean_selected = self.current_to_clean_range(self.selected_range.clone());
        let clean_marked = self
            .marked_range
            .clone()
            .map(|range| self.current_to_clean_range(range));
        if self.projection_cache_key.as_ref()
            == Some(&(
                supports_projection,
                clean_selected.clone(),
                clean_marked.clone(),
            ))
        {
            return;
        }
        let (clean_anchor, clean_focus) = self.clean_selection_anchor_focus();
        let (anchor_affinity, focus_affinity) = self.selection_endpoint_affinities();
        let collapsed_affinity = self.current_collapsed_caret_affinity();
        self.rebuild_inline_projection(clean_selected.clone(), clean_marked.clone());
        if let Some(snapshot) = projected_link_selection
            && let Some(run) = self
                .projection
                .as_ref()
                .and_then(|projection| projection.link_run_for_clean_range(&snapshot.clean_range))
        {
            let start = run.display_range.start
                + snapshot
                    .display_relative_range
                    .start
                    .min(run.display_range.len());
            let end = run.display_range.start
                + snapshot
                    .display_relative_range
                    .end
                    .min(run.display_range.len());
            self.selected_range = start..end;
            self.selection_reversed = snapshot.selection_reversed;
            self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
        } else if clean_selected.is_empty() {
            // 已显形的记号内外位置来自上一帧，不能按块尾重新猜成记号外侧。
            let collapsed_affinity = if had_projection {
                collapsed_affinity
            } else {
                self.caret_affinity_for_clean_offset(clean_selected.start, collapsed_affinity)
            };
            let offset = self.clean_to_current_cursor_offset_with_affinity(
                clean_selected.start,
                collapsed_affinity,
            );
            self.assign_collapsed_selection_offset(offset, collapsed_affinity, None);
        } else {
            self.set_selection_from_clean_anchor_focus(
                clean_anchor,
                clean_focus,
                anchor_affinity,
                focus_affinity,
            );
            self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
        }
        self.marked_range = clean_marked.map(|range| self.clean_to_current_range(range));
    }

    pub(crate) fn clear_inline_projection(&mut self) {
        if self.projection.is_none() {
            self.projection_cache_key = None;
            return;
        }

        let clean_marked = self
            .marked_range
            .clone()
            .map(|range| self.current_to_clean_range(range));
        let (clean_anchor, clean_focus) = self.clean_selection_anchor_focus();
        self.projection = None;
        self.projection_cache_key = None;
        self.set_selection_from_anchor_focus(clean_anchor, clean_focus);
        self.marked_range = clean_marked;
        self.collapsed_caret_affinity = CollapsedCaretAffinity::Default;
        self.refresh_cached_display_text();
    }

    pub(crate) fn rebuild_inline_projection(
        &mut self,
        clean_selected: Range<usize>,
        clean_marked: Option<Range<usize>>,
    ) {
        self.projection_cache_key = Some((
            self.edit_mode.supports_inline_projection(),
            clean_selected.clone(),
            clean_marked.clone(),
        ));
        self.projection = ExpandedInlineProjection::build(
            &self.record.title.fragments,
            clean_selected,
            clean_marked,
        );
        self.refresh_cached_display_text();
    }

    pub(crate) fn projection_segments(&self) -> &[ExpandedInlineSegment] {
        self.projection
            .as_ref()
            .map(|projection| projection.segments.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn projected_link_run_fully_covering_range(
        &self,
        range: &Range<usize>,
    ) -> Option<&ExpandedLinkRun> {
        self.projection
            .as_ref()
            .and_then(|projection| projection.link_run_fully_covering_range(range))
    }

    pub(crate) fn collapsed_caret_affinity_for_display_offset(&self, offset: usize) -> CollapsedCaretAffinity {
        self.projection
            .as_ref()
            .map(|projection| projection.collapsed_affinity_for_display_offset(offset))
            .unwrap_or(CollapsedCaretAffinity::Default)
    }

    /// Affinity of the current selection's anchor and focus, used to restore
    /// each endpoint accurately when the projection is rebuilt.
    pub(crate) fn selection_endpoint_affinities(&self) -> (CollapsedCaretAffinity, CollapsedCaretAffinity) {
        let (anchor, focus) = self.selection_anchor_focus();
        (
            self.collapsed_caret_affinity_for_display_offset(anchor),
            self.collapsed_caret_affinity_for_display_offset(focus),
        )
    }

    pub(crate) fn current_collapsed_caret_affinity(&self) -> CollapsedCaretAffinity {
        if !self.selected_range.is_empty() {
            return CollapsedCaretAffinity::Default;
        }

        self.projection
            .as_ref()
            .map(|projection| {
                projection.collapsed_affinity_for_display_offset(self.cursor_offset())
            })
            .unwrap_or(self.collapsed_caret_affinity)
    }

    pub(crate) fn sync_collapsed_caret_affinity(&mut self) {
        self.collapsed_caret_affinity = if self.selected_range.is_empty() {
            self.projection
                .as_ref()
                .map(|projection| {
                    projection.collapsed_affinity_for_display_offset(self.cursor_offset())
                })
                .unwrap_or(CollapsedCaretAffinity::Default)
        } else {
            CollapsedCaretAffinity::Default
        };
    }

    pub(crate) fn assign_collapsed_selection_offset(
        &mut self,
        offset: usize,
        affinity: CollapsedCaretAffinity,
        preferred_x: Option<Pixels>,
    ) {
        let clamped_offset = offset.min(self.visible_len());
        self.selected_range = clamped_offset..clamped_offset;
        self.selection_reversed = false;
        self.vertical_motion_x = preferred_x;
        self.collapsed_caret_affinity = affinity;
        self.sync_collapsed_caret_affinity();
    }

}
