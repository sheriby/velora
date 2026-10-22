use super::*;
use crate::ai::settings::{Endpoint, Protocol};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

pub(crate) fn read_request(stream: &mut TcpStream) -> (String, Value) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let count = stream.read(&mut chunk).unwrap();
        assert!(count > 0, "请求被提前关闭");
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
            let length = header
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            if bytes.len() >= end + 4 + length {
                return (
                    header.lines().next().unwrap().to_owned(),
                    serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap(),
                );
            }
        }
    }
}

fn events(protocol: Protocol, tool: bool) -> String {
    let arguments = json!({"path":"note.md"}).to_string();
    let messages = match protocol {
        Protocol::ChatCompletions => {
            let delta = if tool {
                json!({"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_document","arguments":arguments}}]})
            } else {
                json!({"role":"assistant","content":"Done"})
            };
            vec![
                json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":if tool {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}),
            ]
        }
        Protocol::Responses => {
            let mut messages = if tool {
                vec![
                    json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"read_document","arguments":""}}),
                    json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc_1","arguments":arguments}),
                ]
            } else {
                vec![json!({"type":"response.output_text.delta","output_index":0,"delta":"Done"})]
            };
            messages.push(json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}}));
            messages
        }
        Protocol::Messages => {
            let block = if tool {
                json!({"type":"tool_use","id":"call_1","name":"read_document","input":{}})
            } else {
                json!({"type":"text","text":""})
            };
            let delta = if tool {
                json!({"type":"input_json_delta","partial_json":arguments})
            } else {
                json!({"type":"text_delta","text":"Done"})
            };
            vec![
                json!({"type":"message_start","message":{"id":"msg_1","role":"assistant","content":[],"model":"fixture","usage":{"input_tokens":10,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":block}),
                json!({"type":"content_block_delta","index":0,"delta":delta}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":if tool {"tool_use"} else {"end_turn"}},"usage":{"output_tokens":2}}),
                json!({"type":"message_stop"}),
            ]
        }
    };
    let mut stream = String::new();
    for message in messages {
        if protocol != Protocol::ChatCompletions {
            stream.push_str(&format!("event: {}\n", message["type"].as_str().unwrap()));
        }
        stream.push_str(&format!("data: {message}\n\n"));
    }
    if protocol == Protocol::ChatCompletions {
        stream.push_str("data: [DONE]\n\n");
    }
    stream
}

fn without_cache_breakpoints(value: &Value) -> Value {
    // Anthropic 的末尾缓存断点会随消息追加移动；模型输入正文仍须保持原前缀。
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .filter(|(key, _)| key.as_str() != "cache_control")
                .map(|(key, value)| (key.clone(), without_cache_breakpoints(value)))
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.iter().map(without_cache_breakpoints).collect())
        }
        _ => value.clone(),
    }
}

fn verify_protocol(protocol: Protocol, suffix: &str) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for tool in [true, false, false] {
            let start = Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && start.elapsed() < Duration::from_secs(10) =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("本地协议测试没有收到请求：{error}"),
                }
            };
            requests.push(read_request(&mut stream));
            let body = events(protocol, tool);
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
        requests
    });
    let endpoint = Endpoint {
        protocol,
        name: "本地协议夹具".into(),
        base_url: format!("http://{address}/v1"),
        model: "fixture".into(),
        ..Endpoint::default()
    };
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![crate::ai::DocumentSnapshot {
                id: "note.md".into(),
                source: "fixture-content".into(),
            }],
        )
        .unwrap(),
    ));
    let output = run_blocking(
        RunRequest {
            model: endpoint.model_config().unwrap(),
            api_key: String::new(),
            history: Vec::new(),
            prompt: "Read note.md, then reply Done".into(),
            attachments: Vec::new(),
            system: "测试".into(),
            max_turns: 3,
            timeout: Duration::from_secs(10),
        },
        workspace.clone(),
        RunControl::default(),
        |_| {},
    )
    .unwrap();
    assert!(output.error.is_none(), "{output:?}");
    let resumed = run_blocking(
        RunRequest {
            model: endpoint.model_config().unwrap(),
            api_key: String::new(),
            history: output.history.clone(),
            prompt: "继续解释，不改变之前的对话".into(),
            attachments: Vec::new(),
            system: "测试".into(),
            max_turns: 3,
            timeout: Duration::from_secs(10),
        },
        workspace.clone(),
        RunControl::default(),
        |_| {},
    )
    .unwrap();
    assert!(resumed.error.is_none(), "{resumed:?}");
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    for pair in requests.windows(2) {
        assert_eq!(
            pair[0].1.get("tools"),
            pair[1].1.get("tools"),
            "工具定义必须保持稳定"
        );
        assert_eq!(pair[0].1.get("system"), pair[1].1.get("system"));
        assert_eq!(pair[0].1.get("instructions"), pair[1].1.get("instructions"));
        let key = if protocol == Protocol::Responses {
            "input"
        } else {
            "messages"
        };
        let before = without_cache_breakpoints(pair[0].1.get(key).unwrap());
        let after = without_cache_breakpoints(pair[1].1.get(key).unwrap());
        assert!(
            after
                .as_array()
                .unwrap()
                .starts_with(before.as_array().unwrap()),
            "工具续轮与新一轮对话只能追加历史，不能重写消息前缀：{protocol:?}"
        );
    }
    assert!(
        requests
            .iter()
            .all(|(request, _)| request.starts_with(&format!("POST /v1/{suffix} ")))
    );
    assert!(requests[0].1.to_string().contains("read_document"));
    assert!(
        requests[1].1.to_string().contains("fixture-content"),
        "工具结果必须进入第二轮请求"
    );
    assert!(output.input_tokens > 0 && output.output_tokens > 0);
    assert!(workspace.lock().unwrap().changes.proposals.is_empty());
}

