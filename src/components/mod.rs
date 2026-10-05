//! Shared UI components and Markdown editing primitives.

/// 单块源码超过该字节长度时，渲染态降级为纯源码文本（roadmap B12）。
pub(crate) const LONG_BLOCK_SOURCE_LIMIT: usize = 20_000;

/// 带行号的代码/源文件行超过这个字符数就不再换行渲染：折叠成单行裁切
/// 显示，点行号展开按宽换行（不允许横向滚动）。shape/paint 的代价都随
/// 行长线性放大，JSONL 之类每行兆级的文件不做折叠会整窗卡死。
pub(crate) const LONG_LINE_SOURCE_LIMIT: usize = 1024;

/// 折叠态单行最多 shape 的字符数。gpui 的行 paint 逐字形迭代、没有按可见区
/// 早退，兆级行即使不换行每帧也要扫全部字形；超出部分截断并在行尾提示，
/// 点行号展开查看全文。
pub(crate) const LONG_LINE_DISPLAY_CHARS: usize = 10_000;

mod actions;
mod block;
pub(crate) mod latex;
pub(crate) mod markdown;
pub(crate) mod mermaid;
pub(crate) mod switch;
// 下一提交(设置 AI 页)即接入;过渡期放行 dead_code。
#[allow(dead_code)]
mod text_field;

pub use crate::editor::Editor;
// 设置页(AI 配置)在下一提交接入;先放行过渡期 unused。
#[allow(unused_imports)]
pub(crate) use text_field::*;
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
