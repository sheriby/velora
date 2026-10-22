//! Agent 运行时的依赖集中在此处，界面不持有框架实体。

use super::workspace::AiWorkspace;
use anyhow::{Context as _, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use yoagent::provider::{
    ApiProtocol, ModelConfig, ProviderError, StreamConfig, StreamEvent, StreamProvider,
    ToolDefinition,
};
use yoagent::{
    AgentContext, AgentEvent, AgentMessage, AgentTool, Content, Message, StopReason, StreamDelta,
    ToolContext, ToolError, ToolResult,
};

#[derive(Clone, Default)]
pub(crate) struct RunControl {
    pub cancel: CancellationToken,
    pub steering: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone, Debug)]
pub(crate) enum RunEvent {
    AssistantStart,
    Text(String),
    /// 模型的思考增量，只用于界面展示。
    Thinking(String),
    Activity(String),
    /// 工具或重试结束，界面不再显示上一次的活动文案。
    ActivityDone,
    /// 一次工具调用开始；参数已转成紧凑 JSON 并截断，供界面轨迹展示。
    ToolCallStart {
        id: String,
        name: String,
        arguments: String,
    },
    /// 工具调用结束；结果摘要只保留文本内容，完整结果仍在对话历史里。
    ToolCallEnd {
        id: String,
        name: String,
        ok: bool,
        result: String,
    },
    Checkpoint {
        workspace: Box<AiWorkspace>,
        history: Vec<Value>,
    },
}

/// 轨迹是给人快速扫一眼的，不替历史保存完整参数与结果。
const TOOL_TRACE_ARGUMENT_LIMIT: usize = 1_000;
const TOOL_TRACE_RESULT_LIMIT: usize = 4_000;

fn summarize_tool_arguments(arguments: &Value) -> String {
    let text = serde_json::to_string(arguments).unwrap_or_else(|_| "{}".into());
    truncate_for_trace(&text, TOOL_TRACE_ARGUMENT_LIMIT)
}

fn summarize_tool_result(result: &ToolResult) -> String {
    let text = result
        .content
        .iter()
        .filter_map(|content| match content {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    truncate_for_trace(&text, TOOL_TRACE_RESULT_LIMIT)
}

fn truncate_for_trace(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

#[derive(Default, Debug)]
pub(crate) struct RunOutput {
    pub history: Vec<Value>,
    pub error: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub(crate) struct RunRequest {
    pub model: ModelConfig,
    pub api_key: String,
    pub history: Vec<Value>,
    pub prompt: String,
    pub attachments: Vec<std::path::PathBuf>,
    pub system: String,
    pub max_turns: usize,
    pub timeout: std::time::Duration,
}

fn image_content(path: &std::path::Path) -> Result<Content> {
    let bytes = std::fs::read(path)?;
    if bytes.len() > 8 * 1024 * 1024 {
        anyhow::bail!("图片超过 8 MiB 发送预算");
    }
    let mime_type = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => anyhow::bail!("AI 目前只能发送 PNG、JPEG、GIF、WebP 图片"),
    };
    Ok(Content::Image {
        data: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
        mime_type: mime_type.to_owned(),
    })
}

pub(crate) fn run_blocking(
    request: RunRequest,
    workspace: Arc<Mutex<AiWorkspace>>,
    control: RunControl,
    emit: impl FnMut(RunEvent) + Send,
) -> Result<RunOutput> {
    // macOS 的 GCD 后台线程只有小栈；调试构建的 Agent/HTTP future
    // 不能在那条线程上 block_on，否则请求尚未发出就会栈溢出。
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("velora-ai".into())
            .stack_size(8 * 1024 * 1024)
            .spawn_scoped(scope, move || {
                let provider = yoagent::provider::ProviderRegistry::default()
                    .resolve(&request.model.api)
                    .context("不支持的模型协议")?;
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .thread_stack_size(8 * 1024 * 1024)
                    .enable_all()
                    .build()?;
                runtime.block_on(run_with_provider(
                    request, workspace, control, provider, emit,
                ))
            })
            .context("无法启动 AI 工作线程")?
            .join()
            .map_err(|payload| {
                // panic 的 payload 带着 true 原因；吞掉后持锁 panic 中毒的 Mutex
                // 只会让后续工具报「状态不可用」，无从排查。
                let reason = payload
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "未知原因".into());
                anyhow::anyhow!("AI 工作线程异常结束：{reason}")
            })?
    })
}

