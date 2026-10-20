//! Agent 运行时的依赖集中在此处，界面不持有框架实体。

use super::workspace::AiWorkspace;
use anyhow::{Context as _, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use yoagent::provider::{
    ApiProtocol, ModelConfig, ProviderError, StreamConfig, StreamEvent, StreamProvider,
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
    Activity(String),
    /// 工具或重试结束，界面不再显示上一次的活动文案。
    ActivityDone,
    Checkpoint {
        workspace: Box<AiWorkspace>,
        history: Vec<Value>,
    },
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
    pub system: String,
    pub max_turns: usize,
    pub timeout: std::time::Duration,
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
    let provider: Arc<dyn StreamProvider> = Arc::new(ToolArgumentRecoveryProvider::new(provider));
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
    // 每轮都被压缩打断。
    context_config.max_context_tokens = request
        .model
        .context_window
        .saturating_sub(request.model.max_tokens)
        .max(4096) as usize;
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
    let runner = yoagent::agent_loop(
        vec![Message::user(request.prompt).into()],
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
                AgentEvent::ToolExecutionStart { tool_name, .. } => {
                    emit(RunEvent::Activity(tool_name))
                }
                AgentEvent::ToolExecutionEnd { .. } => emit(RunEvent::ActivityDone),
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
                            match observed_history
                                .iter()
                                .map(serde_json::to_value)
                                .collect::<Result<Vec<_>, _>>()
                            {
                                Ok(history) => emit(RunEvent::Checkpoint {
                                    workspace: Box::new(snapshot.clone()),
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

struct TrackedToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Anthropic 适配层在 `content_block_stop` 缺失或没有 index 时，会把已经流完
/// 的参数留在累加缓冲里，并把整轮判为 Error；agent loop 见到 Error 直接结束。
/// 这里在 provider 与 loop 之间补两层：参数可解析的调用还原后继续执行，
/// 真正截断的调用追加一条纠正提示重试一次，用户不再看到整轮 abort。
struct ToolArgumentRecoveryProvider {
    inner: Arc<dyn StreamProvider>,
    stall_timeout: std::time::Duration,
}

impl ToolArgumentRecoveryProvider {
    fn new(inner: Arc<dyn StreamProvider>) -> Self {
        Self {
            inner,
            stall_timeout: std::time::Duration::from_secs(120),
        }
    }

    #[cfg(test)]
    fn with_stall_timeout(
        inner: Arc<dyn StreamProvider>,
        stall_timeout: std::time::Duration,
    ) -> Self {
        Self {
            inner,
            stall_timeout,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl StreamProvider for ToolArgumentRecoveryProvider {
    async fn stream(
        &self,
        mut config: StreamConfig,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<Message, ProviderError> {
        let (mut message, calls) = forward_provider_stream(
            self.inner.clone(),
            config.clone(),
            &tx,
            cancel.clone(),
            self.stall_timeout,
        )
        .await?;
        if salvage_unusable_tool_calls(&mut message, &calls) {
            let _ = tx.send(StreamEvent::Done {
                message: message.clone(),
            });
            return Ok(message);
        }
        if !unusable_tool_arguments(&message)
            || cancel.is_cancelled()
            || !push_recovery_note(&mut config)
        {
            let _ = tx.send(StreamEvent::Done {
                message: message.clone(),
            });
            return Ok(message);
        }
        let (mut retried, _) =
            forward_provider_stream(self.inner.clone(), config, &tx, cancel, self.stall_timeout)
                .await?;
        carry_usage(&mut retried, &message);
        let _ = tx.send(StreamEvent::Done {
            message: retried.clone(),
        });
        Ok(retried)
    }

    fn protocol(&self) -> Option<ApiProtocol> {
        self.inner.protocol()
    }
}

async fn forward_provider_stream(
    inner: Arc<dyn StreamProvider>,
    config: StreamConfig,
    tx: &tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    cancel: CancellationToken,
    stall_timeout: std::time::Duration,
) -> std::result::Result<(Message, BTreeMap<usize, TrackedToolCall>), ProviderError> {
    let (inner_tx, mut inner_rx) = tokio::sync::mpsc::unbounded_channel();
    // 子令牌只取消本次尝试：看门狗触发时不能把整轮任务标记为“已停止”。
    let attempt_cancel = cancel.child_token();
    let stream = inner.stream(config, inner_tx, attempt_cancel.clone());
    tokio::pin!(stream);
    let deadline = tokio::time::sleep(stall_timeout);
    tokio::pin!(deadline);
    let mut calls = BTreeMap::new();
    let mut done = None;
    let mut closed = false;
    loop {
        tokio::select! {
            biased;
            result = &mut stream => {
                while let Ok(event) = inner_rx.try_recv() {
                    track_stream_event(event, tx, &mut calls, &mut done);
                }
                let message = result?;
                return Ok((done.unwrap_or(message), calls));
            }
            event = inner_rx.recv(), if !closed => {
                deadline
                    .as_mut()
                    .reset(tokio::time::Instant::now() + stall_timeout);
                match event {
                    Some(event) => track_stream_event(event, tx, &mut calls, &mut done),
                    None => closed = true,
                }
            }
            _ = &mut deadline => {
                attempt_cancel.cancel();
                return Err(ProviderError::Network(format!(
                    "模型服务在 {stall_timeout:?} 内没有返回数据，已中断本次请求"
                )));
            }
        }
    }
}

fn track_stream_event(
    event: StreamEvent,
    tx: &tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    calls: &mut BTreeMap<usize, TrackedToolCall>,
    done: &mut Option<Message>,
) {
    match &event {
        StreamEvent::ToolCallStart {
            content_index,
            id,
            name,
        } => {
            calls.insert(
                *content_index,
                TrackedToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                },
            );
        }
        StreamEvent::ToolCallDelta {
            content_index,
            delta,
        } => {
            if let Some(call) = calls.get_mut(content_index) {
                call.arguments.push_str(delta);
            }
        }
        StreamEvent::Done { message } => {
            *done = Some(message.clone());
            return;
        }
        _ => {}
    }
    let _ = tx.send(event);
}

const UNUSABLE_PLACEHOLDER_PREFIX: &str = "[tool call `";
const UNUSABLE_PLACEHOLDER_SUFFIX: &str =
    "` not executed: the turn failed on unusable tool arguments]";

fn unusable_placeholder_tool(text: &str) -> Option<&str> {
    text.strip_prefix(UNUSABLE_PLACEHOLDER_PREFIX)?
        .strip_suffix(UNUSABLE_PLACEHOLDER_SUFFIX)
}

fn unusable_tool_arguments(message: &Message) -> bool {
    matches!(
        message,
        Message::Assistant {
            stop_reason: StopReason::Error,
            error_message: Some(detail),
            ..
        } if detail.contains("unusable arguments")
    )
}

/// 适配层已经判死的调用里，参数本身完整的那些还原成工具调用；返回是否至少
/// 还原了一个。没有还原任何调用时保持原消息，交给重试路径。
fn salvage_unusable_tool_calls(
    message: &mut Message,
    calls: &BTreeMap<usize, TrackedToolCall>,
) -> bool {
    let Message::Assistant {
        content,
        stop_reason,
        error_message,
        ..
    } = message
    else {
        return false;
    };
    let tracked: Vec<&TrackedToolCall> = calls.values().collect();
    let mut used = vec![false; tracked.len()];
    let mut rebuilt = Vec::with_capacity(content.len());
    let mut replaced = 0;
    for block in content.iter() {
        let Content::Text { text } = block else {
            rebuilt.push(block.clone());
            continue;
        };
        let Some(name) = unusable_placeholder_tool(text) else {
            rebuilt.push(block.clone());
            continue;
        };
        let Some(index) = tracked
            .iter()
            .enumerate()
            .find_map(|(index, call)| (!used[index] && call.name == name).then_some(index))
        else {
            rebuilt.push(block.clone());
            continue;
        };
        let call = tracked[index];
        let arguments = yoagent::provider::parse_tool_arguments(&call.arguments);
        if yoagent::provider::unparsed_tool_arguments(&arguments).is_some() {
            rebuilt.push(block.clone());
            continue;
        }
        used[index] = true;
        rebuilt.push(Content::tool_call(
            call.id.clone(),
            call.name.clone(),
            arguments,
        ));
        replaced += 1;
    }
    if replaced == 0 {
        return false;
    }
    *content = rebuilt;
    *stop_reason = StopReason::ToolUse;
    *error_message = None;
    true
}

const TOOL_ARGUMENT_RECOVERY_NOTE: &str = "上一次的工具调用参数不是合法 JSON，工具没有执行。请重新调用同一个工具，参数必须是完整的 JSON 对象（字符串内的换行与引号要转义），不要重复已经写过的说明。";

fn push_recovery_note(config: &mut StreamConfig) -> bool {
    let Some(Message::User { content, .. }) = config
        .messages
        .iter_mut()
        .rev()
        .find(|message| matches!(message, Message::User { .. }))
    else {
        return false;
    };
    content.push(Content::Text {
        text: TOOL_ARGUMENT_RECOVERY_NOTE.into(),
    });
    true
}

fn carry_usage(retried: &mut Message, failed: &Message) {
    if let (
        Message::Assistant {
            usage: retried_usage,
            ..
        },
        Message::Assistant {
            usage: failed_usage,
            ..
        },
    ) = (retried, failed)
    {
        retried_usage.input += failed_usage.input;
        retried_usage.output += failed_usage.output;
        retried_usage.cache_read += failed_usage.cache_read;
        retried_usage.cache_write += failed_usage.cache_write;
        retried_usage.total_tokens += failed_usage.total_tokens;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod protocol_tests;
