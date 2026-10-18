//! 模型连接与配置文件中的 API Key。

use anyhow::{Context, Result, bail};
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

    pub fn remove_endpoint(&mut self, path: &Path, id: &str) -> Result<()> {
        let mut staged = self.clone();
        staged.endpoints.retain(|endpoint| endpoint.id != id);
        if staged.default_endpoint == id {
            staged.default_endpoint = staged
                .endpoints
                .first()
                .map(|endpoint| endpoint.id.clone())
                .unwrap_or_default();
        }
        staged.save_at(path)?;
        *self = staged;
        Ok(())
    }

    pub fn active_endpoint(&self) -> Option<&Endpoint> {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.id == self.default_endpoint)
            .or_else(|| self.endpoints.first())
    }

    pub fn save_endpoint(
        &mut self,
        path: &Path,
        mut endpoint: Endpoint,
        secret: Option<&str>,
    ) -> Result<()> {
        endpoint.model_config()?;
        if endpoint.id.is_empty() {
            endpoint.id = uuid::Uuid::new_v4().to_string();
        }
        if let Some(secret) = secret {
            endpoint.api_key = secret.to_owned();
        }
        let mut staged = self.clone();
        if let Some(existing) = staged
            .endpoints
            .iter_mut()
            .find(|saved| saved.id == endpoint.id)
        {
            *existing = endpoint.clone();
        } else {
            staged.endpoints.push(endpoint.clone());
        }
        if staged.default_endpoint.is_empty() {
            staged.default_endpoint = endpoint.id;
        }
        staged.save_at(path)?;
        *self = staged;
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
    pub fn model_config(&self) -> Result<yoagent::provider::ModelConfig> {
        if self.model.trim().is_empty() {
            bail!("请输入模型名称");
        }
        if !(4096..=2_000_000).contains(&self.context_tokens) {
            bail!("上下文 token 预算应在 4096 到 2000000 之间");
        }
        let url = url::Url::parse(self.base_url.trim()).context("服务地址无效")?;
        if !matches!(url.scheme(), "https" | "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            bail!("请输入不含密钥的 HTTP 或 HTTPS 服务地址");
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
        if self.protocol == Protocol::ChatCompletions && url.host_str() == Some("api.deepseek.com")
        {
            model.compat = Some(yoagent::provider::OpenAiCompat::deepseek());
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
        cx.set_global(Self { enabled });
    }

    pub fn enabled(cx: &gpui::App) -> bool {
        cx.try_global::<Self>().is_some_and(|feature| feature.enabled)
    }

    pub fn set_enabled(cx: &mut gpui::App, enabled: bool) {
        cx.set_global(Self { enabled });
        if cx.try_global::<crate::theme::ThemeManager>().is_some()
            && cx.try_global::<crate::i18n::I18nManager>().is_some()
        {
            crate::app_menu::install_menus(cx);
        }
        cx.refresh_windows();
    }
}
