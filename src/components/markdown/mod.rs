//! Markdown syntax models and parse/serialize helpers shared by editor blocks.

pub(crate) mod code_highlight;
pub(crate) mod footnote;
pub(crate) mod frontmatter;
pub(crate) mod html;
/// Clipboard HTML conversion is wired up on macOS only; other targets compile
/// it for the tests inside the module.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod html_paste;
pub(crate) mod image;
pub mod inline;
pub(crate) mod link;
pub(crate) mod paste;
pub mod table;
