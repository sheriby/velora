//! AI 端点(一个 agent 档案)的类型与协议元数据。
//!
//! 本层只描述「连谁、怎么标识」,不含任何协议细节——每种协议的 URL 拼接、
//! 请求头、请求体与流解析全部内聚在 `transport/` 下各自的模块里。新增一种
//! 协议的固定动作:`ProviderKind` 加一个枚举臂(本表自动覆盖 id 往返)、
//! 新建一个 transport 模块、在 `transport::stream_completion` 的分发处注册。

/// 端点协议形态。
///
/// 命名对应业界事实标准的三种 API 形态加一个内置演示后端:
/// - `ChatCompletions`:OpenAI 兼容 Chat Completions,覆盖 OpenAI、DeepSeek、
///   智谱 GLM、Moonshot Kimi、OpenRouter 与本地 Ollama/LM Studio。
/// - `Responses`:OpenAI Responses API(instructions + input,SSE 事件流)。
/// - `Messages`:Anthropic Messages API(x-api-key + 顶层 system,content 块流)。
/// - `Stub`:内置演示后端,无网络、按动作回放剧本,供试用与测试。
// Responses/Messages/Stub 的构造点与元数据的读取方在配置层/设置页
// (本系列后续提交)接线;过渡期放行。
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum ProviderKind {
    #[default]
    ChatCompletions,
    Responses,
    Messages,
    Stub,
}

impl ProviderKind {
    /// 稳定存储与配置文件用的标识。
    #[allow(dead_code)]
    pub(crate) fn id(self) -> &'static str {
        match self {
            ProviderKind::ChatCompletions => "chat-completions",
            ProviderKind::Responses => "responses",
            ProviderKind::Messages => "messages",
            ProviderKind::Stub => "stub",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.id() == id)
    }

    /// 协议专名(UI 层直接展示;stub 不是专名,由 UI 层用 i18n 文案替代)。
    pub(crate) fn display_name(self) -> &'static str {
        match self {
            ProviderKind::ChatCompletions => "OpenAI Chat Completions",
            ProviderKind::Responses => "OpenAI Responses",
            ProviderKind::Messages => "Anthropic Messages",
            ProviderKind::Stub => "Stub",
        }
    }

    /// 是否需要 API 密钥(stub 不需要)。
    pub(crate) fn needs_key(self) -> bool {
        !matches!(self, ProviderKind::Stub)
    }

    /// 是否需要服务地址(stub 不需要)。
    pub(crate) fn needs_base_url(self) -> bool {
        !matches!(self, ProviderKind::Stub)
    }

    pub(crate) const ALL: &'static [ProviderKind] = &[
        ProviderKind::ChatCompletions,
        ProviderKind::Responses,
        ProviderKind::Messages,
        ProviderKind::Stub,
    ];
}

/// 一次请求要连的端点(不含名字/默认位等档案属性)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AiEndpointConfig {
    pub(crate) kind: ProviderKind,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) model: String,
}

impl AiEndpointConfig {
    /// 按协议要求齐全才算可用;stub 连模型名都不需要(内置演示)。
    pub(crate) fn is_configured(&self) -> bool {
        if matches!(self.kind, ProviderKind::Stub) {
            return true;
        }
        (!self.kind.needs_base_url() || !self.base_url.trim().is_empty())
            && (!self.kind.needs_key() || !self.api_key.trim().is_empty())
            && !self.model.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kinds_round_trip_through_ids() {
        for kind in ProviderKind::ALL {
            assert_eq!(
                ProviderKind::from_id(kind.id()),
                Some(*kind),
                "id 往返:{}",
                kind.id()
            );
        }
        assert_eq!(ProviderKind::from_id("grpc"), None);
    }

    #[test]
    fn stub_needs_no_connection_fields() {
        let stub = AiEndpointConfig {
            kind: ProviderKind::Stub,
            ..AiEndpointConfig::default()
        };
        assert!(stub.is_configured(), "stub 无需地址密钥即可用");

        let completions = AiEndpointConfig {
            kind: ProviderKind::ChatCompletions,
            base_url: "  ".into(),
            api_key: "".into(),
            model: "m".into(),
        };
        assert!(!completions.is_configured());
        let with_fields = AiEndpointConfig {
            base_url: " https://api.openai.com/v1 ".into(),
            api_key: "sk".into(),
            ..completions
        };
        assert!(with_fields.is_configured());
    }
}
