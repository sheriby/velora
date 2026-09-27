//! Shared UI components and Markdown editing primitives.

/// 单块源码超过该字节长度时，渲染态降级为纯源码文本（roadmap B12）。
pub(crate) const LONG_BLOCK_SOURCE_LIMIT: usize = 20_000;

mod actions;
mod block;
pub(crate) mod latex;
pub(crate) mod markdown;
pub(crate) mod mermaid;
pub(crate) mod switch;

pub use crate::editor::Editor;
#[allow(unused_imports)]
pub(crate) use crate::editor::InfoDialogKind;
pub use actions::*;
pub use block::*;
#[allow(unused_imports)]
pub(crate) use latex::*;
#[allow(unused_imports)]
pub(crate) use markdown::code_highlight::*;
#[allow(unused_imports)]
pub(crate) use markdown::footnote::*;
#[allow(unused_imports)]
pub(crate) use markdown::html::*;
#[allow(unused_imports)]
pub(crate) use markdown::image::*;
#[allow(unused_imports)]
pub use markdown::inline::*;
#[allow(unused_imports)]
pub(crate) use markdown::link::*;
pub use markdown::table::*;
#[allow(unused_imports)]
pub(crate) use mermaid::*;
