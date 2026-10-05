//! OpenAI Responses API 传输。
//!
//! `POST {base}/responses`,`instructions` 承载系统提示、`input` 承载任务,
//! SSE 事件流以 `response.output_text.delta` 递增正文、`response.completed`
//! 终止。与 Chat Completions 同源的鉴权(Bearer)。

use std::sync::atomic::AtomicBool;

use super::{read_plain_json, read_sse, send_post, AiRequestError, SseOutcome};
use crate::ai::endpoint::AiEndpointConfig;
use crate::ai::prompts::AiPrompt;

/// 把 base_url 规范成 responses 的完整地址:填了完整路径不重复追加。
pub(crate) fn responses_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.ends_with("/responses") {
        return trimmed.to_string();
    }
    format!("{trimmed}/responses")
}

fn request_body(model: &str, prompt: &AiPrompt) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "instructions": prompt.system,
        "input": prompt.user,
        "stream": true,
    })
}

/// 增量事件:`response.output_text.delta`,正文在 `delta` 字段。
pub(crate) fn delta_from_sse_payload(payload: &str) -> Option<String> {
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return None;
    }
    let value = serde_json::from_str::<serde_json::Value>(payload).ok()?;
    let event_type = value.get("type")?.as_str()?;
    if event_type != "response.output_text.delta" {
        return None;
    }
    value
        .get("delta")?
        .as_str()
        .map(str::to_string)
        .filter(|delta| !delta.is_empty())
}

/// 单条载荷 → 走向:增量 / 正常终止 / 流内失败。
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

/// 流终止:completed / failed / incomplete / error / [DONE] 任一都结束读取。
pub(crate) fn is_terminal_payload(payload: &str) -> bool {
    let trimmed = payload.trim();
    if trimmed == "[DONE]" {
        return true;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return false;
    };
    matches!(
        value.get("type").and_then(|kind| kind.as_str()),
        Some("response.completed") | Some("response.failed") | Some("response.incomplete")
    ) || value.get("error").is_some()
}

/// 终止事件携带的错误详情(response.failed / error 事件)。
pub(crate) fn terminal_error(payload: &str) -> Option<AiRequestError> {
    let value = serde_json::from_str::<serde_json::Value>(payload.trim()).ok()?;
    let event_type = value.get("type")?.as_str()?;
    match event_type {
        "response.failed" | "response.incomplete" => {
            let message = value
                .get("response")
                .and_then(|response| response.get("status"))
                .and_then(|status| status.as_str())
                .unwrap_or(event_type)
                .to_string();
            Some(AiRequestError::Protocol(format!(
                "response ended with status '{message}'"
            )))
        }
        "error" => {
            let message = value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(|message| message.as_str())
                .unwrap_or("provider reported an error")
                .to_string();
            Some(AiRequestError::Protocol(message))
        }
        _ => None,
    }
}

/// 非流式兜底:从 output 数组里拼出 message 类型的 output_text。
fn content_from_response_json(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let output = value.get("output")?.as_array()?;
    let mut text = String::new();
    for item in output {
        if item.get("type")?.as_str()? != "message" {
            continue;
        }
        for piece in item.get("content")?.as_array()? {
            if piece.get("type")?.as_str()? == "output_text"
                && let Some(part) = piece.get("text").and_then(|text| text.as_str())
            {
                text.push_str(part);
            }
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
    let url = responses_url(&endpoint.base_url);
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
            kind: ProviderKind::Responses,
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
    fn responses_url_appends_path_once() {
        assert_eq!(
            responses_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/responses"
        );
        assert_eq!(
            responses_url("https://api.openai.com/v1/responses"),
            "https://api.openai.com/v1/responses"
        );
        assert_eq!(responses_url("  "), "");
    }

    #[test]
    fn delta_extraction_reads_output_text_deltas_only() {
        assert_eq!(
            delta_from_sse_payload(
                r#"{"type":"response.output_text.delta","delta":"你"}"#
            ),
            Some("你".to_string())
        );
        assert_eq!(
            delta_from_sse_payload(r#"{"type":"response.created","response":{}}"#),
            None
        );
        assert_eq!(
            delta_from_sse_payload(r#"{"type":"response.output_text.done","text":"全部"}"#),
            None
        );
    }

    #[test]
    fn terminal_detection_covers_completed_and_done() {
        assert!(is_terminal_payload("[DONE]"));
        assert!(is_terminal_payload(
            r#"{"type":"response.completed","response":{}}"#
        ));
        assert!(is_terminal_payload(r#"{"type":"response.failed","response":{}}"#));
        assert!(!is_terminal_payload(
            r#"{"type":"response.output_text.delta","delta":"x"}"#
        ));
    }

    #[test]
    fn classification_routes_delta_done_and_failure() {
        assert!(matches!(
            classify_payload(r#"{"type":"response.output_text.delta","delta":"x"}"#),
            SseOutcome::Delta(_)
        ));
        assert!(matches!(
            classify_payload(r#"{"type":"response.completed","response":{}}"#),
            SseOutcome::Done
        ));
        assert!(matches!(
            classify_payload(r#"{"type":"response.failed","response":{"status":"failed"}}"#),
            SseOutcome::Fail(_)
        ));
        assert!(matches!(
            classify_payload(r#"{"type":"response.created","response":{}}"#),
            SseOutcome::Ignore
        ));
    }

    #[test]
    fn streams_deltas_from_a_live_sse_response() {
        let port = spawn_server(|stream| {
            let head = read_request_head(stream);
            assert!(
                head.starts_with("POST /v1/responses"),
                "must hit the POST /v1/responses path, got: {head}"
            );
            assert!(head.contains("authorization: Bearer test-key"));
            write_sse_response(
                stream,
                &[
                    r#"{"type":"response.created","response":{}}"#,
                    r#"{"type":"response.output_text.delta","delta":"早"}"#,
                    r#"{"type":"response.output_text.delta","delta":"安"}"#,
                    r#"{"type":"response.completed","response":{}}"#,
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
        assert_eq!(full, "早安");
        assert_eq!(deltas, vec!["早".to_string(), "安".into()]);
    }

    #[test]
    fn non_stream_json_response_is_accepted_as_a_single_delta() {
        let port = spawn_server(|stream| {
            write_json_response(
                stream,
                "HTTP/1.1 200 OK",
                r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"整包回答"}]}]}"#,
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
        assert_eq!(full, "整包回答");
        assert_eq!(deltas, vec!["整包回答".to_string()]);
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
}