pub(crate) async fn run_with_provider(
    request: RunRequest,
    workspace: Arc<Mutex<AiWorkspace>>,
    control: RunControl,
    provider: Arc<dyn StreamProvider>,
    mut emit: impl FnMut(RunEvent),
) -> Result<RunOutput> {
    if control.cancel.is_cancelled() {
        return Ok(RunOutput {
            error: Some(super::error_codes::STOPPED.into()),
            ..RunOutput::default()
        });
    }
    let provider = if request.model.api == ApiProtocol::OpenAiCompletions {
        let provider: Arc<dyn StreamProvider> = Arc::new(ChatToolNameRepairProvider::new(provider));
        provider
    } else {
        provider
    };
    let history: Vec<AgentMessage> = request
        .history
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()?;
    let definitions = {
        let mut workspace = workspace
            .lock()
            .map_err(|_| anyhow::anyhow!("文档任务状态不可用"))?;
        // 把取消令牌注进工作区：超时/停止后，正在跑的同步工具在循环边界退出。
        workspace.cancel = Some(control.cancel.clone());
        super::workspace::tool_definitions()
            .into_iter()
            .filter(|(name, _, _)| workspace.tool_allowed(name))
            .collect::<Vec<_>>()
    };
    let tools = definitions
        .into_iter()
        .map(|(name, description, schema)| {
            Box::new(DocumentTool {
                name,
                description,
                schema,
                workspace: workspace.clone(),
            }) as Box<dyn AgentTool>
        })
        .collect();
    let mut observed_history = history.clone();
    let mut context = AgentContext {
        system_prompt: request.system,
        messages: history,
        tools,
    };
    let mut config = yoagent::agent_loop::AgentLoopConfig::new(provider, request.model.id.clone());
    let mut context_config = yoagent::context::ContextConfig::default();
    // 0 或极小的窗口会让压缩立即触发；给一个非零下限，配置异常时不至于
    // 每轮都被压缩打断。上限必须给补全预算留出空间：4096 窗口配 1024 补全时，
    // 再抬到 4096 会让提示 + 补全直接超窗。
    context_config.max_context_tokens = request
        .model
        .context_window
        .saturating_sub(request.model.max_tokens)
        .max(1024) as usize;
    config.max_tokens = Some(request.model.max_tokens);
    config.model_config = Some(request.model);
    config.api_key = request.api_key;
    config.tool_execution = yoagent::ToolExecutionStrategy::Sequential;
    config.context_config = Some(context_config);
    let mut limits = yoagent::context::ExecutionLimits::default();
    limits.max_turns = request.max_turns.clamp(1, 128);
    // 时长统一由外层 timeout 控制，涵盖等待模型响应；避免与轮间检查竞争。
    limits.max_duration = std::time::Duration::MAX;
    config.execution_limits = Some(limits);
    config.get_steering_messages = Some(Box::new({
        let steering = control.steering.clone();
        move || match steering.lock() {
            Ok(mut pending) => pending
                .drain(..)
                .map(|text| Message::user(text).into())
                .collect(),
            Err(error) => {
                eprintln!("读取 AI 补充指令失败：{error}");
                Vec::new()
            }
        }
    }));
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut initial = Message::user(request.prompt);
    if let Message::User { content, .. } = &mut initial {
        for path in request.attachments {
            content.push(image_content(&path)?);
        }
    }
    let runner = yoagent::agent_loop(
        vec![initial.into()],
        &mut context,
        &config,
        sender,
        control.cancel.clone(),
    );
    let observer = async {
        let mut output = RunOutput::default();
        while let Some(event) = receiver.recv().await {
            match event {
                AgentEvent::MessageStart {
                    message: AgentMessage::Llm(Message::Assistant { .. }),
                } => emit(RunEvent::AssistantStart),
                AgentEvent::MessageEnd {
                    message: message @ AgentMessage::Llm(Message::User { .. }),
                } => observed_history.push(message),
                AgentEvent::MessageUpdate {
                    delta: StreamDelta::Text { delta },
                    ..
                } => emit(RunEvent::Text(delta)),
                AgentEvent::MessageUpdate {
                    delta: StreamDelta::Thinking { delta },
                    ..
                } => emit(RunEvent::Thinking(delta)),
                AgentEvent::ToolExecutionStart {
                    tool_call_id,
                    tool_name,
                    args,
                } => {
                    emit(RunEvent::Activity(tool_name.clone()));
                    emit(RunEvent::ToolCallStart {
                        id: tool_call_id,
                        name: tool_name,
                        arguments: summarize_tool_arguments(&args),
                    });
                }
                AgentEvent::ToolExecutionEnd {
                    tool_call_id,
                    tool_name,
                    result,
                    is_error,
                } => {
                    emit(RunEvent::ActivityDone);
                    emit(RunEvent::ToolCallEnd {
                        id: tool_call_id,
                        name: tool_name,
                        ok: !is_error,
                        result: summarize_tool_result(&result),
                    });
                }
                AgentEvent::TurnEnd {
                    message,
                    tool_results,
                } => {
                    if matches!(message.as_llm(), Some(Message::Assistant { .. })) {
                        observed_history.push(message);
                    }
                    observed_history.extend(tool_results.into_iter().map(AgentMessage::from));
                    // 锁忙时跳过本轮检查点要留痕：进度持久化的缺失事后无法排查。
                    match workspace.try_lock() {
                        Ok(snapshot) => {
                            let mut checkpoint = snapshot.clone();
                            // 运行令牌只属于本次请求；检查点会写回界面会话，
                            // 留着会让停止后的资料面板误判任务仍在取消态。
                            checkpoint.cancel = None;
                            match observed_history
                                .iter()
                                .map(serde_json::to_value)
                                .collect::<Result<Vec<_>, _>>()
                            {
                                Ok(history) => emit(RunEvent::Checkpoint {
                                    workspace: Box::new(checkpoint),
                                    history,
                                }),
                                Err(error) => {
                                    output.error = Some(format!("保存任务进度失败：{error}"))
                                }
                            }
                        }
                        Err(error) => eprintln!("保存任务进度时读取文档状态失败，已跳过本轮检查点：{error}"),
                    }
                }
                AgentEvent::ProviderRetry { attempt, .. } => {
                    output.error = None;
                    emit(RunEvent::Activity(format!("重试连接（{attempt}）")));
                }
                AgentEvent::MessageEnd {
                    message:
                        AgentMessage::Llm(Message::Assistant {
                            stop_reason: StopReason::Length,
                            ..
                        }),
                } => {
                    // 截断是可见状态，不能只把半句话当成完整回答。
                    output.error = Some(super::error_codes::LENGTH.into());
                }
                AgentEvent::MessageEnd {
                    message:
                        AgentMessage::Llm(Message::Assistant {
                            stop_reason: StopReason::Error | StopReason::Refusal,
                            error_message,
                            ..
                        }),
                } => {
                    output.error = Some(super::error_codes::model_failed(error_message))
                }
                AgentEvent::AgentEnd { stats, .. } => {
                    output.input_tokens =
                        stats.usage.input + stats.usage.cache_read + stats.usage.cache_write;
                    output.output_tokens = stats.usage.output;
                }
                _ => {}
            }
        }
        output
    };
    let runner = async {
        if tokio::time::timeout(request.timeout, runner).await.is_err() {
            control.cancel.cancel();
            true
        } else {
            false
        }
    };
    let (timed_out, mut output) = futures::join!(runner, observer);
    if timed_out {
        output.error = Some(super::error_codes::TIMEOUT.into());
    } else if control.cancel.is_cancelled() {
        output.error = Some(super::error_codes::STOPPED.into());
    }
    if context.messages.last().is_some_and(|message| {
        matches!(message.as_llm(), Some(Message::User { content, .. }) if content.iter().any(|content| matches!(content, Content::Text { text } if text.starts_with(yoagent::agent_loop::AGENT_STOPPED_PREFIX))))
    }) && output.error.is_none() {
        output.error = Some(super::error_codes::BUDGET.into());
    }
    // 强制超时可能发生在 tool_use 与 tool_result 之间，只恢复完整轮次的检查点。
    let history = if timed_out {
        observed_history
    } else {
        context.messages
    };
    output.history = history
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<_, _>>()?;
    Ok(output)
}

