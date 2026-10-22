use super::*;
use crate::ai::DocumentSnapshot;
use serde_json::json;
use std::time::Duration;
use yoagent::provider::{
    MockProvider,
    mock::{MockResponse, MockToolCall},
};

fn request() -> RunRequest {
    RunRequest {
        model: ModelConfig::custom(
            yoagent::provider::ApiProtocol::OpenAiCompletions,
            "mock",
            "http://localhost",
            "mock",
            "mock",
        ),
        api_key: String::new(),
        history: vec![],
        prompt: "修订这篇文章后再次检查".into(),
        attachments: Vec::new(),
        system: "文档助手".into(),
        max_turns: 32,
        timeout: Duration::from_secs(600),
    }
}

fn call(name: &str, arguments: Value) -> MockResponse {
    MockResponse::ToolCalls(vec![MockToolCall {
        name: name.into(),
        arguments,
        provider_metadata: None,
    }])
}

#[test]
fn ai_runtime_drives_multiple_tools_and_revises_pending_content_without_writing() {
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "note.md".into(),
                source: "旧文字".into(),
            }],
        )
        .unwrap(),
    ));
    let provider = MockProvider::new(vec![
        call("read_document", json!({"path":"note.md"})),
        call(
            "propose_edit",
            json!({"path":"note.md","before":"旧文字","after":"第一稿"}),
        ),
        call("read_document", json!({"path":"note.md"})),
        call(
            "propose_edit",
            json!({"path":"note.md","before":"第一稿","after":"最终稿"}),
        ),
        MockResponse::Text("已提出修改，等待审阅。".into()),
    ]);
    let mut events = Vec::new();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime
        .block_on(run_with_provider(
            request(),
            workspace.clone(),
            RunControl::default(),
            Arc::new(provider),
            |event| events.push(event),
        ))
        .unwrap();
    assert!(result.history.len() > 5, "必须实际驱动多轮模型和工具调用");
    assert!(result.error.is_none(), "{result:?}");
    let workspace = workspace.lock().unwrap();
    assert_eq!(&*workspace.documents["note.md"].source, "旧文字");
    assert_eq!(&*workspace.changes.proposals["note.md"].proposed, "最终稿");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RunEvent::Text(text) if text.contains("等待审阅")))
    );
}

#[test]
fn ai_runtime_keeps_a_multi_turn_conversation_and_pending_proposals() {
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "note.md".into(),
                source: "原文".into(),
            }],
        )
        .unwrap(),
    ));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let prompts = ["先读 note.md", "把原文改成第一稿", "再读一次确认", "总结一下"];
    let responses = vec![
        vec![
            call("read_document", json!({"path":"note.md"})),
            MockResponse::Text("第一轮完成".into()),
        ],
        vec![
            call(
                "propose_edit",
                json!({"path":"note.md","before":"原文","after":"第一稿"}),
            ),
            MockResponse::Text("第二轮完成".into()),
        ],
        vec![
            call("read_document", json!({"path":"note.md"})),
            MockResponse::Text("第三轮完成".into()),
        ],
        vec![MockResponse::Text("总结完成".into())],
    ];
    let mut history: Vec<Value> = Vec::new();
    for (prompt, responses) in prompts.iter().zip(responses) {
        let provider = MockProvider::new(responses);
        let output = runtime
            .block_on(run_with_provider(
                RunRequest {
                    history: history.clone(),
                    prompt: (*prompt).into(),
                    attachments: Vec::new(),
                    ..request()
                },
                workspace.clone(),
                RunControl::default(),
                Arc::new(provider),
                |_| {},
            ))
            .unwrap();
        assert!(output.error.is_none(), "{output:?}");
        assert!(
            output.history.len() > history.len(),
            "「{prompt}」必须追加历史"
        );
        assert_eq!(
            &output.history[..history.len()],
            &history[..],
            "「{prompt}」不能重写之前的历史"
        );
        history = output.history;
    }
    let serialized = serde_json::to_string(&history).unwrap();
    for prompt in prompts {
        assert!(serialized.contains(prompt), "「{prompt}」必须留在历史里");
    }
    for text in ["第一轮完成", "第二轮完成", "第三轮完成", "总结完成"] {
        assert!(serialized.contains(text), "「{text}」必须留在历史里");
    }
    let workspace = workspace.lock().unwrap();
    let proposal = workspace
        .changes
        .proposals
        .get("note.md")
        .expect("提议必须跨轮保留");
    assert_eq!(&*proposal.proposed, "第一稿");
    let reads: Vec<_> = history
        .iter()
        .filter(|message| {
            message.get("role").and_then(Value::as_str) == Some("toolResult")
                && message.get("toolName").and_then(Value::as_str) == Some("read_document")
        })
        .collect();
    assert_eq!(reads.len(), 2, "两轮读取都必须留下结果：{history:?}");
    assert!(
        reads[1]["content"].to_string().contains("第一稿"),
        "第三轮读取必须看到待审阅版本：{:?}",
        reads[1]
    );
}

