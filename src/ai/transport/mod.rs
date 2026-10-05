//! 传输层:把「一份协议无关的提示词」按端点协议发出去。
//!
//! 统一入口 [`stream_completion`] 按 [`ProviderKind`] 分发到各协议模块。
//! 每个协议模块内聚自己的全部协议知识——URL 拼接、请求头、请求体、
//! SSE 载荷解析与终止条件——共享的只有三样:HTTP POST 与状态/错误处理
//! ([`send_post`])、SSE 读循环([`read_sse`])、错误体消息提取
//! ([`message_from_error_body`])。新增协议 = 新建模块 + 注册一个 match 臂,
//! 不碰任何既有协议的实现。
//!
//! 本模块不依赖 gpui;阻塞式读流由调用方放到工作线程,增量经 `on_delta`
//! 回调送出(编辑器侧经 channel 泵回 UI)。

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::endpoint::{AiEndpointConfig, ProviderKind};
use super::prompts::AiPrompt;
use super::sse::SseParser;

mod chat_completions;
mod messages;
mod responses;

/// 面板/设置页共用的默认客户端:连接 10s,整体 5min 上限;
/// 流式取消走 cancel 标志,不靠超时。
pub(crate) fn default_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()
        .expect("default http client")
}

/// 请求失败的原因。网络细节原样保留,给用户看的措辞由 UI 层按
/// [`AiRequestError::status_hint`] 映射成本地化文案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AiRequestError {
    /// 用户主动停止。
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
            status if (500..600).contains(&status) => {
                Some("provider server error ({status})")
            }
            _ => None,
        }
    }
}

/// 传输层统一入口:阻塞、流式、可取消,返回完整文本。
pub(crate) fn stream_completion(
    client: &reqwest::blocking::Client,
    endpoint: &AiEndpointConfig,
    prompt: &AiPrompt,
    on_delta: &mut dyn FnMut(&str),
    cancel: Arc<AtomicBool>,
) -> Result<String, AiRequestError> {
    if !endpoint.is_configured() {
        return Err(AiRequestError::Protocol(format!(
            "endpoint for kind '{}' is not fully configured",
            endpoint.kind.id()
        )));
    }
    match endpoint.kind {
        ProviderKind::ChatCompletions => {
            chat_completions::stream(client, endpoint, prompt, on_delta, &cancel)
        }
        ProviderKind::Responses => {
            responses::stream(client, endpoint, prompt, on_delta, &cancel)
        }
        ProviderKind::Messages => messages::stream(client, endpoint, prompt, on_delta, &cancel),
        // cancel 检查在 stub 内部做(分片间隙),这里提前查一次省去空转。
        ProviderKind::Stub if cancel.load(Ordering::Relaxed) => Err(AiRequestError::Cancelled),
        ProviderKind::Stub => super::stub::stream(&prompt.stub_scenario, on_delta, &cancel),
    }
}

/// 连通性探测:发一个最小请求,返回模型回复的全文(设置页「测试连接」用)。
pub(crate) fn test_endpoint(
    client: &reqwest::blocking::Client,
    endpoint: &AiEndpointConfig,
) -> Result<String, AiRequestError> {
    let probe = AiPrompt {
        system: "You are a connectivity probe. Reply with the single word: OK".to_string(),
        user: "ping".to_string(),
        stub_scenario: super::prompts::StubScenario::Custom,
    };
    stream_completion(client, endpoint, &probe, &mut |_| {}, Arc::new(AtomicBool::new(false)))
}

/// 发一个 POST 并处理状态码;非 2xx 读出错误体并转成 [`AiRequestError::Http`]。
pub(crate) fn send_post(
    client: &reqwest::blocking::Client,
    url: &str,
    headers: &[(&'static str, String)],
    body: String,
    cancel: &AtomicBool,
) -> Result<reqwest::blocking::Response, AiRequestError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AiRequestError::Cancelled);
    }
    if url.trim().is_empty() {
        return Err(AiRequestError::Protocol(
            "request URL is empty; check the endpoint base URL".to_string(),
        ));
    }
    let mut request = client.post(url).header("Content-Type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    let mut response = request
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
    Ok(response)
}

/// 单条 SSE 载荷的分类结果:协议模块把自家的事件形状映射到这里,
/// 读循环只认这四种走向。
pub(crate) enum SseOutcome {
    /// 与正文无关的事件(心跳、created、role 帧等)。
    Ignore,
    /// 一段增量正文。
    Delta(String),
    /// 流正常终止([DONE] / message_stop / response.completed)。
    Done,
    /// 流内错误(message_stop 前的 error 事件、response.failed)。
    Fail(AiRequestError),
}

