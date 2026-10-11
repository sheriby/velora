//! HTTP client integration used by remote image loading.

pub(crate) mod update;

use std::io;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use futures::AsyncReadExt;
use futures::FutureExt;
use gpui::App;
use gpui::http_client::{self, AsyncBody, HttpClient};
use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, HeaderMap, HeaderValue, PRAGMA, USER_AGENT,
};

const DEFAULT_IMAGE_ACCEPT: &str =
    "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8";
const DEFAULT_ACCEPT_LANGUAGE: &str = "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7";
const DEFAULT_CACHE_CONTROL: &str = "no-cache";
pub(crate) const MAX_IMAGE_BYTES: usize = 50 * 1024 * 1024;

/// HTTP 客户端装配失败的原因（进程内只装一次，失败即定格）。远程图占位框
/// 据此把「整篇没有网络能力」与「这一张加载失败」分开说——gpui 的回退闭包
/// 不带错误参数，界面层只能从这里拿到真实原因（评审报告安全建议 7）。
static HTTP_CLIENT_INSTALL_ERROR: OnceLock<String> = OnceLock::new();

pub(crate) fn http_client_install_error() -> Option<&'static str> {
    HTTP_CLIENT_INSTALL_ERROR.get().map(String::as_str)
}

pub(crate) fn install_http_client(cx: &mut App) {
    match ReqwestTransportHttpClient::new(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36",
    ) {
        Ok(client) => cx.set_http_client(Arc::new(client)),
        Err(error) => {
            let _ = HTTP_CLIENT_INSTALL_ERROR.set(error.to_string());
            eprintln!("failed to install HTTP client for image loading: {error}");
        }
    }
}

pub(crate) fn is_remote_image_source(source: &str) -> bool {
    http_client::Uri::from_str(source)
        .ok()
        .and_then(|uri| uri.scheme_str().map(str::to_owned))
        .is_some_and(|scheme| scheme == "http" || scheme == "https")
}

struct ReqwestTransportHttpClient {
    client: reqwest::blocking::Client,
    user_agent: HeaderValue,
    default_headers: HeaderMap,
    runtime: Arc<tokio::runtime::Runtime>,
    requests: Arc<tokio::sync::Semaphore>,
    max_response_bytes: usize,
}

struct ImageRequestCancellation(tokio::task::AbortHandle);

impl Drop for ImageRequestCancellation {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl ReqwestTransportHttpClient {
    fn new(user_agent: &str) -> anyhow::Result<Self> {
        let default_headers = default_image_request_headers(user_agent)?;
        let (max_response_bytes, timeout) = (MAX_IMAGE_BYTES, std::time::Duration::from_secs(30));
        #[cfg(test)]
        let (max_response_bytes, timeout) = tests::REQUEST_LIMITS.with(|limits| limits.get()).unwrap_or((max_response_bytes, timeout));
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(timeout)
            .user_agent(user_agent)
            .default_headers(default_headers.clone())
            .build()?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1).max_blocking_threads(8).enable_all().build()?;
        Ok(Self {
            client,
            user_agent: HeaderValue::from_str(user_agent)?,
            default_headers,
            runtime: Arc::new(runtime),
            requests: Arc::new(tokio::sync::Semaphore::new(128)),
            max_response_bytes,
        })
    }

    fn execute_request(
        client: reqwest::blocking::Client,
        default_headers: HeaderMap,
        request: http_client::Request<AsyncBody>,
        max_response_bytes: usize,
    ) -> anyhow::Result<http_client::Response<AsyncBody>> {
        let (parts, mut body) = request.into_parts();
        let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())?;
        let url = parts.uri.to_string();
        let body_bytes = futures::executor::block_on(async move {
            let mut bytes = Vec::new();
            body.read_to_end(&mut bytes).await?;
            Ok::<Vec<u8>, io::Error>(bytes)
        })?;

        let mut builder = apply_missing_default_headers(
            client.request(method, url),
            &parts.headers,
            &default_headers,
        );
        if !body_bytes.is_empty() {
            builder = builder.body(body_bytes);
        }

