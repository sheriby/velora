//! Anthropic Messages API 传输。
//!
//! `POST {base}/v1/messages`:鉴权走 `x-api-key` 头(不是 Bearer)、
//! `anthropic-version` 必填、system 提示是顶层字段而非消息数组、
//! 流式增量是 `content_block_delta` 里的 `text_delta`、`message_stop` 终止。
//! 与 OpenAI 系的差异全部收敛在本模块。

use std::sync::atomic::AtomicBool;

use super::{read_plain_json, read_sse, send_post, AiRequestError, SseOutcome};
use crate::ai::endpoint::AiEndpointConfig;
use crate::ai::prompts::AiPrompt;

/// Messages API 的协议版本头(Anthropic 要求必填)。
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// 单次补全的输出上限。Messages API 必填 max_tokens;4096 对编辑器内的
/// 写作任务绰绰有余。
const MAX_TOKENS: u32 = 4096;

/// 把 base_url 规范成 messages 的完整地址:
/// `https://api.anthropic.com` → `…/v1/messages`;
/// 已填 `…/v1` 只补 `/messages`;填了完整路径不重复追加。
pub(crate) fn messages_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.ends_with("/messages") {
        return trimmed.to_string();
    }
    if trimmed.ends_with("/v1") {
        return format!("{trimmed}/messages");
    }
    format!("{trimmed}/v1/messages")
}

fn request_body(model: &str, prompt: &AiPrompt) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": prompt.system,
        "messages": [{"role": "user", "content": prompt.user}],
        "stream": true,
    })
}

/// 增量事件:`content_block_delta` 且 delta 类型为 `text_delta`。
pub(crate) fn delta_from_sse_payload(payload: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(payload.trim()).ok()?;
    if value.get("type")?.as_str()? != "content_block_delta" {
        return None;
    }
    let delta = value.get("delta")?;
    if delta.get("type")?.as_str()? != "text_delta" {
        return None;
    }
    delta
        .get("text")?
        .as_str()
        .map(str::to_string)
        .filter(|text| !text.is_empty())
}

/// 流终止:message_stop 正常结束;error 事件(如 overload)转成失败。
pub(crate) fn is_terminal_payload(payload: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload.trim()) else {
        return false;
    };
    matches!(
        value.get("type").and_then(|kind| kind.as_str()),
        Some("message_stop") | Some("error")
    )
}

/// 流内错误事件(type: error)的详情。
pub(crate) fn terminal_error(payload: &str) -> Option<AiRequestError> {
    let value = serde_json::from_str::<serde_json::Value>(payload.trim()).ok()?;
    if value.get("type")?.as_str()? != "error" {
        return None;
    }
    let message = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(|message| message.as_str())
        .unwrap_or("provider reported an error")
        .to_string();
    Some(AiRequestError::Protocol(message))
}

/// 单条载荷 → 走向。
pub(crate) fn classify_payload(payload: &str) -> SseOutcome {
    if let Some(delta) = delta_from_sse_payload(payload) {
        return SseOutcome::Delta(delta);
    }
    if let Some(error) = terminal_error(payload) {
        return SseOutcome::Fail(error);
    }
    if is_terminal_payload(payload) {
        return SseOutcome::Done;
    }
    SseOutcome::Ignore
}

/// 非流式兜底:content 数组里拼接全部 text 块。
fn content_from_response_json(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let content = value.get("content")?.as_array()?;
    let mut text = String::new();
    for piece in content {
        if piece.get("type")?.as_str()? == "text"
            && let Some(part) = piece.get("text").and_then(|text| text.as_str())
        {
            text.push_str(part);
        }
    }
    (!text.is_empty()).then_some(text)
}

