use super::settings::{Endpoint, Protocol};

pub(crate) struct ProviderPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub protocol: Protocol,
    pub base_url: &'static str,
    pub model: &'static str,
    pub context_tokens: u32,
    pub requires_key: bool,
    pub compatibility: Option<fn() -> yoagent::provider::OpenAiCompat>,
}

impl ProviderPreset {
    pub fn all() -> &'static [Self] {
        &[
            Self {
                id: "deepseek",
                name: "DeepSeek",
                protocol: Protocol::ChatCompletions,
                base_url: "https://api.deepseek.com",
                model: "deepseek-flash",
                context_tokens: 64_000,
                requires_key: true,
                compatibility: Some(yoagent::provider::OpenAiCompat::deepseek),
            },
            Self {
                id: "openai",
                name: "OpenAI",
                protocol: Protocol::Responses,
                base_url: "https://api.openai.com/v1",
                model: "gpt-6-astra",
                context_tokens: 64_000,
                requires_key: true,
                compatibility: Some(yoagent::provider::OpenAiCompat::openai),
            },
            Self {
                id: "anthropic",
                name: "Anthropic",
                protocol: Protocol::Messages,
                base_url: "https://api.anthropic.com/v1",
                model: "claude-sonnet-5-5",
                context_tokens: 64_000,
                requires_key: true,
                compatibility: None,
            },
            Self {
                id: "ollama",
                name: "Ollama",
                protocol: Protocol::ChatCompletions,
                base_url: "http://localhost:11434/v1",
                model: "",
                context_tokens: 8192,
                requires_key: false,
                compatibility: Some(yoagent::provider::OpenAiCompat::ollama),
            },
            Self {
                id: "custom",
                name: "Custom",
                protocol: Protocol::ChatCompletions,
                base_url: "http://localhost:8080/v1",
                model: "",
                context_tokens: 64_000,
                requires_key: false,
                compatibility: None,
            },
        ]
    }

    pub fn get(id: &str) -> Option<&'static Self> {
        Self::all().iter().find(|preset| preset.id == id)
    }

    pub fn for_endpoint(endpoint: &Endpoint) -> Option<&'static Self> {
        if let Some(preset) = Self::get(&endpoint.provider).filter(|preset| preset.id != "custom") {
            return Some(preset);
        }
        let Ok(url) = url::Url::parse(&endpoint.base_url) else {
            return Self::get(&endpoint.provider);
        };
        let matching = Self::all()
            .iter()
            .filter(|preset| preset.id != "custom")
            .find(|preset| {
                let Ok(base) = url::Url::parse(preset.base_url) else {
                    return false;
                };
                if preset.id == "ollama" {
                    matches!(
                        url.host_str(),
                        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
                    ) && url.port() == base.port()
                } else {
                    url.host_str() == base.host_str()
                }
            });
        matching.or_else(|| Self::get(&endpoint.provider))
    }

    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            id: uuid::Uuid::new_v4().to_string(),
            provider: self.id.into(),
            name: self.name.into(),
            protocol: self.protocol,
            base_url: self.base_url.into(),
            model: self.model.into(),
            context_tokens: self.context_tokens,
            api_key: String::new(),
        }
    }
}

#[cfg(test)]
mod tests;
