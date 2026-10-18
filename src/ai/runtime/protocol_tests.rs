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