/// 共享的 SSE 读循环:逐 chunk 喂解析器,`classify` 把 data 载荷分类成
/// [`SseOutcome`]。UTF-8 多字节字符被 chunk 劈开时只喂合法前缀,余下字节
/// 留到下一轮。
pub(crate) fn read_sse(
    response: reqwest::blocking::Response,
    cancel: &AtomicBool,
    classify: &dyn Fn(&str) -> SseOutcome,
    on_delta: &mut dyn FnMut(&str),
) -> Result<String, AiRequestError> {
    // 处理一批事件;`Some` = 流已终止(正常或失败),`None` = 继续读。
    fn drain_events(
        events: Vec<super::sse::SseEvent>,
        full: &mut String,
        classify: &dyn Fn(&str) -> SseOutcome,
        on_delta: &mut dyn FnMut(&str),
    ) -> Option<Result<String, AiRequestError>> {
        for event in events {
            match classify(&event.data) {
                SseOutcome::Ignore => {}
                SseOutcome::Delta(delta) => {
                    full.push_str(&delta);
                    on_delta(&delta);
                }
                SseOutcome::Done => return Some(Ok(std::mem::take(full))),
                SseOutcome::Fail(error) => return Some(Err(error)),
            }
        }
        None
    }

    let mut parser = SseParser::new();
    let mut full = String::new();
    let mut raw: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut response = response;
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
        let valid = match std::str::from_utf8(&raw) {
            Ok(text) => text.len(),
            Err(error) => error.valid_up_to(),
        };
        let text = String::from_utf8_lossy(&raw[..valid]).into_owned();
        raw.drain(..valid);
        if let Some(result) = drain_events(parser.feed(&text), &mut full, classify, on_delta) {
            return result;
        }
    }
    if let Some(result) = drain_events(parser.finish(), &mut full, classify, on_delta) {
        return result;
    }
    (!full.is_empty())
        .then_some(full)
        .ok_or_else(|| AiRequestError::Protocol("provider returned an empty completion".to_string()))
}

/// 非流式响应兜底:个别代理会无视 `stream: true` 直接回一份完整 JSON。
/// `extract` 从整包 JSON 里取正文,整段当一个增量发出。
pub(crate) fn read_plain_json(
    response: reqwest::blocking::Response,
    cancel: &AtomicBool,
    extract: &dyn Fn(&str) -> Option<String>,
    on_delta: &mut dyn FnMut(&str),
) -> Result<String, AiRequestError> {
    let mut response = response;
    let mut body_text = String::new();
    if let Err(error) = response.read_to_string(&mut body_text) {
        return Err(AiRequestError::Network(error.to_string()));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(AiRequestError::Cancelled);
    }
    match extract(&body_text) {
        Some(content) => {
            on_delta(&content);
            Ok(content)
        }
        None => Err(AiRequestError::Protocol(message_from_error_body(
            &body_text,
        ))),
    }
}

/// 从错误响应体里提取给用户看的消息:优先 error.message,退回原文(截断)。
/// OpenAI 与 Anthropic 的错误体都是 `error.message` 形状。
pub(crate) fn message_from_error_body(body: &str) -> String {
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

/// 测试支撑:本地 TCP mock 服务器与请求行读取,各协议的集成测试共用。
#[cfg(test)]
pub(crate) mod test_support {
    use std::io::Write;
    use std::time::Duration;

    /// 起一个一次性 mock:接受一条连接,交给 `script` 处理后结束。
    pub(crate) fn spawn_server(
        script: impl FnOnce(&mut std::net::TcpStream) + Send + 'static,
    ) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            script(&mut stream);
        });
        port
    }

    /// 读完整个请求头(请求行 + 全部头部,含结尾空行前的所有内容)。
    /// 必须一次读全:分两次各建 BufReader 会把第一次缓冲里剩余的字节丢掉。
    pub(crate) fn read_request_head(stream: &std::net::TcpStream) -> String {
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stream);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line).expect("read header");
            if read == 0 || line == "\r\n" || line == "\n" {
                break;
            }
            head.push_str(&line);
        }
        head
    }

    /// 写 HTTP 头 + 分片 SSE 事件,块间稍作停顿模拟流式。
    pub(crate) fn write_sse_response(
        stream: &mut std::net::TcpStream,
        events: &[String],
    ) {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .expect("write head");
        for event in events {
            stream
                .write_all(format!("data: {event}\n\n").as_bytes())
                .expect("write event");
            stream.flush().expect("flush");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 写一个固定 JSON 响应(非流式/错误体用)。
    pub(crate) fn write_json_response(
        stream: &mut std::net::TcpStream,
        status_line: &str,
        body: &str,
    ) {
        let response = format!(
            "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).expect("write json");
    }

    /// 短超时客户端,测试挂死不超过 10s。
    pub(crate) fn test_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("test client")
    }
}
