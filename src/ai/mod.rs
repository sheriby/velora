//! AI 助手的服务端接入层。
//!
//! 协议只认 OpenAI 兼容的 Chat Completions(POST `{base}/chat/completions` + SSE 流式)。
//! 这一族协议同时覆盖 OpenAI、DeepSeek、智谱 GLM、Moonshot Kimi、OpenRouter 以及本地的
//! Ollama/LM Studio,设置页只需要一组「地址/密钥/模型」三元组。
//!
//! 分层与编辑器解耦:本模块不依赖 gpui,阻塞式读流由调用方放到工作线程,
//! 增量经 channel 送回 UI(见 editor/ai_assistant.rs)。


// 编辑器面板/设置页在本系列后续提交接线;在那之前先放行 dead_code,
// 面板落地时一并摘掉。
#![allow(dead_code)]

mod client;
mod prompts;
mod sse;

// 转出口在编辑器面板/设置页接线后才被引用,allow 摘除时机与上面的
// dead_code 相同(面板落地提交)。
#[allow(unused_imports)]
pub(crate) use client::*;
#[allow(unused_imports)]
pub(crate) use prompts::*;
#[allow(unused_imports)]
pub(crate) use sse::*;