        let response = builder.send()?;
        let status = response.status();
        let version = response.version();
        let headers = response.headers().clone();
        if response.content_length().is_some_and(|length| length > max_response_bytes as u64) {
            anyhow::bail!("图片响应超过字节上限");
        }
        let mut bytes = Vec::new();
        let mut limited_response = std::io::Read::take(response, max_response_bytes as u64 + 1);
        std::io::Read::read_to_end(&mut limited_response, &mut bytes)?;
        if bytes.len() > max_response_bytes {
            anyhow::bail!("图片响应超过字节上限");
        }

        let mut response_builder = http_client::Response::builder()
            .status(status)
            .version(version);
        for (name, value) in &headers {
            response_builder = response_builder.header(name, value);
        }
        Ok(response_builder.body(AsyncBody::from(bytes))?)
    }
}

fn default_image_request_headers(user_agent: &str) -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_str(user_agent)?);
    headers.insert(ACCEPT, HeaderValue::from_static(DEFAULT_IMAGE_ACCEPT));
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_static(DEFAULT_ACCEPT_LANGUAGE),
    );
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static(DEFAULT_CACHE_CONTROL),
    );
    headers.insert(PRAGMA, HeaderValue::from_static(DEFAULT_CACHE_CONTROL));
    Ok(headers)
}

fn apply_missing_default_headers(
    builder: reqwest::blocking::RequestBuilder,
    request_headers: &HeaderMap,
    default_headers: &HeaderMap,
) -> reqwest::blocking::RequestBuilder {
    let mut headers = request_headers.clone();
    for (name, value) in default_headers {
        if !headers.contains_key(name) {
            headers.insert(name.clone(), value.clone());
        }
    }
    builder.headers(headers)
}

