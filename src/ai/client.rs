//! OpenAI 兼容 Chat Completions 客户端(阻塞式,SSE 流式)。
//!
//! 调用方把 [`stream_chat_completion`] 放到 `std::thread` 上跑(与
//! `net::update`、导出任务同一条线程+channel 模式),增量文本经 unbounded
//! channel 送回 UI;`cancel` 置位后按 chunk 粒度尽快退出。本模块不碰 gpui。

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::sse::{SseEvent, SseParser};

/// 单次请求的服务端配置(设置页的「地址/密钥/模型」三元组)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AiEndpointConfig {
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) model: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    fn as_str(self) -> &'static str {
        match self {
            ChatRole::System => "system",
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChatMessage {
    pub(crate) role: ChatRole,
    pub(crate) content: String,
}

impl ChatMessage {
    pub(crate) fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    pub(crate) fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }
}

/// 请求失败的原因。网络细节原样保留,给用户看的措辞由 UI 层按 [`Self::status`]
/// 与 [`status_hint`] 映射成本地化文案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AiRequestError {
    /// 用户主动停止(或文档已变化导致结果作废后的取消)。
    Cancelled,
    /// 服务端返回非 2xx;`message` 尽量取自响应体里的 error.message。
    Http { status: u16, message: String },
    /// 连不上、超时、被重置等传输层失败。
    Network(String),
    /// 2xx 但响应不是预期的形状(缺字段、非 JSON)。
    Protocol(String),
}

impl AiRequestError {
    /// 常见状态码的一句话归因(UI 层拿它替换本地化模板里的 `{error}`)。
    pub(crate) fn status_hint(status: u16) -> Option<&'static str> {
        match status {
            401 | 403 => Some("API key was rejected (401/403)"),
            404 => Some("endpoint or model not found (404)"),
            429 => Some("rate limited or quota exceeded (429)"),
            status if (500..600).contains(&status) => Some("provider server error ({status})"),
            _ => None,
        }
    }
}

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
pub(crate) fn chat_request_body(model: &str, messages: &[ChatMessage]) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": messages
            .iter()
            .map(|message| serde_json::json!({
                "role": message.role.as_str(),
                "content": message.content,
            }))
            .collect::<Vec<_>>(),
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

/// 从错误响应体里提取给用户看的消息:优先 error.message,退回原文(截断)。
fn message_from_error_body(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
        {
            return message.to_string();
        }
    }
    let mut text = body.trim().to_string();
    if text.len() > 300 {
        text = format!("{}…", &text[..text.floor_char_boundary(300)]);
    }
    text
}

/// 流式补全。阻塞直至完成/失败/取消;每个增量回调 `on_delta`,返回完整文本。
pub(crate) fn stream_chat_completion(
    config: &AiEndpointConfig,
    messages: &[ChatMessage],
    on_delta: &mut dyn FnMut(&str),
    cancel: Arc<AtomicBool>,
) -> Result<String, AiRequestError> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        // 流式响应可能 legitimately 很长,整体上限放宽到 5 分钟;
        // 用户的「停止」走 cancel,不靠超时。
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| AiRequestError::Network(error.to_string()))?;
    stream_chat_completion_with_client(&client, config, messages, on_delta, cancel)
}