pub(crate) fn stream(
    client: &reqwest::blocking::Client,
    endpoint: &AiEndpointConfig,
    prompt: &AiPrompt,
    on_delta: &mut dyn FnMut(&str),
    cancel: &AtomicBool,
) -> Result<String, AiRequestError> {
    let url = messages_url(&endpoint.base_url);
    let body = serde_json::to_string(&request_body(&endpoint.model, prompt))
        .map_err(|error| AiRequestError::Protocol(error.to_string()))?;
    let response = send_post(
        client,
        &url,
        &[
            (
                "x-api-key",
                endpoint.api_key.trim().to_string(),
            ),
            (
                "anthropic-version",
                ANTHROPIC_VERSION.to_string(),
            ),
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
        return read_plain_json(response, cancel, &content_from_response_json, on_delta);
    }

    read_sse(response, cancel, &classify_payload, on_delta)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::ai::endpoint::ProviderKind;
    use crate::ai::prompts::StubScenario;
    use crate::ai::transport::test_support::*;

    fn endpoint_at(port: u16) -> AiEndpointConfig {
        AiEndpointConfig {
            kind: ProviderKind::Messages,
            base_url: format!("http://127.0.0.1:{port}"),
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
    fn messages_url_joins_v1_and_messages() {
        assert_eq!(
            messages_url("https://api.anthropic.com"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            messages_url("https://api.anthropic.com/v1"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            messages_url("https://proxy.example/anthropic/messages"),
            "https://proxy.example/anthropic/messages"
        );
        assert_eq!(messages_url("  "), "");
    }

    #[test]
    fn request_body_puts_system_on_top_and_sets_max_tokens() {
        let body = request_body("claude-sonnet-4-5", &prompt());
        assert_eq!(body["model"], "claude-sonnet-4-5");
        assert_eq!(body["system"], "sys");
        assert!(body["max_tokens"].as_u64().is_some());
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hi");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn delta_extraction_reads_text_deltas_only() {
        assert_eq!(
            delta_from_sse_payload(
                r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"你"}}"#
            ),
            Some("你".to_string())
        );
        // input_json_delta 之类不是正文。
        assert_eq!(
            delta_from_sse_payload(
                r#"{"type":"content_block_delta","delta":{"type":"input_json_delta"}}"#
            ),
            None
        );
        assert_eq!(
            delta_from_sse_payload(r#"{"type":"message_start","message":{}}"#),
            None
        );
    }

    #[test]
    fn classification_routes_delta_done_and_failure() {
        assert!(matches!(
            classify_payload(
                r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"x"}}"#
            ),
            SseOutcome::Delta(_)
        ));
        assert!(matches!(
            classify_payload(r#"{"type":"message_stop"}"#),
            SseOutcome::Done
        ));
        assert!(matches!(
            classify_payload(r#"{"type":"error","error":{"message":"overloaded"}}"#),
            SseOutcome::Fail(_)
        ));
        assert!(matches!(
            classify_payload(r#"{"type":"message_delta","delta":{}}"#),
            SseOutcome::Ignore
        ));
    }

    #[test]
    fn streams_deltas_from_a_live_sse_response() {
        let port = spawn_server(|stream| {
            let head = read_request_head(stream);
            assert!(
                head.starts_with("POST /v1/messages"),
                "must hit the POST /v1/messages path, got: {head}"
            );
            assert!(head.contains("x-api-key: test-key"));
            assert!(head.contains("anthropic-version: 2023-06-01"));
            write_sse_response(
                stream,
                &[
                    r#"{"type":"message_start","message":{}}"#,
                    r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"午"}}"#,
                    r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"安"}}"#,
                    r#"{"type":"message_stop"}"#,
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
        assert_eq!(full, "午安");
        assert_eq!(deltas, vec!["午".to_string(), "安".into()]);
    }

    #[test]
    fn non_stream_json_response_is_accepted_as_a_single_delta() {
        let port = spawn_server(|stream| {
            write_json_response(
                stream,
                "HTTP/1.1 200 OK",
                r#"{"content":[{"type":"text","text":"整包回答"}]}"#,
            );
        });

        let full = stream(
            &test_client(),
            &endpoint_at(port),
            &prompt(),
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .expect("ok");
        assert_eq!(full, "整包回答");
    }

    #[test]
    fn http_error_carries_status_and_provider_message() {
        let port = spawn_server(|stream| {
            write_json_response(
                stream,
                "HTTP/1.1 401 Unauthorized",
                r#"{"type":"error","error":{"message":"invalid x-api-key"}}"#,
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
                assert_eq!(message, "invalid x-api-key");
            }
            other => panic!("expected http error, got {other:?}"),
        }
    }
}