#[test]
fn ai_chat_completions_transport_executes_a_tool_and_continues() {
    verify_protocol(Protocol::ChatCompletions, "chat/completions");
}
#[test]
fn ai_responses_transport_executes_a_tool_and_continues() {
    verify_protocol(Protocol::Responses, "responses");
}
#[test]
fn ai_messages_transport_executes_a_tool_and_continues() {
    verify_protocol(Protocol::Messages, "messages");
}

#[test]
fn ai_runtime_completes_http_tools_from_a_small_native_background_stack() {
    const PROBE: &str = "VELORA_AI_SMALL_STACK_PROBE";
    if std::env::var_os(PROBE).is_some() {
        std::thread::Builder::new()
            .name("native-background-stack".into())
            .stack_size(512 * 1024)
            .spawn(|| verify_protocol(Protocol::ChatCompletions, "chat/completions"))
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let _process_guard = crate::test_support::exclusive_process_probe();
    // 栈溢出会终止整个进程，子进程让回归仍能明确落在退出状态断言上。
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "ai::runtime::protocol_tests::ai_runtime_completes_http_tools_from_a_small_native_background_stack", "--nocapture"])
        .env(PROBE, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "原生后台线程必须完成实际 HTTP 工具链，不能栈溢出：{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn anthropic_sse(messages: &[Value]) -> String {
    let mut stream = String::new();
    for message in messages {
        stream.push_str(&format!("event: {}\n", message["type"].as_str().unwrap()));
        stream.push_str(&format!("data: {message}\n\n"));
    }
    stream
}

/// `close_block` 为 false 模拟网关漏发 `content_block_stop`：参数已经完整
/// 流完，但适配层没有把它从累加缓冲解析成工具参数。
fn anthropic_tool_call_events(name: &str, arguments: &str, close_block: bool) -> String {
    anthropic_tool_blocks_events(&[AnthropicToolBlock {
        id: "call_1",
        name,
        arguments,
        close: if close_block {
            BlockClose::Stop
        } else {
            BlockClose::Missing
        },
    }])
}

#[derive(Clone, Copy)]
enum BlockClose {
    Stop,
    Missing,
}

struct AnthropicToolBlock<'a> {
    id: &'a str,
    name: &'a str,
    arguments: &'a str,
    close: BlockClose,
}