pub(crate) fn test_connection(
    endpoint: super::settings::Endpoint,
    control: RunControl,
) -> Result<RunOutput> {
    let mut model = endpoint.request_config()?;
    model.max_tokens = 128;
    let workspace = Arc::new(Mutex::new(AiWorkspace::new(None, Vec::new())?));
    let output = run_blocking(RunRequest {
        model, api_key: endpoint.api_key.trim().to_owned(), history: Vec::new(),
        prompt: "Call list_documents once to verify tool calling, then reply OK. This is an empty isolated test workspace.".into(),
        attachments: Vec::new(),
        system: "You are a connectivity and tool-calling probe.".into(), max_turns: 4, timeout: std::time::Duration::from_secs(30),
    }, workspace, control, |_| {})?;
    if let Some(error) = &output.error {
        anyhow::bail!("{error}");
    }
    if !output.history.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("toolResult")
            && message.get("isError").and_then(Value::as_bool) == Some(false)
    }) {
        anyhow::bail!("模型未完成工具调用，请确认此模型支持工具能力");
    }
    Ok(output)
}

struct DocumentTool {
    name: &'static str,
    description: &'static str,
    schema: Value,
    workspace: Arc<Mutex<AiWorkspace>>,
}

#[async_trait::async_trait]
impl AgentTool for DocumentTool {
    fn name(&self) -> &str {
        self.name
    }
    fn label(&self) -> &str {
        self.description
    }
    fn description(&self) -> &str {
        self.description
    }
    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }

    async fn execute(&self, params: Value, context: ToolContext) -> Result<ToolResult, ToolError> {
        if context.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let workspace = self.workspace.clone();
        let name = self.name;
        let result = tokio::task::spawn_blocking(move || {
            if context.cancel.is_cancelled() {
                anyhow::bail!("任务已停止");
            }
            let mut workspace = workspace
                .lock()
                .map_err(|_| anyhow::anyhow!("文档任务状态不可用"))?;
            workspace.call(name, &params)
        })
        .await
        .context("文档工具执行失败")
        .and_then(|result| result)
        .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(ToolResult {
            content: vec![Content::Text {
                text: result.to_string(),
            }],
            details: Value::Null,
        })
    }
}