#[test]
fn ai_runtime_clears_the_activity_label_when_a_tool_finishes() {
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "note.md".into(),
                source: "正文".into(),
            }],
        )
        .unwrap(),
    ));
    let provider = MockProvider::new(vec![
        call("read_document", json!({"path":"note.md"})),
        MockResponse::Text("已阅读。".into()),
    ]);
    let mut events = Vec::new();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime
        .block_on(run_with_provider(
            request(),
            workspace,
            RunControl::default(),
            Arc::new(provider),
            |event| events.push(event),
        ))
        .unwrap();
    assert!(result.error.is_none(), "{result:?}");
    let activity = events
        .iter()
        .position(|event| matches!(event, RunEvent::Activity(name) if name == "read_document"))
        .expect("工具开始必须设置活动文案");
    let done = events
        .iter()
        .position(|event| matches!(event, RunEvent::ActivityDone))
        .expect("工具结束必须清除活动文案");
    let answered = events
        .iter()
        .rposition(|event| matches!(event, RunEvent::Text(text) if text.contains("已阅读")))
        .expect("模型必须给出回答");
    assert!(
        activity < done && done < answered,
        "工具结束到回答之间活动文案必须清空"
    );
}

/// 工具轨迹要能显示名称、参数、结果与成败：开始/结束事件必须成对，
/// 失败调用也不能只发 Activity 而没有结果。
#[test]
fn ai_runtime_reports_tool_call_arguments_and_results_for_the_trace() {
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "note.md".into(),
                source: "正文".into(),
            }],
        )
        .unwrap(),
    ));
    let provider = MockProvider::new(vec![
        call("read_document", json!({"path":"note.md"})),
        call("read_document", json!({})),
        MockResponse::Text("完成。".into()),
    ]);
    let mut events = Vec::new();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime
        .block_on(run_with_provider(
            request(),
            workspace,
            RunControl::default(),
            Arc::new(provider),
            |event| events.push(event),
        ))
        .unwrap();
    assert!(result.error.is_none(), "{result:?}");

    let starts = events
        .iter()
        .filter_map(|event| match event {
            RunEvent::ToolCallStart { id, name, arguments } if name == "read_document" => {
                Some((id.clone(), arguments.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 2, "两次读取都要有开始事件：{events:?}");
    assert!(starts[0].1.contains("note.md"), "参数摘要要带路径：{starts:?}");

    let ends = events
        .iter()
        .filter_map(|event| match event {
            RunEvent::ToolCallEnd {
                id,
                name,
                ok,
                result,
            } if name == "read_document" => Some((id.clone(), *ok, result.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ends.len(), 2, "两次读取都要有结束事件：{events:?}");
    assert_eq!(starts[0].0, ends[0].0, "开始与结束必须按 id 配对");
    assert_eq!(starts[1].0, ends[1].0, "开始与结束必须按 id 配对");
    assert!(ends[0].1, "首次读取应成功");
    assert!(ends[0].2.contains("正文"), "结果摘要要带正文：{:?}", ends[0]);
    assert!(!ends[1].1, "缺少路径应失败");
    assert!(!ends[1].2.is_empty(), "失败也要给出原因：{:?}", ends[1]);
}

#[test]
fn ai_runtime_executes_multiple_tools_in_one_turn() {
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "note.md".into(),
                source: "正文".into(),
            }],
        )
        .unwrap(),
    ));
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "read_document".into(),
                arguments: json!({"path":"note.md"}),
                provider_metadata: None,
            },
            MockToolCall {
                name: "get_outline".into(),
                arguments: json!({"path":"note.md"}),
                provider_metadata: None,
            },
        ]),
        MockResponse::Text("Done".into()),
    ]);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime
        .block_on(run_with_provider(
            request(),
            workspace,
            RunControl::default(),
            Arc::new(provider),
            |_| {},
        ))
        .unwrap();
    assert!(result.error.is_none(), "{result:?}");
    let history = serde_json::to_string(&result.history).unwrap();
    assert!(history.contains("正文"), "{history}");
    assert!(history.contains("headings"), "{history}");
}

