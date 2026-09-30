//! Markdown-to-editor-tree deserialization.
//! 测试夹具放在 velora 的 tests/fixtures 目录。
//!
//! Raw Markdown is parsed into the subset of native block structures Velora
//! can edit safely. Syntax that exceeds the current runtime model is preserved
//! as raw Markdown blocks so it can round-trip without loss.

pub(super) use gpui::*;

pub(super) use super::Editor;
pub(super) use crate::components::{
    BlockKind, BlockRecord, CalloutVariant, CodeFenceOpening, InlineTextTree,
    parse_footnote_definition_head,
};
pub(super) use crate::components::{
    HtmlSafetyClass, is_block_level_html_tag, is_html_container_tag, is_raw_text_html_tag,
    parse_html_document,
};
pub(super) use crate::components::{
    collect_pipeless_table_region, collect_root_table_candidate_region,
    collect_table_candidate_region, is_root_table_candidate_line, is_table_candidate_line,
    parse_root_table_region, parse_standalone_image, parse_table_region,
};
pub(super) use crate::components::{is_mermaid_info_string, parse_display_math_source};

/// Parsed opening code-fence metadata.
///
/// The opening fence records both the marker character and its run length so
/// only a matching closing fence can terminate the block.
type FenceInfo = CodeFenceOpening;

/// Resumption state for building one run of root blocks from a line array.
///
/// A huge document is imported in chunks so the editor can show and scroll the
/// first blocks while the rest streams in (roadmap G8). Each call to
/// [`Editor::build_root_block_chunk`] gets one cursor; the returned line index
/// tells the next call where to resume.
#[derive(Clone, Copy)]
pub(crate) struct ChunkCursor {
    /// Soft bound on roots built by this call. `usize::MAX` builds everything.
    pub(super) root_budget: usize,
    /// Whether the line slice begins at the top of the document. Frontmatter
    /// detection and the leading blank run are document-start only.
    pub(super) is_document_start: bool,
    /// Whether the previous chunk ended on a list item, which decides the
    /// list-group blank rule for a blank run at the start of this chunk.
    pub(super) previous_root_is_list_item: bool,
}

impl ChunkCursor {
    /// Builds the whole line slice in one call.
    pub(super) const WHOLE_DOCUMENT: Self = Self {
        root_budget: usize::MAX,
        is_document_start: true,
        previous_root_is_list_item: false,
    };
}

/// HTML block form recognized by the Markdown importer.
pub(crate) enum HtmlBlockStart {
    /// HTML comment region beginning with `<!--`.
    Comment,
    /// HTML tag block whose closing behavior depends on the tag shape.
    Tag {
        name: String,
        self_closing: bool,
        closes_same_line: bool,
    },
}

/// Ordered-list or unordered-list marker parsed from one source line.

pub(super) use parse::*;

mod blocks;
mod import;
mod parse;

#[cfg(test)]
mod tests;