#[derive(Default)]
struct TrackedToolName {
    id: String,
    name: String,
}

/// Chat Completions 网关可能完全不转发 `function.name`，或者在中途用空字符串
/// 覆盖它；适配层最终给出名字为空的工具调用，agent loop 按名字查工具只能得到
/// 「Tool  not found」。在消息交给 loop 之前补齐名字，其余内容与事件原样透传。
struct ChatToolNameRepairProvider {
    inner: Arc<dyn StreamProvider>,
}

impl ChatToolNameRepairProvider {
    fn new(inner: Arc<dyn StreamProvider>) -> Self {
        Self { inner }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl StreamProvider for ChatToolNameRepairProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<Message, ProviderError> {
        let tools = config.tools.clone();
        let (inner_tx, mut inner_rx) = tokio::sync::mpsc::unbounded_channel();
        let stream = self.inner.stream(config, inner_tx, cancel);
        tokio::pin!(stream);
        let mut names = BTreeMap::new();
        let mut closed = false;
        let mut message = loop {
            tokio::select! {
                biased;
                result = &mut stream => {
                    while let Ok(event) = inner_rx.try_recv() {
                        track_tool_name(&event, &mut names);
                        let _ = tx.send(event);
                    }
                    break result?;
                }
                event = inner_rx.recv(), if !closed => match event {
                    Some(event) => {
                        track_tool_name(&event, &mut names);
                        let _ = tx.send(event);
                    }
                    None => closed = true,
                },
            }
        };
        repair_tool_call_names(&mut message, &names, &tools);
        Ok(message)
    }

    fn protocol(&self) -> Option<ApiProtocol> {
        self.inner.protocol()
    }
}

fn track_tool_name(event: &StreamEvent, names: &mut BTreeMap<usize, TrackedToolName>) {
    if let StreamEvent::ToolCallStart {
        content_index,
        id,
        name,
    } = event
    {
        let tracked = names.entry(*content_index).or_default();
        if tracked.id.is_empty() {
            tracked.id = id.clone();
        }
        // 网关可能先给名字、再补一条空名字；空值不能覆盖已经拿到的名字。
        if !name.is_empty() {
            tracked.name = name.clone();
        }
    }
}

fn repair_tool_call_names(
    message: &mut Message,
    names: &BTreeMap<usize, TrackedToolName>,
    tools: &[ToolDefinition],
) {
    let Message::Assistant {
        content,
        stop_reason,
        ..
    } = message
    else {
        return;
    };
    let tracked: Vec<&TrackedToolName> = names.values().collect();
    let mut ordinal = 0;
    for block in content.iter_mut() {
        let Content::ToolCall {
            id,
            name,
            arguments,
            ..
        } = block
        else {
            continue;
        };
        ordinal += 1;
        if name.is_empty() {
            *name = tracked
                .get(ordinal - 1)
                .filter(|tracked| !tracked.name.is_empty())
                .map(|tracked| tracked.name.clone())
                .or_else(|| {
                    tracked
                        .iter()
                        .find(|tracked| {
                            !tracked.id.is_empty()
                                && !tracked.name.is_empty()
                                && tracked.id == *id
                        })
                        .map(|tracked| tracked.name.clone())
                })
                .or_else(|| infer_tool_name(arguments, tools))
                .unwrap_or_default();
        }
        if id.is_empty() {
            *id = format!("call_{ordinal}");
        }
    }
    if !content
        .iter()
        .any(|block| matches!(block, Content::ToolCall { name, .. } if name.is_empty()))
    {
        return;
    }
    *content = content
        .drain(..)
        .map(|block| match block {
            Content::ToolCall { name, arguments, .. } if name.is_empty() => Content::Text {
                text: format!("[工具调用缺少名称，本轮未执行；参数：{arguments}]"),
            },
            block => block,
        })
        .collect();
    if matches!(stop_reason, StopReason::ToolUse) {
        *stop_reason = StopReason::Stop;
    }
}

fn infer_tool_name(arguments: &Value, tools: &[ToolDefinition]) -> Option<String> {
    let object = arguments.as_object()?;
    // 有的网关把整次调用包进参数：{"name": "...", "arguments": {...}}。
    if let Some(Value::String(name)) = object.get("name") {
        if tools.iter().any(|tool| tool.name == *name) {
            return Some(name.clone());
        }
    }
    let mut candidates = Vec::new();
    for tool in tools {
        let Some(properties) = tool.parameters.get("properties").and_then(Value::as_object) else {
            continue;
        };
        let required = tool
            .parameters
            .get("required")
            .and_then(Value::as_array)
            .map(|fields| fields.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        if !required.iter().all(|field| object.contains_key(*field)) {
            continue;
        }
        if !object.keys().all(|key| properties.contains_key(key)) {
            continue;
        }
        candidates.push(tool);
    }
    // {"path": ...} 同时符合 read_document、get_outline 与 reset_proposal，
    // 参数本身无法区分；读取正文最常见，按这个顺序兜底。
    for preferred in ["read_document", "get_outline", "reset_proposal"] {
        if candidates.iter().any(|tool| tool.name == preferred) {
            return Some(preferred.to_string());
        }
    }
    candidates.first().map(|tool| tool.name.clone())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod protocol_tests;
