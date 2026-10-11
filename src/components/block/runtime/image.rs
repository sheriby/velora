//! Rendered standalone image runtime state.

use super::*;

impl Block {
    pub(crate) fn image_runtime(&self) -> Option<&ImageRuntime> {
        self.image_runtime.as_ref()
    }

    pub(super) fn can_present_as_image(&self) -> bool {
        self.is_table_cell()
            || matches!(
                self.kind(),
                BlockKind::Paragraph
                    | BlockKind::BulletedListItem
                    | BlockKind::NumberedListItem
                    | BlockKind::TaskListItem { .. }
            )
    }

    /// Whether this block's text is a lone image that renders as a
    /// self-contained image widget. Unlike `showing_rendered_image`, this is
    /// derived from the title text rather than the computed runtime, so it is
    /// valid before image runtimes are (re)built.
    pub(crate) fn renders_as_standalone_image(&self) -> bool {
        self.can_present_as_image() && self.standalone_image_markdown_for_runtime().is_some()
    }

    pub(super) fn compute_image_runtime(
        &self,
        base_dir: Option<&Path>,
        syntax: ImageSyntax,
    ) -> Option<ImageRuntime> {
        let resolved_target = syntax.resolve_target(&self.image_reference_definitions)?;
        self.can_present_as_image().then(|| ImageRuntime {
            alt: syntax.alt.clone(),
            src: resolved_target.src.clone(),
            title: resolved_target.title.clone(),
            resolved_source: resolve_image_source(&resolved_target.src, base_dir),
        })
    }

    pub(crate) fn image_runtime_for_syntax(&self, syntax: ImageSyntax) -> Option<ImageRuntime> {
        self.compute_image_runtime(self.image_base_dir.as_deref(), syntax)
    }

    pub(crate) fn image_base_dir(&self) -> Option<&Path> {
        self.image_base_dir.as_deref()
    }

    pub(super) fn sync_image_runtime(&mut self) {
        // 挡板：fragments 里没有 `![` 且当前也不是图片块，永远解不出独立图片，
        // 整体跳过——省掉每次结构变更对全文档每块的 visible_text() 拼接与两轮
        // 解析（评审报告维度四建议 4 前半）。图片片段的文本自带 `![`（与
        // has_mixed_inline_visuals 同一口径）；runtime 还挂着的块（编辑正把
        // `!` 删掉的那几键）必须放行，否则旧 runtime 残留。
        let may_be_image = self.image_runtime.is_some()
            || self
                .record
                .title
                .fragments
                .iter()
                .any(|fragment| fragment.text.contains("!["));
        let markdown = if may_be_image {
            self.can_present_as_image()
                .then(|| self.standalone_image_markdown_for_runtime())
                .flatten()
        } else {
            None
        };
        if may_be_image {
            #[cfg(test)]
            crate::components::block::runtime::image::IMAGE_SYNC_SCANS
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let next_runtime = markdown
            .as_deref()
            .and_then(parse_standalone_image)
            .and_then(|syntax| {
                self.compute_image_runtime(self.image_base_dir.as_deref(), syntax)
            });

        if next_runtime.is_none() {
            self.image_edit_expanded = false;
            self.image_expand_requested = false;
        }
        self.image_runtime = next_runtime;
        // 宽度因子以源码中的 `{width=NN%}` 为准（roadmap C10 v2）。
        self.image_width_factor = markdown
            .as_deref()
            .and_then(standalone_image_width_percent)
            .map(|percent| (percent as f32 / 100.0).clamp(0.2, 1.0))
            .unwrap_or(1.0);
    }

    fn standalone_image_markdown_for_runtime(&self) -> Option<String> {
        let visible = self.record.title.visible_text();
        if parse_standalone_image(&visible).is_some() {
            return Some(visible);
        }

        let serialized = self.record.title.serialize_markdown();
        parse_standalone_image(&serialized)
            .is_some()
            .then_some(serialized)
    }

    pub(crate) fn request_image_edit_expansion(&mut self) {
        if self.image_runtime.is_some() {
            self.image_expand_requested = true;
        }
    }

    pub(super) fn consume_requested_image_edit_expansion(&mut self) -> bool {
        if self.image_runtime.is_some() && self.image_expand_requested && !self.image_edit_expanded
        {
            self.image_expand_requested = false;
            self.image_edit_expanded = true;
            self.clear_inline_projection();
            self.assign_collapsed_selection_offset(
                self.visible_len(),
                CollapsedCaretAffinity::Default,
                None,
            );
            self.cursor_blink_epoch = Instant::now();
            self.clear_vertical_motion();
            return true;
        }

        false
    }

    pub(crate) fn sync_image_focus_state(&mut self, focused: bool) -> bool {
        if self.image_runtime.is_none() {
            if self.image_edit_expanded || self.image_expand_requested {
                self.image_edit_expanded = false;
                self.image_expand_requested = false;
                self.clear_inline_projection();
                return true;
            }
            return false;
        }

        if focused {
            return self.consume_requested_image_edit_expansion();
        }

        if self.image_edit_expanded {
            self.image_edit_expanded = false;
            self.clear_inline_projection();
            return true;
        }

        false
    }

    pub(crate) fn showing_rendered_image(&self) -> bool {
        self.image_runtime.is_some() && !self.is_source_raw_mode() && !self.image_edit_expanded
    }
}

/// 形状探针：进入图片解析路径（visible_text 拼接 + parse）的块数。
#[cfg(test)]
pub(crate) static IMAGE_SYNC_SCANS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