/// 同 [`stream_chat_completion`],但客户端由调用方注入(测试用短超时)。
pub(crate) fn stream_chat_completion_with_client(
    client: &reqwest::blocking::Client,
    config: &AiEndpointConfig,
    messages: &[ChatMessage],
    on_delta: &mut dyn FnMut(&str),
    cancel: Arc<AtomicBool>,
) -> Result<String, AiRequestError> {
    if config.base_url.trim().is_empty() {
        return Err(AiRequestError::Protocol(
            "API base URL is empty".to_string(),
        ));
    }
    let url = chat_completions_url(&config.base_url);
    let body = serde_json::to_string(&chat_request_body(&config.model, messages))
        .map_err(|error| AiRequestError::Protocol(error.to_string()))?;

    if cancel.load(Ordering::Relaxed) {
        return Err(AiRequestError::Cancelled);
    }
    let mut response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .body(body)
        .send()
        .map_err(|error| AiRequestError::Network(error.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let mut error_body = String::new();
        let _ = response.read_to_string(&mut error_body);
        let mut message = message_from_error_body(&error_body);
        if message.is_empty() {
            message = AiRequestError::status_hint(status.as_u16())
                .unwrap_or("request failed")
                .replace("{status}", &status.as_u16().to_string());
        }
        return Err(AiRequestError::Http {
            status: status.as_u16(),
            message,
        });
    }

    let is_event_stream = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"));
    if !is_event_stream {
        // 非流式兜底:整包读完按 completion JSON 解。
        let mut body_text = String::new();
        if let Err(error) = response.read_to_string(&mut body_text) {
            return Err(AiRequestError::Network(error.to_string()));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(AiRequestError::Cancelled);
        }
        return match content_from_completion_json(&body_text) {
            Some(content) => {
                on_delta(&content);
                Ok(content)
            }
            None => Err(AiRequestError::Protocol(
                message_from_error_body(&body_text),
            )),
        };
    }

    let mut parser = SseParser::new();
    let mut full = String::new();
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiRequestError::Cancelled);
        }
        let read = response
            .read(&mut chunk)
            .map_err(|error| AiRequestError::Network(error.to_string()))?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
        // chunk 可能把一个 UTF-8 字符劈成两半:只喂合法前缀,余下字节留到下一轮。
        let valid = match std::str::from_utf8(&raw) {
            Ok(text) => text.len(),
            Err(error) => error.valid_up_to(),
        };
        let text = String::from_utf8_lossy(&raw[..valid]).into_owned();
        raw.drain(..valid);
        for SseEvent { data } in parser.feed(&text) {
            if data.trim() == "[DONE]" {
                return Ok(full);
            }
            if let Some(delta) = delta_from_sse_payload(&data) {
                full.push_str(&delta);
                on_delta(&delta);
            }
        }
    }
    for SseEvent { data } in parser.finish() {
        if let Some(delta) = delta_from_sse_payload(&data) {
            full.push_str(&delta);
            on_delta(&delta);
        }
    }
    if full.is_empty() {
        return Err(AiRequestError::Protocol(
            "provider returned an empty completion".to_string(),
        ));
    }
    Ok(full)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::mpsc as std_mpsc;

    use super::*;

    fn config_at(port: u16) -> AiEndpointConfig {
        AiEndpointConfig {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            api_key: "test-key".into(),
            model: "test-model".into(),
        }
    }

    fn messages() -> Vec<ChatMessage> {
        vec![ChatMessage::user("hi")]
    }

    fn short_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("client")
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
    fn request_body_carries_model_messages_and_stream_flag() {
        let body = chat_request_body(
            "glm-4-flash",
            &[ChatMessage::system("sys"), ChatMessage::user("你好")],
        );
        assert_eq!(body["model"], "glm-4-flash");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "你好");
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
            message_from_error_body(r#"{"error":{"message":"bad key","type":"auth"}}"#),
            "bad key"
        );
        assert_eq!(message_from_error_body("plain text"), "plain text");
    }

    /// 本地 mock:起一个 TCP 服务,校验请求行/头,再按给定脚本回 SSE。
    fn spawn_sse_server(script: impl FnOnce(&std::net::TcpStream) + Send + 'static) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            script(&stream);
        });
        port
    }

    fn read_request_line(stream: &std::net::TcpStream) -> String {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read request line");
        line
    }

    #[test]
    fn streams_deltas_from_a_live_sse_response() {
        let port = spawn_sse_server(|stream| {
            assert!(
                read_request_line(stream).starts_with("POST /v1/chat/completions"),
                "must hit the chat/completions path"
            );
            let mut stream = stream;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .expect("write head");
            for payload in [
                r#"{"choices":[{"delta":{"content":"你"}}]}"#,
                r#"{"choices":[{"delta":{"content":"好"}}]}"#,
                "[DONE]",
            ] {
                stream
                    .write_all(format!("data: {payload}\n\n").as_bytes())
                    .expect("write event");
                stream.flush().expect("flush");
            }
        });

        let mut deltas = Vec::new();
        let result = stream_chat_completion_with_client(
            &short_client(),
            &config_at(port),
            &messages(),
            &mut |delta| deltas.push(delta.to_string()),
            Arc::new(AtomicBool::new(false)),
        );
        let full = result.expect("stream succeeds");
        assert_eq!(full, "你好");
        assert_eq!(deltas, vec!["你".to_string(), "好".into()]);
    }

    #[test]
    fn non_stream_json_response_is_accepted_as_a_single_delta() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let body = r#"{"choices":[{"message":{"content":"完整回答"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = stream;
            stream.write_all(response.as_bytes()).expect("write");
        });

        let mut deltas = Vec::new();
        let result = stream_chat_completion_with_client(
            &short_client(),
            &config_at(port),
            &messages(),
            &mut |delta| deltas.push(delta.to_string()),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(result.expect("ok"), "完整回答");
        assert_eq!(deltas, vec!["完整回答".to_string()]);
    }

    #[test]
    fn http_error_carries_status_and_provider_message() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let body = r#"{"error":{"message":"Invalid API key"}}"#;
            let response = format!(
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut stream = stream;
            stream.write_all(response.as_bytes()).expect("write");
        });

        let result = stream_chat_completion_with_client(
            &short_client(),
            &config_at(port),
            &messages(),
            &mut |_| {},
            Arc::new(AtomicBool::new(false)),
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
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (held_tx, held_rx) = std_mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut stream = stream;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
                )
                .expect("write head");
            stream.flush().expect("flush");
            let _ = held_tx.send(());
            // 不关流、不写正文:取消测试靠 flag 而不是 EOF。
            std::thread::sleep(Duration::from_secs(10));
        });

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = cancel.clone();
        let (result_tx, result_rx) = std_mpsc::channel();
        std::thread::spawn(move || {
            let _ = held_rx.recv_timeout(Duration::from_secs(5));
            cancel_for_thread.store(true, Ordering::Relaxed);
        });
        std::thread::spawn(move || {
            let result = stream_chat_completion_with_client(
                &short_client(),
                &config_at(port),
                &messages(),
                &mut |_| {},
                cancel,
            );
            let _ = result_tx.send(result);
        });
        let result = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("cancel within timeout");
        assert_eq!(result, Err(AiRequestError::Cancelled));
    }

    #[test]
    fn empty_base_url_fails_fast_without_network() {
        let config = AiEndpointConfig {
            base_url: "  ".into(),
            api_key: String::new(),
            model: "m".into(),
        };
        let result = stream_chat_completion_with_client(
            &short_client(),
            &config,
            &messages(),
            &mut |_| {},
            Arc::new(AtomicBool::new(false)),
        );
        assert!(matches!(result, Err(AiRequestError::Protocol(_))));
    }
}
