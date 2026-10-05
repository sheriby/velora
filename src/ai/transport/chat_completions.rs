//! OpenAI 兼容 Chat Completions 传输。
//!
//! 覆盖 OpenAI、DeepSeek、智谱 GLM、Moonshot Kimi、OpenRouter 以及本地
//! Ollama/LM Studio——凡实现 `POST {base}/chat/completions` + SSE 的服务。

use std::sync::atomic::AtomicBool;

use super::{read_plain_json, read_sse, send_post, AiRequestError, SseOutcome};
use crate::ai::endpoint::AiEndpointConfig;
use crate::ai::prompts::AiPrompt;

/// 把 base_url 规范成 chat/completions 的完整地址:
/// `https://host/v1` → `https://host/v1/chat/completions`;用户把完整地址
/// 填进来时不再追加,避免出现双重路径。
pub(crate) fn chat_completions_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.ends_with("/chat/completions") {
        return trimmed.to_string();
    }
    format!("{trimmed}/chat/completions")
}

/// 请求体:OpenAI 兼容的流式 chat completion。
fn request_body(model: &str, prompt: &AiPrompt) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": prompt.system},
            {"role": "user", "content": prompt.user},
        ],
        "stream": true,
    })
}

/// 从一条 SSE `data` 载荷提取增量文本;`[DONE]`、心跳、角色帧等非内容载荷
/// 返回 `None`。
pub(crate) fn delta_from_sse_payload(payload: &str) -> Option<String> {
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    let value = serde_json::from_str::<serde_json::Value>(payload).ok()?;
    let delta = value.get("choices")?.get(0)?.get("delta")?.clone();
    delta
        .get("content")?
        .as_str()
        .map(str::to_string)
        .filter(|content| !content.is_empty())
}

/// 非流式响应体兜底:个别代理会无视 `stream: true` 直接回一份完整 JSON,
/// 这时把 message.content 整段当一个增量返回。
fn content_from_completion_json(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let choice = value.get("choices")?.get(0)?;
    choice
        .get("message")?
        .get("content")?
        .as_str()
        .map(str::to_string)
}

