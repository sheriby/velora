//! Agent 运行时的依赖集中在此处，界面不持有框架实体。

use super::workspace::AiWorkspace;
use anyhow::{Context as _, Result};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use yoagent::provider::{ModelConfig, StreamProvider};
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
            .map_err(|_| anyhow::anyhow!("AI 工作线程异常结束"))?
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
            error: Some("任务已停止".into()),
            ..RunOutput::default()
        });
    }
    let history: Vec<AgentMessage> = request
        .history
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()?;
    let definitions = {
        let workspace = workspace
            .lock()
            .map_err(|_| anyhow::anyhow!("文档任务状态不可用"))?;
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
    context_config.max_context_tokens = request
        .model
        .context_window
        .saturating_sub(request.model.max_tokens) as usize;
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
                AgentEvent::TurnEnd {
                    message,
                    tool_results,
                } => {
                    if matches!(message.as_llm(), Some(Message::Assistant { .. })) {
                        observed_history.push(message);
                    }
                    observed_history.extend(tool_results.into_iter().map(AgentMessage::from));
                    if let Ok(snapshot) = workspace.try_lock() {
                        match observed_history
                            .iter()
                            .map(serde_json::to_value)
                            .collect::<Result<Vec<_>, _>>()
                        {
                            Ok(history) => emit(RunEvent::Checkpoint {
                                workspace: Box::new(snapshot.clone()),
                                history,
                            }),
                            Err(error) => output.error = Some(format!("保存任务进度失败：{error}")),
                        }
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
                    output.error = Some(error_message.unwrap_or_else(|| "模型未能完成请求".into()))
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
        output.error = Some("任务超时，已保留完成的内容，可以继续讨论".into());
    } else if control.cancel.is_cancelled() {
        output.error = Some("任务已停止".into());
    }
    if context.messages.last().is_some_and(|message| {
        matches!(message.as_llm(), Some(Message::User { content, .. }) if content.iter().any(|content| matches!(content, Content::Text { text } if text.starts_with(yoagent::agent_loop::AGENT_STOPPED_PREFIX))))
    }) && output.error.is_none() {
        output.error = Some("任务已达到执行预算，可以继续讨论".into());
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

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod protocol_tests;