fn anthropic_tool_blocks_events(blocks: &[AnthropicToolBlock<'_>]) -> String {
    let mut messages = vec![
        json!({"type":"message_start","message":{"id":"msg_1","role":"assistant","content":[],"model":"fixture","usage":{"input_tokens":10,"output_tokens":0}}}),
    ];
    for (index, block) in blocks.iter().enumerate() {
        let index = index as u64;
        messages.push(json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":block.id,"name":block.name,"input":{}}}));
        messages.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":block.arguments}}));
        match block.close {
            BlockClose::Stop => messages.push(json!({"type":"content_block_stop","index":index})),
            BlockClose::Missing => {}
        }
    }
    messages.push(json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":2}}));
    messages.push(json!({"type":"message_stop"}));
    anthropic_sse(&messages)
}

fn anthropic_text_events(text: &str) -> String {
    anthropic_sse(&[
        json!({"type":"message_start","message":{"id":"msg_2","role":"assistant","content":[],"model":"fixture","usage":{"input_tokens":10,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ])
}

fn chat_sse(chunks: &[Value], done: bool) -> String {
    let mut stream = String::new();
    for chunk in chunks {
        stream.push_str(&format!("data: {chunk}\n\n"));
    }
    if done {
        stream.push_str("data: [DONE]\n\n");
    }
    stream
}

fn chat_tool_call_delta(
    index: u32,
    id: Option<&str>,
    name: Option<&str>,
    arguments: Option<&str>,
) -> Value {
    let mut function = serde_json::Map::new();
    if let Some(name) = name {
        function.insert("name".into(), name.into());
    }
    if let Some(arguments) = arguments {
        function.insert("arguments".into(), arguments.into());
    }
    let mut call = serde_json::Map::new();
    call.insert("index".into(), index.into());
    if let Some(id) = id {
        call.insert("id".into(), id.into());
    }
    call.insert("type".into(), "function".into());
    call.insert("function".into(), Value::Object(function));
    json!({"choices":[{"index":0,"delta":{"tool_calls":[Value::Object(call)]},"finish_reason":null}]})
}

fn chat_finish_chunk(reason: &str) -> Value {
    json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]})
}

fn chat_text_events(text: &str) -> String {
    chat_sse(
        &[
            json!({"choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}),
            chat_finish_chunk("stop"),
        ],
        true,
    )
}

/// 按顺序回应；请求没来齐时安静退出，让失败落在断言而不是夹具线程上。
fn serve_sse(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        'responses: for body in responses {
            let start = Instant::now();
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && start.elapsed() < Duration::from_secs(2) =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break 'responses,
                }
            };
            requests.push(read_request(&mut stream).1);
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
        requests
    });
    (format!("http://{address}/v1"), server)
}

fn run_against(
    address: String,
    protocol: Protocol,
    prompt: &str,
    workspace: &Arc<Mutex<AiWorkspace>>,
) -> RunOutput {
    run_against_with_history(address, protocol, prompt, Vec::new(), workspace)
}

fn run_against_with_history(
    address: String,
    protocol: Protocol,
    prompt: &str,
    history: Vec<Value>,
    workspace: &Arc<Mutex<AiWorkspace>>,
) -> RunOutput {
    let endpoint = Endpoint {
        protocol,
        name: "本地协议夹具".into(),
        base_url: address,
        model: "fixture".into(),
        ..Endpoint::default()
    };
    run_blocking(
        RunRequest {
            model: endpoint.model_config().unwrap(),
            api_key: String::new(),
            history,
            prompt: prompt.into(),
            attachments: Vec::new(),
            system: "测试".into(),
            max_turns: 3,
            timeout: Duration::from_secs(10),
        },
        workspace.clone(),
        RunControl::default(),
        |_| {},
    )
    .unwrap()
}

fn fixture_workspace() -> Arc<Mutex<AiWorkspace>> {
    Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![crate::ai::DocumentSnapshot {
                id: "note.md".into(),
                source: "fixture-content".into(),
            }],
        )
        .unwrap(),
    ))
}

/// 网关漏发 `content_block_stop` 时，参数留在未解析缓冲里，适配层把整轮判为
/// Error。这里不能把缓存事件重拼成工具调用：重拼出的调用一旦丢名字，就会被
/// 当成一次空名字的真实调用执行，界面只剩「Tool  not found」。错误必须原样
/// 带着工具名报给用户，历史里也不能出现空名字的调用。
#[test]
fn anthropic_missing_content_block_stop_reports_the_named_error_without_executing() {
    let (address, server) = serve_sse(vec![
        anthropic_tool_call_events("list_documents", "{}", false),
        anthropic_text_events("Done"),
    ]);
    let workspace = fixture_workspace();
    let output = run_against(address, Protocol::Messages, "列出工作区文档", &workspace);
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 1, "未执行的调用不能进入下一轮");
    let error = output.error.as_deref().unwrap_or_default();
    assert!(error.contains("list_documents"), "{error}");
    assert!(
        !serde_json::to_string(&output.history)
            .unwrap()
            .contains("Tool  not found"),
        "历史里不能出现空名字的工具调用：{output:?}"
    );
}

