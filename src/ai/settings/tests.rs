use super::*;
fn endpoint() -> Endpoint {
    Endpoint {
        id: "test".into(),
        name: "本地测试".into(),
        base_url: "http://localhost:11434/v1".into(),
        model: "model".into(),
        context_tokens: 64_000,
        ..Endpoint::default()
    }
}

#[test]
fn ai_settings_store_api_keys_in_configuration_and_load_keyless_endpoints() {
    let root = std::env::temp_dir().join(format!("velora-ai-settings-{}", uuid::Uuid::new_v4()));
    let path = root.join("ai.json");
    let mut settings = AiSettings::default();
    let saved = settings.save_endpoint(&path, endpoint(), Some("private-test-key"));
    assert!(saved.is_ok(), "{saved:?}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["endpoints"][0]["api_key"],
        "private-test-key"
    );
    assert!(!text.contains("credential"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let loaded = AiSettings::load_at(&path).unwrap();
    assert_eq!(loaded.endpoints.len(), 1);
    assert_eq!(loaded.endpoints[0].api_key, "private-test-key");
    assert_eq!(loaded, settings);
    let mut local = endpoint();
    local.id = "keyless".into();
    settings.save_endpoint(&path, local, None).unwrap();
    assert_eq!(AiSettings::load_at(&path).unwrap().endpoints.len(), 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_settings_failed_save_preserves_old_configuration_and_key() {
    let root = std::env::temp_dir().join(format!("velora-ai-settings-{}", uuid::Uuid::new_v4()));
    let path = root.join("ai.json");
    let mut settings = AiSettings::default();
    assert!(
        settings
            .save_endpoint(&path, endpoint(), Some("old-key"))
            .is_ok()
    );
    let previous = settings.clone();
    let invalid = path.join("blocked.json");
    assert!(
        settings
            .save_endpoint(&invalid, endpoint(), Some("new-key"))
            .is_err()
    );
    assert_eq!(settings, previous);
    assert_eq!(AiSettings::load_at(&path).unwrap(), previous);
    assert_eq!(settings.endpoints[0].api_key, "old-key");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_settings_removing_an_endpoint_removes_its_key_from_configuration() {
    let root = std::env::temp_dir().join(format!("velora-ai-remove-{}", uuid::Uuid::new_v4()));
    let path = root.join("ai.json");
    let mut settings = AiSettings::default();
    settings
        .save_endpoint(&path, endpoint(), Some("test-secret"))
        .unwrap();
    assert!(
        settings
            .remove_endpoint(&path.join("blocked"), "test")
            .is_err()
    );
    assert_eq!(settings.endpoints[0].api_key, "test-secret");
    settings.remove_endpoint(&path, "test").unwrap();
    assert!(AiSettings::load_at(&path).unwrap().endpoints.is_empty());
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("test-secret")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_settings_load_manually_configured_keys_without_system_credentials_and_redact_debug_output() {
    let root = std::env::temp_dir().join(format!("velora-ai-key-file-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("ai.json");
    let mut encoded = serde_json::to_value(endpoint()).unwrap();
    encoded["api_key"] = serde_json::json!("manual-test-key");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({"endpoints":[encoded],"default_endpoint":"test"}))
            .unwrap(),
    )
    .unwrap();
    let settings = AiSettings::load_at(&path).unwrap();
    assert_eq!(
        serde_json::to_value(&settings).unwrap()["endpoints"][0]["api_key"],
        "manual-test-key"
    );
    assert!(!format!("{settings:?}").contains("manual-test-key"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ai_settings_editing_a_connection_preserves_its_key_until_explicitly_changed_or_cleared() {
    let root = std::env::temp_dir().join(format!("velora-ai-key-edit-{}", uuid::Uuid::new_v4()));
    let path = root.join("ai.json");
    let mut settings = AiSettings::default();
    settings
        .save_endpoint(&path, endpoint(), Some("original-key"))
        .unwrap();
    let mut edited = settings.endpoints[0].clone();
    edited.model = "another-model".into();
    settings.save_endpoint(&path, edited, None).unwrap();
    assert_eq!(
        AiSettings::load_at(&path).unwrap().endpoints[0].api_key,
        "original-key"
    );
    settings
        .save_endpoint(
            &path,
            settings.endpoints[0].clone(),
            Some("replacement-key"),
        )
        .unwrap();
    assert_eq!(
        AiSettings::load_at(&path).unwrap().endpoints[0].api_key,
        "replacement-key"
    );
    settings
        .save_endpoint(&path, settings.endpoints[0].clone(), Some(""))
        .unwrap();
    assert_eq!(AiSettings::load_at(&path).unwrap().endpoints[0].api_key, "");
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("replacement-key")
    );
    std::fs::remove_dir_all(root).unwrap();
}
