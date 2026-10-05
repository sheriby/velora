//! AI 助手的服务端接入层。
//!
//! 分三层,边界都为「换掉一层不惊动其余两层」而画:
//!
//! 1. endpoint.rs —— 端点与协议元数据(ProviderKind,AiEndpointConfig)。
//! 2. prompts.rs —— 协议无关的提示词构建(动作枚举 + 输出契约 + 上下文标签)。
//! 3. transport/ —— 各协议的请求与流解析(chat-completions / responses /
//!    messages),stub.rs 是内置演示后端(无网络,按动作回放剧本)。
//!
//! 本模块不依赖 gpui:阻塞式读流由调用方放到工作线程,增量经回调送出,
//! 编辑器侧经 channel 泵回 UI(见 editor/ai_assistant.rs)。新增协议的
//! 固定动作见 transport/mod.rs 的模块文档。

mod endpoint;
mod prompts;
mod sse;
mod stub;
mod transport;

pub(crate) use endpoint::*;
pub(crate) use prompts::*;
// sse/stub 的类型经由 transport 的公共接口使用,这里只给测试与后续模块留门。
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use sse::*;
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use stub::*;
pub(crate) use transport::*;
