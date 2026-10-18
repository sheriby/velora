use super::ProviderPreset;
use crate::ai::settings::Protocol;

#[test]
fn ai_provider_presets_fill_independent_endpoints_without_copying_credentials() {
    assert!(
        ProviderPreset::get("deepseek").is_some(),
        "应提供 DeepSeek 官方预设"
    );
    let preset = ProviderPreset::get("deepseek").unwrap();
    let mut first = preset.endpoint();
    first.api_key = "first-key".into();
    let second = preset.endpoint();
    assert_ne!(first.id, second.id);
    assert_eq!(second.base_url, "https://api.deepseek.com");
    assert_eq!(second.model, "deepseek-flash");
    assert_eq!(second.protocol, Protocol::ChatCompletions);
    assert!(second.api_key.is_empty());
    let openai = ProviderPreset::get("openai").unwrap().endpoint();
    assert_eq!(openai.protocol, Protocol::Responses);
    let anthropic = ProviderPreset::get("anthropic").unwrap().endpoint();
    assert_eq!(anthropic.protocol, Protocol::Messages);
    let local = ProviderPreset::get("ollama").unwrap().endpoint();
    assert!(local.model.is_empty(), "本地模型由用户选择，不假定已经安装");
    assert_eq!(local.base_url, "http://localhost:11434/v1");
    let ids = ProviderPreset::all()
        .iter()
        .map(|preset| preset.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), ProviderPreset::all().len());
}

#[test]
fn ai_provider_cloud_requests_require_keys_but_local_endpoints_remain_keyless() {
    let mut cloud = ProviderPreset::get("deepseek").unwrap().endpoint();
    assert!(cloud.model_config().is_ok(), "可以先保存连接后填写 Key");
    assert!(
        cloud.request_config().is_err(),
        "缺少云端 Key 时应在请求之前失败"
    );
    cloud.api_key = "test-key".into();
    assert!(cloud.request_config().is_ok());
    let mut local = ProviderPreset::get("ollama").unwrap().endpoint();
    local.model = "installed-model".into();
    local.base_url = "http://example.test:11434/v1".into();
    let model = local.request_config().unwrap();
    assert!(
        model.compat.unwrap().requires_assistant_after_tool_result,
        "更改本地模型地址也保留其工具续轮兼容设置"
    );
}
