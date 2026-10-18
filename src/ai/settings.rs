//! 模型连接与配置文件中的 API Key。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Protocol {
    #[default]
    ChatCompletions,
    Responses,
    Messages,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Endpoint {
    pub id: String,
    #[serde(default)]
    pub provider: String,
    pub name: String,
    pub protocol: Protocol,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_context")]
    pub context_tokens: u32,
}

impl Default for Endpoint {
    fn default() -> Self {
        Self {
            id: String::new(),
            provider: String::new(),
            name: String::new(),
            protocol: Protocol::default(),
            base_url: String::new(),
            model: String::new(),
            api_key: String::new(),
            context_tokens: default_context(),
        }
    }
}

fn default_context() -> u32 {
    64_000
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CustomInstruction {
    pub name: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AiSettings {
    pub enabled: bool,
    pub endpoints: Vec<Endpoint>,
    pub default_endpoint: String,
    pub instructions: Vec<CustomInstruction>,
    pub quick_instructions: std::collections::BTreeMap<String, String>,
    pub excluded_paths: Vec<String>,
    pub disabled_skills: Vec<String>,
}

impl AiSettings {
    pub fn load_at(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    pub fn load() -> Result<Self> {
        Self::load_at(&settings_path()?)
    }

    pub fn save_at(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::config::write_config_file_atomic(path, &serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn remove_endpoint(&mut self, id: &str) {
        self.endpoints.retain(|endpoint| endpoint.id != id);
        if self.default_endpoint == id {
            self.default_endpoint = self
                .endpoints
                .first()
                .map(|endpoint| endpoint.id.clone())
                .unwrap_or_default();
        }
    }

    pub fn active_endpoint(&self) -> Option<&Endpoint> {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.id == self.default_endpoint)
            .or_else(|| self.endpoints.first())
    }

    pub fn set_endpoint(&mut self, mut endpoint: Endpoint) -> Result<()> {
        if self.enabled {
            endpoint.model_config()?;
        }
        if endpoint.id.is_empty() {
            endpoint.id = uuid::Uuid::new_v4().to_string();
        }
        if self.default_endpoint.is_empty() {
            self.default_endpoint = endpoint.id.clone();
        }
        if let Some(existing) = self
            .endpoints
            .iter_mut()
            .find(|saved| saved.id == endpoint.id)
        {
            *existing = endpoint;
        } else {
            self.endpoints.push(endpoint);
        }
        Ok(())
    }
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Endpoint")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("protocol", &self.protocol)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field(
                "api_key",
                &if self.api_key.is_empty() {
                    ""
                } else {
                    "[redacted]"
                },
            )
            .field("context_tokens", &self.context_tokens)
            .finish()
    }
}

impl Endpoint {
    pub fn request_config(&self) -> Result<yoagent::provider::ModelConfig> {
        let model = self.model_config()?;
        if self.api_key.trim().is_empty()
            && super::providers::ProviderPreset::for_endpoint(self)
                .is_some_and(|preset| preset.requires_key)
        {
            return Err(EndpointError::MissingKey.into());
        }
        Ok(model)
    }

    pub fn model_config(&self) -> Result<yoagent::provider::ModelConfig> {
        if self.model.trim().is_empty() {
            return Err(EndpointError::MissingModel.into());
        }
        if !(4096..=2_000_000).contains(&self.context_tokens) {
            return Err(EndpointError::InvalidContext.into());
        }
        let url = url::Url::parse(self.base_url.trim()).map_err(|_| EndpointError::InvalidUrl)?;
        if !matches!(url.scheme(), "https" | "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(EndpointError::InvalidUrl.into());
        }
        let protocol = match self.protocol {
            Protocol::ChatCompletions => yoagent::provider::ApiProtocol::OpenAiCompletions,
            Protocol::Responses => yoagent::provider::ApiProtocol::OpenAiResponses,
            Protocol::Messages => yoagent::provider::ApiProtocol::AnthropicMessages,
        };
        let mut model = yoagent::provider::ModelConfig::custom(
            protocol,
            self.name.clone(),
            self.base_url.trim().trim_end_matches('/'),
            self.model.trim(),
            self.name.clone(),
        );
        if self.protocol == Protocol::ChatCompletions {
            model.compat = super::providers::ProviderPreset::for_endpoint(self)
                .and_then(|preset| preset.compatibility.map(|configure| configure()));
        }
        model.context_window = self.context_tokens;
        model.max_tokens = 8192.min(model.context_window / 4);
        Ok(model)
    }
}

pub(crate) fn settings_path() -> Result<PathBuf> {
    Ok(crate::config::VeloraConfigDirs::from_system()?
        .app_config_file()
        .with_file_name("ai.json"))
}

#[cfg(test)]
mod tests;

pub(crate) struct AiFeature {
    enabled: bool,
    revision: u64,
}

impl gpui::Global for AiFeature {}

impl AiFeature {
    pub fn init(cx: &mut gpui::App) {
        let enabled = match AiSettings::load() {
            Ok(settings) => settings.enabled,
            Err(error) => {
                eprintln!("读取 AI 开关失败，已保持关闭：{error}");
                false
            }
        };
        cx.set_global(Self {
            enabled,
            revision: 0,
        });
    }

    pub fn enabled(cx: &gpui::App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|feature| feature.enabled)
    }

    pub fn revision(cx: &gpui::App) -> u64 {
        cx.try_global::<Self>()
            .map_or(0, |feature| feature.revision)
    }

    /// 配置内容变化（不含开关）也推进版本号：按版本刷新的会话与偏好页
    /// 才能感知别的窗口/助手面板写入的连接与指令。
    pub fn bump_revision(cx: &mut gpui::App) {
        let enabled = Self::enabled(cx);
        let revision = Self::revision(cx).wrapping_add(1);
        cx.set_global(Self { enabled, revision });
        cx.refresh_windows();
    }

    pub fn set_enabled(cx: &mut gpui::App, enabled: bool) {
        let revision = Self::revision(cx).wrapping_add(1);
        cx.set_global(Self { enabled, revision });
        if cx.try_global::<crate::theme::ThemeManager>().is_some()
            && cx.try_global::<crate::i18n::I18nManager>().is_some()
        {
            crate::app_menu::install_menus(cx);
        }
        cx.refresh_windows();
    }
}

#[derive(Debug)]
pub(crate) enum EndpointError {
    MissingKey,
    MissingModel,
    InvalidContext,
    InvalidUrl,
}
impl std::fmt::Display for EndpointError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MissingKey => "API key is required",
            Self::MissingModel => "Model name is required",
            Self::InvalidContext => "Context budget must be between 4096 and 2000000",
            Self::InvalidUrl => {
                "A HTTP(S) service address without embedded credentials is required"
            }
        })
    }
}
impl std::error::Error for EndpointError {}

pub(crate) fn error_message(error: &anyhow::Error, strings: &crate::i18n::I18nStrings) -> String {
    match error.downcast_ref::<EndpointError>() {
        Some(EndpointError::MissingKey) => strings.preferences_ai_key_required.clone(),
        Some(EndpointError::MissingModel) => strings.preferences_ai_model_required.clone(),
        Some(EndpointError::InvalidContext) => strings.preferences_ai_context_invalid.clone(),
        Some(EndpointError::InvalidUrl) => strings.preferences_ai_url_invalid.clone(),
        None => error.to_string(),
    }
}
