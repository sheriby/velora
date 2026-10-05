//! AI 助手的服务端接入层。
//!
//! 协议只认 OpenAI 兼容的 Chat Completions(`POST {base}/chat/completions`
//! + SSE 流式),这一族协议同时覆盖 OpenAI、DeepSeek、智谱、Kimi、
//! OpenRouter 以及本地 Ollama/LM Studio,设置页只需要一组「地址/密钥/模型」。
//!
//! 分层与编辑器解耦:本模块不依赖 gpui,阻塞式读流由调用方放到工作线程,
//! 增量经 channel 送回 UI(`editor/ai_assistant.rs` 负责那半边)。

// 接线由编辑器面板与设置页在本系列后续提交完成;在那之前先放行 dead_code,
// 面板落地时一并摘掉。
#![allow(dead_code)]

mod client;
mod sse;

// 转出口在本系列后续提交(编辑器接线/设置页)才会被引用,先挂 allow
// 让中间提交保持零告警。
#[allow(unused_imports)]
pub(crate) use client::*;
#[allow(unused_imports)]
pub(crate) use sse::*;