#[test]
fn ai_runtime_unknown_tool_becomes_an_error_result_and_the_run_continues() {
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "note.md".into(),
                source: "正文".into(),
            }],
        )
        .unwrap(),
    ));
    let provider = MockProvider::new(vec![
        call("missing_tool", json!({})),
        MockResponse::Text("Done".into()),
    ]);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime
        .block_on(run_with_provider(
            request(),
            workspace,
            RunControl::default(),
            Arc::new(provider),
            |_| {},
        ))
        .unwrap();
    assert!(result.error.is_none(), "{result:?}");
    let history = serde_json::to_string(&result.history).unwrap();
    assert!(history.contains("Tool missing_tool not found"), "{history}");
}

#[test]
fn ai_runtime_missing_required_argument_becomes_a_tool_error_and_the_run_continues() {
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "note.md".into(),
                source: "正文".into(),
            }],
        )
        .unwrap(),
    ));
    let provider = MockProvider::new(vec![
        call("search_documents", json!({})),
        MockResponse::Text("Done".into()),
    ]);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime
        .block_on(run_with_provider(
            request(),
            workspace,
            RunControl::default(),
            Arc::new(provider),
            |_| {},
        ))
        .unwrap();
    assert!(result.error.is_none(), "{result:?}");
    let history = serde_json::to_string(&result.history).unwrap();
    assert!(history.contains("缺少参数：query"), "{history}");
}

#[test]
fn ai_runtime_a_failed_tool_does_not_skip_the_next_one() {
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "note.md".into(),
                source: "正文".into(),
            }],
        )
        .unwrap(),
    ));
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![
            MockToolCall {
                name: "read_document".into(),
                arguments: json!({"path":"missing.md"}),
                provider_metadata: None,
            },
            MockToolCall {
                name: "get_outline".into(),
                arguments: json!({"path":"note.md"}),
                provider_metadata: None,
            },
        ]),
        MockResponse::Text("Done".into()),
    ]);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime
        .block_on(run_with_provider(
            request(),
            workspace,
            RunControl::default(),
            Arc::new(provider),
            |_| {},
        ))
        .unwrap();
    assert!(result.error.is_none(), "{result:?}");
    let results: Vec<_> = result
        .history
        .iter()
        .filter(|message| {
            message.get("role").and_then(Value::as_str) == Some("toolResult")
        })
        .collect();
    assert_eq!(results.len(), 2, "两个工具都必须留下结果：{result:?}");
    assert_eq!(
        results
            .iter()
            .filter(|message| message.get("isError").and_then(Value::as_bool) == Some(true))
            .count(),
        1,
        "第一个工具必须失败，第二个不能跟着被跳过：{result:?}"
    );
    assert!(
        serde_json::to_string(&result.history)
            .unwrap()
            .contains("headings"),
        "{result:?}"
    );
}

#[test]
fn ai_runtime_cancelled_before_start_does_not_execute_tools() {
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "note.md".into(),
                source: "原文".into(),
            }],
        )
        .unwrap(),
    ));
    let control = RunControl::default();
    control.cancel.cancel();
    let provider = MockProvider::new(vec![call(
        "propose_edit",
        json!({"path":"note.md","before":"原文","after":"错误修改"}),
    )]);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime
        .block_on(run_with_provider(
            request(),
            workspace.clone(),
            control,
            Arc::new(provider),
            |_| {},
        ))
        .unwrap();
    assert!(result.error.is_some(), "停止必须与成功完成区别");
    assert!(workspace.lock().unwrap().changes.proposals.is_empty());
}