/// 参数是合法 JSON 但不是对象（双重编码）：工具本身返回参数错误，整轮继续。
#[test]
fn anthropic_non_object_arguments_become_a_tool_error_and_the_run_continues() {
    let (address, server) = serve_sse(vec![
        anthropic_tool_blocks_events(&[AnthropicToolBlock {
            id: "call_1",
            name: "read_document",
            arguments: r#""{\"path\":\"note.md\"}""#,
            close: BlockClose::Stop,
        }]),
        anthropic_text_events("Done"),
    ]);
    let workspace = fixture_workspace();
    let output = run_against(address, Protocol::Messages, "读取 note.md", &workspace);
    let requests = server.join().unwrap();
    assert!(output.error.is_none(), "{output:?}");
    assert_eq!(requests.len(), 2, "工具错误必须交回模型而不是结束整轮");
    let second = requests[1].to_string();
    assert!(second.contains("缺少参数：path"), "{second}");
    assert!(!second.contains("not executed"), "{second}");
}

/// Chat Completions 一轮多个工具调用、参数分多片到达：按 index 拼接后全部执行。
#[test]
fn chat_completions_executes_multiple_tool_calls_with_split_arguments() {
    let (address, server) = serve_sse(vec![
        chat_sse(
            &[
                chat_tool_call_delta(0, Some("call_1"), Some("read_document"), None),
                chat_tool_call_delta(0, None, None, Some("{\"path\":")),
                chat_tool_call_delta(0, None, None, Some("\"note.md\"}")),
                chat_tool_call_delta(
                    1,
                    Some("call_2"),
                    Some("get_outline"),
                    Some("{\"path\":\"note.md\"}"),
                ),
                chat_finish_chunk("tool_calls"),
            ],
            true,
        ),
        chat_text_events("Done"),
    ]);
    let workspace = fixture_workspace();
    let output = run_against(address, Protocol::ChatCompletions, "读取并列出标题", &workspace);
    let requests = server.join().unwrap();
    assert!(output.error.is_none(), "{output:?}");
    assert_eq!(requests.len(), 2);
    let second = requests[1].to_string();
    assert!(second.contains("fixture-content"), "{second}");
    assert!(second.contains("headings"), "{second}");
}

/// Chat Completions 一轮里一个合法、一个截断：合法的照常执行，坏的以工具
/// 错误交回模型，整轮不中断。
#[test]
fn chat_completions_executes_valid_calls_and_reports_the_malformed_one() {
    let (address, server) = serve_sse(vec![
        chat_sse(
            &[
                chat_tool_call_delta(
                    0,
                    Some("call_1"),
                    Some("read_document"),
                    Some("{\"path\":\"note.md\"}"),
                ),
                chat_tool_call_delta(
                    1,
                    Some("call_2"),
                    Some("search_documents"),
                    Some("{\"query\":"),
                ),
                chat_finish_chunk("tool_calls"),
            ],
            true,
        ),
        chat_text_events("Done"),
    ]);
    let workspace = fixture_workspace();
    let output = run_against(address, Protocol::ChatCompletions, "读取并搜索", &workspace);
    let requests = server.join().unwrap();
    assert!(output.error.is_none(), "{output:?}");
    assert_eq!(requests.len(), 2);
    let second = requests[1].to_string();
    assert!(second.contains("fixture-content"), "{second}");
    assert!(second.contains("cut off"), "{second}");
}