impl HttpClient for ReqwestTransportHttpClient {
    fn type_name(&self) -> &'static str {
        "velora_reqwest_transport_http_client"
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        Some(&self.user_agent)
    }

    fn send(
        &self,
        request: http_client::Request<AsyncBody>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<http_client::Response<AsyncBody>>> {
        let client = self.client.clone();
        let default_headers = self.default_headers.clone();
        let max_response_bytes = self.max_response_bytes;
        let permit = match self.requests.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => return futures::future::ready(Err(anyhow::anyhow!("图片请求队列已满"))).boxed(),
        };
        let task = self.runtime.spawn_blocking(move || {
            let _permit = permit;
            Self::execute_request(client, default_headers, request, max_response_bytes)
        });
        let cancellation = ImageRequestCancellation(task.abort_handle());
        async move {
            let _cancellation = cancellation;
            task.await.map_err(|error| anyhow::anyhow!("图片请求任务失败：{error}"))?
        }
        .boxed()
    }

    fn proxy(&self) -> Option<&http_client::Url> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_ACCEPT_LANGUAGE, DEFAULT_CACHE_CONTROL, DEFAULT_IMAGE_ACCEPT,
        apply_missing_default_headers, default_image_request_headers, is_remote_image_source,
    };
    use reqwest::header::{
        ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CACHE_CONTROL, CONNECTION, CONTENT_LENGTH, HOST,
        HeaderMap, HeaderValue, PRAGMA, USER_AGENT,
    };

    const TEST_USER_AGENT: &str = "VeloraTest/1.0";

    std::thread_local! {
        pub(super) static REQUEST_LIMITS: std::cell::Cell<Option<(usize, std::time::Duration)>> = const { std::cell::Cell::new(None) };
    }

    fn test_transport(limit: usize, timeout: std::time::Duration) -> super::ReqwestTransportHttpClient {
        REQUEST_LIMITS.with(|limits| limits.set(Some((limit, timeout))));
        let client = super::ReqwestTransportHttpClient::new(TEST_USER_AGENT).unwrap();
        REQUEST_LIMITS.with(|limits| limits.set(None));
        client
    }

    fn image_request(url: &str) -> gpui::http_client::Request<gpui::http_client::AsyncBody> {
        gpui::http_client::Request::builder().uri(url).body(gpui::http_client::AsyncBody::empty()).unwrap()
    }

    #[test]
    fn review_remote_image_body_is_bounded_with_and_without_content_length() {
        use gpui::http_client::HttpClient;
        use std::io::{Read, Write};
        for chunked in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/image.png", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = [0; 4096]; socket.read(&mut request).unwrap();
                let payload = "x".repeat(1025);
                let response = if chunked {
                    format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n401\r\n{payload}\r\n0\r\n\r\n")
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: 1025\r\nConnection: close\r\n\r\n{payload}")
                };
                if let Err(error) = socket.write_all(response.as_bytes()) {
                    assert!(matches!(error.kind(), std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset));
                }
            });
            let client = test_transport(1024, std::time::Duration::from_secs(5));
            let result = futures::executor::block_on(client.send(image_request(&url)));
            server.join().unwrap();
            assert!(result.is_err(), "超限响应应当拒绝：chunked={chunked}");
        }
    }

    #[test]
    fn review_remote_image_stalled_response_returns_timeout_error() {
        use gpui::http_client::HttpClient;
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/image.png", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096]; socket.read(&mut request).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(400));
        });
        let client = test_transport(1024, std::time::Duration::from_millis(50));
        let result = futures::executor::block_on(client.send(image_request(&url)));
        server.join().unwrap();
        let error = result.err().expect("停滞响应应失败");
        assert!(error.downcast_ref::<reqwest::Error>().is_some_and(reqwest::Error::is_timeout), "{error}");
    }

    #[test]
    fn default_image_headers_include_browser_like_fetch_context() {
        let headers = default_image_request_headers(TEST_USER_AGENT).expect("headers");

        assert_eq!(headers.get(USER_AGENT).unwrap(), TEST_USER_AGENT);
        assert_eq!(headers.get(ACCEPT).unwrap(), DEFAULT_IMAGE_ACCEPT);
        assert_eq!(
            headers.get(ACCEPT_LANGUAGE).unwrap(),
            DEFAULT_ACCEPT_LANGUAGE
        );
        assert_eq!(headers.get(CACHE_CONTROL).unwrap(), DEFAULT_CACHE_CONTROL);
        assert_eq!(headers.get(PRAGMA).unwrap(), DEFAULT_CACHE_CONTROL);
    }

    #[test]
    fn default_image_headers_leave_transport_managed_headers_unset() {
        let headers = default_image_request_headers(TEST_USER_AGENT).expect("headers");

        assert!(!headers.contains_key(ACCEPT_ENCODING));
        assert!(!headers.contains_key(CONNECTION));
        assert!(!headers.contains_key(CONTENT_LENGTH));
        assert!(!headers.contains_key(HOST));
    }

    #[test]
    fn explicit_request_headers_override_default_image_headers() {
        let defaults = default_image_request_headers(TEST_USER_AGENT).expect("headers");
        let mut request_headers = HeaderMap::new();
        request_headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        request_headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-GB"));

        let request = apply_missing_default_headers(
            reqwest::blocking::Client::new().get("https://example.com/image.png"),
            &request_headers,
            &defaults,
        )
        .build()
        .expect("request should build");
        let headers = request.headers();

        assert_eq!(headers.get(ACCEPT).unwrap(), "application/json");
        assert_eq!(headers.get(ACCEPT_LANGUAGE).unwrap(), "en-GB");
        assert_eq!(headers.get(USER_AGENT).unwrap(), TEST_USER_AGENT);
        assert_eq!(headers.get(CACHE_CONTROL).unwrap(), DEFAULT_CACHE_CONTROL);
        assert_eq!(headers.get(PRAGMA).unwrap(), DEFAULT_CACHE_CONTROL);
    }

    #[test]
    fn detects_remote_http_sources() {
        assert!(is_remote_image_source("https://example.com/image.png"));
        assert!(is_remote_image_source("http://example.com/image.gif"));
        assert!(!is_remote_image_source("./image.png"));
        assert!(!is_remote_image_source("images/photo.jpg"));
    }
}

#[cfg(test)]
mod install_visibility_tests {
    use super::{HTTP_CLIENT_INSTALL_ERROR, http_client_install_error};

    /// 占位框区分「整篇没有网络能力」依据的就是这条读回契约（评审报告
    /// 安全建议 7）。失败分支本机构造不出（reqwest 总能装上，install_http_
    /// client 由 main 持有），注入侧写侧各断一端：写入可见、未写为 None。
    #[test]
    fn install_error_is_readable_once_recorded() {
        assert!(http_client_install_error().is_none(), "进程初装不该有错误记录");
        assert!(HTTP_CLIENT_INSTALL_ERROR.set("probe: no network".into()).is_ok());
        assert_eq!(
            http_client_install_error(),
            Some("probe: no network"),
            "装配失败的原因必须能被界面层读到"
        );
    }
}