#[test]
fn ai_runtime_configured_backend_reads_proposes_and_verifies_an_exact_change() {
    use crate::ai::settings::AiSettings;
    let workspace = Arc::new(Mutex::new(
        AiWorkspace::new(
            None,
            vec![DocumentSnapshot {
                id: "fixture.md".into(),
                source: "# Test\n\nsource-before-marker\n".into(),
            }],
        )
        .unwrap(),
    ));
    let mut request = request();
    request.prompt = "This is an isolated document editing test. First read fixture.md using read_document. Use propose_edit to replace exactly source-before-marker with source-after-marker. Read fixture.md again to verify the pending change. Do not search other documents. Finally reply Done.".into();
    request.system = "You are a Markdown editing assistant. Use the provided document tools. All edits remain pending review; do not claim they are saved.".into();
    let provider: Arc<dyn StreamProvider> = if let Some(path) =
        std::env::var_os("VELORA_AI_LIVE_CONFIG")
    {
        let settings = AiSettings::load_at(std::path::Path::new(&path)).unwrap();
        let endpoint = settings.active_endpoint().unwrap();
        request.model = endpoint.model_config().unwrap();
        request.api_key =
            std::env::var("VELORA_AI_LIVE_KEY").unwrap_or_else(|_| endpoint.api_key.clone());
        eprintln!("真实协议验证：{} / {}", endpoint.name, endpoint.model);
        yoagent::provider::ProviderRegistry::default()
            .resolve(&request.model.api)
            .unwrap()
    } else {
        Arc::new(MockProvider::new(vec![
            call("read_document", json!({"path":"fixture.md"})),
            call(
                "propose_edit",
                json!({"path":"fixture.md","before":"source-before-marker","after":"source-after-marker"}),
            ),
            call("read_document", json!({"path":"fixture.md"})),
            MockResponse::Text("Done".into()),
        ]))
    };
    let output = if std::env::var_os("VELORA_AI_LIVE_CONFIG").is_some() {
        let workspace = workspace.clone();
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || run_blocking(request, workspace, RunControl::default(), |_| {}))
            .unwrap()
            .join()
            .unwrap()
            .unwrap()
    } else {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(run_with_provider(
                request,
                workspace.clone(),
                RunControl::default(),
                provider,
                |_| {},
            ))
            .unwrap()
    };
    assert!(output.error.is_none(), "{output:?}");
    let workspace = workspace.lock().unwrap();
    assert_eq!(
        &*workspace.documents["fixture.md"].source,
        "# Test\n\nsource-before-marker\n"
    );
    assert_eq!(
        &*workspace.changes.proposals["fixture.md"].proposed,
        "# Test\n\nsource-after-marker\n"
    );
    assert!(output.history.len() >= 5);
}

struct HangingProvider;
#[async_trait::async_trait]
impl StreamProvider for HangingProvider {
    async fn stream(
        &self,
        _config: yoagent::provider::StreamConfig,
        _sender: tokio::sync::mpsc::UnboundedSender<yoagent::provider::StreamEvent>,
        _cancel: CancellationToken,
    ) -> std::result::Result<Message, yoagent::provider::ProviderError> {
        std::future::pending().await
    }
}

#[test]
fn ai_runtime_deadline_stops_a_provider_that_does_not_finish_within_budget() {
    let workspace = Arc::new(Mutex::new(AiWorkspace::new(None, vec![]).unwrap()));
    let mut request = request();
    request.timeout = Duration::from_millis(1);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let output = runtime
        .block_on(run_with_provider(
            request,
            workspace,
            RunControl::default(),
            Arc::new(HangingProvider),
            |_| {},
        ))
        .unwrap();
    // 运行时发的是错误码，界面层按码取本地化文案。
    assert_eq!(output.error.as_deref(), Some(super::super::error_codes::TIMEOUT), "{output:?}");
}

#[test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
fn ai_runtime_timeout_during_a_tool_keeps_the_conversation_resumable() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let entered = Arc::new(AtomicBool::new(false));
    let control = RunControl::default();
    let mut documents = AiWorkspace::new(
        None,
        vec![DocumentSnapshot {
            id: "note.md".into(),
            source: "原文".into(),
        }],
    )
    .unwrap();
    documents.refresh.callback = Some(Arc::new({
        let entered = entered.clone();
        let cancel = control.cancel.clone();
        move |_| {
            entered.store(true, Ordering::SeqCst);
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            anyhow::bail!("任务已停止")
        }
    }));
    let workspace = Arc::new(Mutex::new(documents));
    let mut first_request = request();
    first_request.timeout = Duration::from_secs(2);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let first = runtime
        .block_on(run_with_provider(
            first_request,
            workspace.clone(),
            control,
            Arc::new(MockProvider::new(vec![call(
                "read_document",
                json!({"path":"note.md"}),
            )])),
            |_| {},
        ))
        .unwrap();
    assert!(
        entered.load(Ordering::SeqCst),
        "必须实际进入被中断的文档工具"
    );
    // 错误是语言中立的固定码（界面层再按码取词），断言比的是码本身。
    assert_eq!(
        first.error.as_deref(),
        Some(super::super::error_codes::TIMEOUT),
        "工具执行期间超时该报 TIMEOUT：{first:?}"
    );
    let mut resumed = request();
    resumed.history = first.history;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(run_with_provider(
            resumed,
            workspace,
            RunControl::default(),
            Arc::new(MockProvider::new(vec![MockResponse::Text(
                "继续完成".into(),
            )])),
            |_| {},
        ))
    }));
    assert!(result.is_ok(), "恢复请求不能包含未配对的工具消息");
    let second = result.unwrap().unwrap();
    assert!(
        second.error.is_none(),
        "超时后不能留下模型拒绝接收的工具消息序列：{second:?}"
    );
}