/// 网关先给出工具名，随后又用空名字覆盖：适配层最终拿到空名字，但流里的
/// 非空名字必须还原，工具照常执行。
#[test]
fn chat_completions_restores_a_tool_name_overwritten_by_an_empty_delta() {
    let (address, server) = serve_sse(vec![
        chat_sse(
            &[
                chat_tool_call_delta(0, Some("call_1"), Some("read_document"), Some("")),
                chat_tool_call_delta(0, None, Some(""), Some(r#"{"path":"note.md"}"#)),
                chat_finish_chunk("tool_calls"),
            ],
            true,
        ),
        chat_text_events("Done"),
    ]);
    let workspace = fixture_workspace();
    let output = run_against(address, Protocol::ChatCompletions, "读取 note.md", &workspace);
    let requests = server.join().unwrap();
    assert!(output.error.is_none(), "{output:?}");
    assert_eq!(requests.len(), 2, "还原名字后工具必须真的执行");
    assert!(
        requests[1].to_string().contains("fixture-content"),
        "第二轮请求必须带着工具结果：{}",
        requests[1]
    );
}

/// 网关完全不转发 `function.name`：参数与工具 schema 唯一匹配时必须推断出
/// 名字并执行，而不是把空名字交给 agent loop。
#[test]
fn chat_completions_infers_a_missing_tool_name_from_arguments() {
    let (address, server) = serve_sse(vec![
        chat_sse(
            &[
                chat_tool_call_delta(0, Some("call_1"), None, Some(r#"{"path":"note.md"}"#)),
                chat_finish_chunk("tool_calls"),
            ],
            true,
        ),
        chat_text_events("Done"),
    ]);
    let workspace = fixture_workspace();
    let output = run_against(address, Protocol::ChatCompletions, "读取 note.md", &workspace);
    let requests = server.join().unwrap();
    assert!(output.error.is_none(), "{output:?}");
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1].to_string().contains("fixture-content"),
        "{}",
        requests[1]
    );
}

/// 名字缺失且参数无法对应任何工具：不能以空名字执行，历史里也不能出现
/// 「Tool  not found」；本轮直接结束并留下可读原因。
#[test]
fn chat_completions_never_executes_a_nameless_tool_call() {
    let (address, server) = serve_sse(vec![chat_sse(
        &[
            chat_tool_call_delta(0, Some("call_1"), None, Some(r#"{"zzz":1}"#)),
            chat_finish_chunk("tool_calls"),
        ],
        true,
    )]);
    let workspace = fixture_workspace();
    let output = run_against(address, Protocol::ChatCompletions, "做点事", &workspace);
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 1, "未执行的调用不能进入下一轮");
    let history = serde_json::to_string(&output.history).unwrap();
    assert!(!history.contains("Tool  not found"), "{history}");
    assert!(history.contains("缺少名称"), "{history}");
}

/// 有的网关只发 finish_reason 就关闭连接，不发 `[DONE]`；响应仍按完成处理，
/// 不能挂着等终止符。
#[test]
fn chat_completions_finishes_when_a_gateway_closes_without_done() {
    let (address, server) = serve_sse(vec![chat_sse(
        &[
            json!({"choices":[{"index":0,"delta":{"content":"Done"},"finish_reason":null}]}),
            chat_finish_chunk("stop"),
        ],
        false,
    )]);
    let workspace = fixture_workspace();
    let output = run_against(address, Protocol::ChatCompletions, "打招呼", &workspace);
    let requests = server.join().unwrap();
    assert!(output.error.is_none(), "{output:?}");
    assert_eq!(requests.len(), 1);
    assert!(
        serde_json::to_string(&output.history)
            .unwrap()
            .contains("Done"),
        "{output:?}"
    );
}

fn verify_many_plain_turns(protocol: Protocol) {
    let responses: Vec<String> = (0..5)
        .map(|turn| match protocol {
            Protocol::Messages => anthropic_text_events(&format!("回答{turn}")),
            Protocol::ChatCompletions => chat_text_events(&format!("回答{turn}")),
            _ => panic!("只覆盖两种对话协议"),
        })
        .collect();
    let (address, server) = serve_sse(responses);
    let workspace = fixture_workspace();
    let mut history = Vec::new();
    let mut previous_messages: Option<Value> = None;
    for turn in 0..5 {
        let output = run_against_with_history(
            address.clone(),
            protocol,
            &format!("问题{turn}"),
            history,
            &workspace,
        );
        assert!(output.error.is_none(), "{output:?}");
        history = output.history;
    }
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 5);
    for (turn, body) in requests.iter().enumerate() {
        let messages = without_cache_breakpoints(body.get("messages").unwrap());
        if let Some(previous) = &previous_messages {
            assert!(
                messages
                    .as_array()
                    .unwrap()
                    .starts_with(previous.as_array().unwrap()),
                "第 {turn} 轮请求只能追加历史，不能重写前缀"
            );
        }
        let last = messages.as_array().unwrap().last().unwrap();
        assert_eq!(last["role"], "user", "第 {turn} 轮最后一条必须是新问题");
        assert!(
            last["content"].to_string().contains(&format!("问题{turn}")),
            "第 {turn} 轮问题丢失或错位：{last}"
        );
        previous_messages = Some(messages);
    }
}

#[test]
fn ai_messages_keeps_many_plain_turns_append_only() {
    verify_many_plain_turns(Protocol::Messages);
}

#[test]
fn ai_chat_completions_keeps_many_plain_turns_append_only() {
    verify_many_plain_turns(Protocol::ChatCompletions);
}