pub(crate) fn stream(
    client: &reqwest::blocking::Client,
    endpoint: &AiEndpointConfig,
    prompt: &AiPrompt,
    on_delta: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<String, AiRequestError> {
    let url = chat_completions_url(&endpoint.base_url);
    let body = serde_json::to_string(&request_body(&endpoint.model, prompt))
        .map_err(|error| AiRequestError::Protocol(error.to_string()))?;
    let response = send_post(
        client,
        &url,
        &[
            ("Authorization", format!("Bearer {}", endpoint.api_key)),
            ("Accept", "text/event-stream".to_string()),
        ],
        body,
        cancel,
    )?;

    let is_event_stream = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"));
    if !is_event_stream {
        return read_plain_json(response, cancel, &content_from_completion_json, on_delta);
    }

    read_sse(response, cancel, &|payload| match delta_from_sse_payload(payload) {
        Some(delta) => SseOutcome::Delta(delta),
        None if payload.trim() == "[DONE]" => SseOutcome::Done,
        None => SseOutcome::Ignore,
    }, on_delta)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use super::*;
    use crate::ai::prompts::StubScenario;
    use crate::ai::transport::test_support::*;

    fn endpoint_at(port: u16) -> AiEndpointConfig {
        AiEndpointConfig {
            kind: crate::ai::endpoint::ProviderKind::ChatCompletions,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "test-key".into(),
            model: "test-model".into(),
        }
    }

    fn prompt() -> AiPrompt {
        AiPrompt {
            system: "sys".into(),
            user: "hi".into(),
            stub_scenario: StubScenario::Custom,
        }
    }

    #[test]
    fn chat_completions_url_appends_path_once() {
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("  http://localhost:11434/v1 "),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn request_body_carries_system_user_and_stream_flag() {
        let body = request_body("glm-4-flash", &prompt());
        assert_eq!(body["model"], "glm-4-flash");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "sys");
        assert_eq!(body["messages"][1]["content"], "hi");
    }

    #[test]
    fn delta_extraction_accepts_content_frames_only() {
        assert_eq!(
            delta_from_sse_payload(r#"{"choices":[{"delta":{"content":"你好"}}]}"#),
            Some("你好".to_string())
        );
        // 角色帧:content 为 null,不是增量。
        assert_eq!(
            delta_from_sse_payload(r#"{"choices":[{"delta":{"role":"assistant"}}]}"#),
            None
        );
        assert_eq!(delta_from_sse_payload("[DONE]"), None);
        assert_eq!(delta_from_sse_payload("not json"), None);
        assert_eq!(
            delta_from_sse_payload(r#"{"choices":[{"delta":{"content":""}}]}"#),
            None
        );
    }

    #[test]
    fn error_message_prefers_the_provider_message() {
        assert_eq!(
            super::super::message_from_error_body(r#"{"error":{"message":"bad key","type":"auth"}}"#),
            "bad key"
        );
        assert_eq!(
            super::super::message_from_error_body("plain text"),
            "plain text"
        );
    }

    #[test]
    fn streams_deltas_from_a_live_sse_response() {
        let port = spawn_server(|stream| {
            let head = read_request_head(stream);
            eprintln!("PROBE HEAD: {head}");
            assert!(head.contains("authorization: Bearer test-key"));
            write_sse_response(
                stream,
                &[
                    r#"{"choices":[{"delta":{"content":"你"}}]}"#,
                    r#"{"choices":[{"delta":{"content":"好"}}]}"#,
                    "[DONE]",
                ]
                .iter()
                .map(|event| event.to_string())
                .collect::<Vec<_>>(),
            );
        });

        let mut deltas = Vec::new();
        let full = stream(
            &test_client(),
            &endpoint_at(port),
            &prompt(),
            &mut |delta| deltas.push(delta.to_string()),
            &AtomicBool::new(false),
        )
        .expect("stream succeeds");
        assert_eq!(full, "你好");
        assert_eq!(deltas, vec!["你".to_string(), "好".into()]);
    }

    #[test]
    fn non_stream_json_response_is_accepted_as_a_single_delta() {
        let port = spawn_server(|stream| {
            write_json_response(
                stream,
                "HTTP/1.1 200 OK",
                r#"{"choices":[{"message":{"content":"完整回答"}}]}"#,
            );
        });

        let mut deltas = Vec::new();
        let full = stream(
            &test_client(),
            &endpoint_at(port),
            &prompt(),
            &mut |delta| deltas.push(delta.to_string()),
            &AtomicBool::new(false),
        )
        .expect("ok");
        assert_eq!(full, "完整回答");
        assert_eq!(deltas, vec!["完整回答".to_string()]);
    }

    #[test]
    fn http_error_carries_status_and_provider_message() {
        let port = spawn_server(|stream| {
            write_json_response(
                stream,
                "HTTP/1.1 401 Unauthorized",
                r#"{"error":{"message":"Invalid API key"}}"#,
            );
        });

        let result = stream(
            &test_client(),
            &endpoint_at(port),
            &prompt(),
            &mut |_| {},
            &AtomicBool::new(false),
        );
        match result {
            Err(AiRequestError::Http { status, message }) => {
                assert_eq!(status, 401);
                assert_eq!(message, "Invalid API key");
            }
            other => panic!("expected http error, got {other:?}"),
        }
    }

    #[test]
    fn cancel_flag_stops_the_stream() {
        // 服务器只回头部然后挂住:取消必须不等正文就返回。
        let port = spawn_server(|stream| {
            use std::io::Write;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .expect("write head");
            stream.flush().expect("flush");
            std::thread::sleep(std::time::Duration::from_secs(10));
        });

        let cancel = Arc::new(AtomicBool::new(false));
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let result = stream(
            &test_client(),
            &endpoint_at(port),
            &prompt(),
            &mut |_| {},
            &cancel,
        );
        assert_eq!(result, Err(AiRequestError::Cancelled));
    }

    #[test]
    fn empty_base_url_fails_fast_without_network() {
        let endpoint = AiEndpointConfig {
            kind: crate::ai::endpoint::ProviderKind::ChatCompletions,
            base_url: "  ".into(),
            api_key: String::new(),
            model: "m".into(),
        };
        let result = stream(
            &test_client(),
            &endpoint,
            &prompt(),
            &mut |_| {},
            &AtomicBool::new(false),
        );
        assert!(matches!(result, Err(AiRequestError::Protocol(_))));
    }
}
