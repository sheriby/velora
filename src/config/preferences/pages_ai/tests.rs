use super::{AppPreferences, PreferencesWindow, RunControl};
use gpui::TestAppContext;

#[test]
fn ai_http_warning_only_accepts_loopback_services() {
    assert!(!PreferencesWindow::ai_address_is_insecure_http(
        "http://127.0.0.1:11434/v1"
    ));
    assert!(!PreferencesWindow::ai_address_is_insecure_http(
        "http://localhost:11434/v1"
    ));
    assert!(!PreferencesWindow::ai_address_is_insecure_http(
        "https://api.example.com/v1"
    ));
    assert!(PreferencesWindow::ai_address_is_insecure_http(
        "http://api.example.com/v1"
    ));
}

#[gpui::test]
fn changing_a_connection_clears_the_previous_test_status(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
    });
    let (preferences, cx) = cx
        .add_window_view(|_, cx| PreferencesWindow::new(AppPreferences::default(), Vec::new(), cx));
    let control = preferences.update_in(cx, |preferences, window, cx| {
        preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx);
        preferences
            .ai_preferences
            .as_mut()
            .unwrap()
            .settings
            .enabled = true;
        preferences.select_ai_preferences_endpoint(
            crate::ai::providers::ProviderPreset::get("custom")
                .unwrap()
                .endpoint(),
            window,
            cx,
        );
        let state = preferences.ai_preferences.as_mut().unwrap();
        state.control = RunControl::default();
        state.testing = true;
        state.status = Some("old result".into());
        state
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.focus(window, cx));
        state.control.clone()
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.simulate_input("changed-model");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    preferences.read_with(cx, |preferences, _| {
        assert!(
            preferences
                .ai_preferences
                .as_ref()
                .unwrap()
                .status
                .is_none(),
            "新连接输入不能沿用旧测试结果"
        )
    });
    assert!(control.cancel.is_cancelled());
}

#[gpui::test]
fn ai_opt_in_preferences_only_apply_on_save_and_keep_connection_data_when_disabled(
    cx: &mut TestAppContext,
) {
    let directory = std::env::temp_dir().join(format!("velora-ai-opt-in-{}", uuid::Uuid::new_v4()));
    let _config = crate::config::override_test_config_root(&directory);
    let mut settings = crate::ai::settings::AiSettings::default();
    let path = crate::ai::settings::settings_path().unwrap();
    settings
        .set_endpoint(crate::ai::settings::Endpoint {
            id: "local".into(),
            name: "Local".into(),
            base_url: "http://localhost:11434/v1".into(),
            model: "model".into(),
            api_key: "test-secret".into(),
            ..Default::default()
        })
        .unwrap();
    settings.save_at(&path).unwrap();
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
        crate::ai::settings::AiFeature::init(cx);
    });
    let (preferences, cx) = cx
        .add_window_view(|_, cx| PreferencesWindow::new(AppPreferences::default(), Vec::new(), cx));
    preferences.update_in(cx, |preferences, window, cx| {
        preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx)
    });
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    assert!(cx.debug_bounds("preferences-ai-model").is_none());
    let toggle = cx
        .debug_bounds("preferences-ai-enabled")
        .expect("AI 设置应从启用开关开始");
    cx.simulate_click(toggle.center(), gpui::Modifiers::none());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    preferences.read_with(cx, |preferences, cx| {
        assert!(
            preferences
                .ai_preferences
                .as_ref()
                .unwrap()
                .settings
                .enabled
        );
        assert!(
            !crate::ai::settings::AiFeature::enabled(cx),
            "未保存的开关不能影响编辑器"
        );
    });
    assert!(cx.debug_bounds("preferences-ai-model").is_some());
    assert_eq!(
        cx.debug_bounds("preferences-ai-enabled").unwrap(),
        toggle,
        "开启后开关自身不能跳位"
    );
    preferences.update(cx, |preferences, cx| {
        preferences.save_ai_preferences(cx).unwrap()
    });
    let saved = crate::ai::settings::AiSettings::load_at(&path).unwrap();
    assert!(saved.enabled);
    assert_eq!(saved.endpoints[0].api_key, "test-secret");
    cx.simulate_click(toggle.center(), gpui::Modifiers::none());
    preferences.update(cx, |preferences, cx| {
        assert!(crate::ai::settings::AiFeature::enabled(cx));
        preferences.save_ai_preferences(cx).unwrap();
        assert!(!crate::ai::settings::AiFeature::enabled(cx));
    });
    let disabled = crate::ai::settings::AiSettings::load_at(&path).unwrap();
    assert!(!disabled.enabled);
    assert_eq!(disabled.endpoints, saved.endpoints);
    cx.on_quit(move || std::fs::remove_dir_all(directory).unwrap());
}

#[gpui::test]
fn ai_preferences_keeps_newer_unsaved_connection_input_after_a_save(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
    });
    let (preferences, cx) = cx
        .add_window_view(|_, cx| PreferencesWindow::new(AppPreferences::default(), Vec::new(), cx));
    preferences.update_in(cx, |preferences, window, cx| {
        preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx);
        preferences
            .ai_preferences
            .as_mut()
            .unwrap()
            .settings
            .enabled = true;
        preferences.select_ai_preferences_endpoint(
            crate::ai::providers::ProviderPreset::get("custom")
                .unwrap()
                .endpoint(),
            window,
            cx,
        );
        preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.set_value("saved-model", window, cx));
        preferences.save_ai_preferences(cx).unwrap();
        preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| {
                field.set_value("newer-unsaved-model", window, cx)
            });
    });
    cx.run_until_parked();
    preferences.read_with(cx, |preferences, cx| {
        let state = preferences.ai_preferences.as_ref().unwrap();
        assert_eq!(
            state.draft.as_ref().unwrap().model.read(cx).value(),
            "newer-unsaved-model"
        );
        assert_eq!(state.saved.endpoints[0].model, "saved-model");
        assert_eq!(
            state.staged(cx).unwrap().endpoints[0].model,
            "newer-unsaved-model"
        );
    });
}

#[gpui::test]
fn ai_provider_picker_prefills_service_details_and_keeps_advanced_fields_collapsed(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
    });
    let (preferences, cx) = cx
        .add_window_view(|_, cx| PreferencesWindow::new(AppPreferences::default(), Vec::new(), cx));
    preferences.update_in(cx, |preferences, window, cx| {
        preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx);
        preferences
            .ai_preferences
            .as_mut()
            .unwrap()
            .settings
            .enabled = true;
    });
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let provider = cx.debug_bounds("preferences-ai-provider-deepseek");
    assert!(
        provider.is_some(),
        "新建连接应先选提供商，而不是手填协议和地址"
    );
    cx.simulate_click(provider.unwrap().center(), gpui::Modifiers::none());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    preferences.read_with(cx, |preferences, cx| {
        let draft = preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap();
        assert_eq!(draft.model.read(cx).value(), "deepseek-flash");
        assert_eq!(draft.address.read(cx).value(), "https://api.deepseek.com");
        assert!(draft.key.read(cx).value().is_empty());
    });
    assert!(cx.debug_bounds("preferences-ai-key").is_some());
    assert!(cx.debug_bounds("preferences-ai-model").is_some());
    assert!(cx.debug_bounds("preferences-ai-address").is_none());
    assert!(cx.debug_bounds("preferences-ai-context").is_none());
    cx.simulate_input("  pasted-test-key  ");
    preferences.read_with(cx, |preferences, cx| {
        let settings = preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .staged(cx)
            .unwrap();
        assert_eq!(
            settings.active_endpoint().unwrap().api_key,
            "pasted-test-key",
            "选择服务后应直接输入 Key，复制时的边界空白不应进入凭据"
        );
    });
}

#[gpui::test]
fn ai_provider_switching_and_deleting_preserves_incomplete_drafts_and_separate_keys(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
    });
    let (preferences, cx) = cx
        .add_window_view(|_, cx| PreferencesWindow::new(AppPreferences::default(), Vec::new(), cx));
    preferences.update_in(cx, |preferences, window, cx| {
        preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx);
        preferences
            .ai_preferences
            .as_mut()
            .unwrap()
            .settings
            .enabled = true;
        preferences.add_ai_preferences_provider(
            crate::ai::providers::ProviderPreset::get("deepseek").unwrap(),
            window,
            cx,
        );
        let first = {
            let draft = preferences
                .ai_preferences
                .as_ref()
                .unwrap()
                .draft
                .as_ref()
                .unwrap();
            draft
                .key
                .update(cx, |field, cx| field.set_value("first-key", window, cx));
            draft
                .model
                .update(cx, |field, cx| field.set_value("", window, cx));
            draft.endpoint.clone()
        };
        preferences.add_ai_preferences_provider(
            crate::ai::providers::ProviderPreset::get("openai").unwrap(),
            window,
            cx,
        );
        let second = preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap();
        assert!(second.key.read(cx).value().is_empty());
        second
            .key
            .update(cx, |field, cx| field.set_value("second-key", window, cx));
        second
            .model
            .update(cx, |field, cx| field.set_value("second-model", window, cx));
        preferences.select_ai_preferences_endpoint(first, window, cx);
        let draft = preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap();
        assert_eq!(draft.key.read(cx).value(), "first-key");
        assert_eq!(draft.model.read(cx).value(), "");
        assert!(
            preferences.save_error.is_none(),
            "不完整草稿不能阻止切换连接"
        );
    });
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    // 「显示思考过程」开关加高页面：先把目标控件滚进可视范围再点击。
    let page = cx.debug_bounds("preferences-page-scroll").unwrap();
    if let Some(bottom) = cx
        .debug_bounds("preferences-ai-advanced")
        .map(|bounds| bounds.bottom())
    {
        let overflow = bottom - page.bottom() + gpui::px(24.0);
        if overflow > gpui::px(0.0) {
            preferences.update_in(cx, |preferences, _, cx| {
                let offset = preferences.page_scroll.offset();
                preferences
                    .page_scroll
                    .set_offset(gpui::point(offset.x, offset.y - overflow));
                cx.notify();
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
    }
    let advanced = cx.debug_bounds("preferences-ai-advanced").unwrap();
    cx.simulate_click(advanced.center(), gpui::Modifiers::none());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let page = cx.debug_bounds("preferences-page-scroll").unwrap();
    if let Some(bottom) = cx
        .debug_bounds("preferences-ai-delete")
        .map(|bounds| bounds.bottom())
    {
        let overflow = bottom - page.bottom() + gpui::px(24.0);
        if overflow > gpui::px(0.0) {
            preferences.update_in(cx, |preferences, _, cx| {
                let offset = preferences.page_scroll.offset();
                preferences
                    .page_scroll
                    .set_offset(gpui::point(offset.x, offset.y - overflow));
                cx.notify();
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
    }
    let delete = cx.debug_bounds("preferences-ai-delete").unwrap();
    cx.simulate_click(delete.center(), gpui::Modifiers::none());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    preferences.read_with(cx, |preferences, cx| {
        let draft = preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap();
        assert_eq!(
            draft.key.read(cx).value(),
            "second-key",
            "删除另一连接后也不能重建并丢弃已有草稿"
        );
        assert_eq!(draft.model.read(cx).value(), "second-model");
    });
}

#[gpui::test]
fn ai_provider_disabling_preserves_a_cleared_key_and_paused_connection_draft(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
    });
    let path = crate::ai::settings::settings_path().unwrap();
    let mut settings = crate::ai::settings::AiSettings::default();
    let mut endpoint = crate::ai::providers::ProviderPreset::get("deepseek")
        .unwrap()
        .endpoint();
    endpoint.api_key = "old-key".into();
    settings.set_endpoint(endpoint).unwrap();
    settings.enabled = true;
    settings.save_at(&path).unwrap();
    let (preferences, cx) = cx
        .add_window_view(|_, cx| PreferencesWindow::new(AppPreferences::default(), Vec::new(), cx));
    preferences.update_in(cx, |preferences, window, cx| {
        preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx);
        let draft = preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap();
        draft
            .key
            .update(cx, |field, cx| field.set_value("", window, cx));
        draft
            .model
            .update(cx, |field, cx| field.set_value("", window, cx));
    });
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let toggle = cx.debug_bounds("preferences-ai-enabled").unwrap();
    cx.simulate_click(toggle.center(), gpui::Modifiers::none());
    preferences.update(cx, |preferences, cx| {
        preferences.save_ai_preferences(cx).unwrap()
    });
    let saved = crate::ai::settings::AiSettings::load_at(&path).unwrap();
    assert!(!saved.enabled);
    assert!(
        saved.endpoints[0].api_key.is_empty(),
        "停用不能恢复已经清空的旧 Key"
    );
    assert!(
        saved.endpoints[0].model.is_empty(),
        "停用时保留尚未填写完的连接，而不阻止关闭"
    );
}

#[gpui::test]
fn ai_preferences_customizes_quick_instructions_as_multiline_unsaved_drafts(
    cx: &mut TestAppContext,
) {
    let directory =
        std::env::temp_dir().join(format!("velora-ai-prompts-{}", uuid::Uuid::new_v4()));
    let _config = crate::config::override_test_config_root(&directory);
    let path = crate::ai::settings::settings_path().unwrap();
    crate::ai::settings::AiSettings {
        enabled: true,
        ..Default::default()
    }
    .save_at(&path)
    .unwrap();
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
        crate::ai::settings::AiFeature::init(cx);
    });
    let (preferences, cx) = cx
        .add_window_view(|_, cx| PreferencesWindow::new(AppPreferences::default(), Vec::new(), cx));
    preferences.update_in(cx, |preferences, window, cx| {
        window.activate_window();
        preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx);
    });
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let tab = cx
        .debug_bounds("preferences-ai-instructions-tab")
        .expect("AI 偏好设置需要独立的指令页");
    cx.simulate_click(tab.center(), gpui::Modifiers::none());
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let menu = cx.debug_bounds("preferences-ai-command-menu").unwrap();
    cx.simulate_click(menu.center(), gpui::Modifiers::none());
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let polish = cx.debug_bounds("preferences-ai-command-polish").unwrap();
    cx.simulate_click(polish.center(), gpui::Modifiers::none());
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let body = cx.debug_bounds("preferences-ai-instruction-body").unwrap();
    assert!(
        body.size.height > gpui::px(100.0),
        "任务要求需要可换行的编辑区"
    );
    cx.simulate_click(body.center(), gpui::Modifiers::none());
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input("保留术语。");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("保留链接。");
    cx.run_until_parked();
    preferences.read_with(cx, |preferences, cx| {
        let staged = preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .staged(cx)
            .unwrap();
        assert_eq!(
            staged.quick_instructions["polish"],
            "保留术语。\n保留链接。"
        );
    });
    assert!(
        crate::ai::settings::AiSettings::load_at(&path)
            .unwrap()
            .quick_instructions
            .is_empty(),
        "未保存的指令不能改变实际配置"
    );
    for selector in [
        "preferences-ai-command-summarize",
        "preferences-ai-command-polish",
    ] {
        let menu = cx.debug_bounds("preferences-ai-command-menu").unwrap();
        cx.simulate_click(menu.center(), gpui::Modifiers::none());
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        let item = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(item.center(), gpui::Modifiers::none());
        for _ in 0..2 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
    }
    preferences.read_with(cx, |preferences, cx| {
        assert_eq!(
            preferences
                .ai_preferences
                .as_ref()
                .unwrap()
                .instruction
                .field
                .read(cx)
                .value(),
            "保留术语。\n保留链接。",
            "切换指令不能丢弃草稿"
        );
    });
    preferences.update(cx, |preferences, cx| {
        preferences.save_ai_preferences(cx).unwrap()
    });
    let saved = crate::ai::settings::AiSettings::load_at(&path).unwrap();
    assert_eq!(saved.quick_instructions["polish"], "保留术语。\n保留链接。");
    for _ in 0..2 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }
    let reset = cx
        .debug_bounds("preferences-ai-restore-instruction")
        .unwrap();
    cx.simulate_click(reset.center(), gpui::Modifiers::none());
    preferences.read_with(cx, |preferences, cx| {
        assert!(
            !preferences
                .ai_preferences
                .as_ref()
                .unwrap()
                .staged(cx)
                .unwrap()
                .quick_instructions
                .contains_key("polish")
        );
    });
    assert_eq!(
        crate::ai::settings::AiSettings::load_at(&path)
            .unwrap()
            .quick_instructions,
        saved.quick_instructions
    );
    preferences.update(cx, |preferences, cx| {
        preferences.save_ai_preferences(cx).unwrap()
    });
    assert!(
        crate::ai::settings::AiSettings::load_at(&path)
            .unwrap()
            .quick_instructions
            .is_empty()
    );
    cx.on_quit(move || std::fs::remove_dir_all(directory).unwrap());
}

#[gpui::test]
fn ai_preferences_updates_default_instruction_language_without_overwriting_a_custom_draft(
    cx: &mut TestAppContext,
) {
    let directory =
        std::env::temp_dir().join(format!("velora-ai-prompt-locale-{}", uuid::Uuid::new_v4()));
    let _config = crate::config::override_test_config_root(&directory);
    let path = crate::ai::settings::settings_path().unwrap();
    crate::ai::settings::AiSettings {
        enabled: true,
        ..Default::default()
    }
    .save_at(&path)
    .unwrap();
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
    });
    let (preferences, cx) = cx
        .add_window_view(|_, cx| PreferencesWindow::new(AppPreferences::default(), Vec::new(), cx));
    preferences.update_in(cx, |preferences, window, cx| {
        preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx)
    });
    cx.update(|_, cx| crate::i18n::I18nManager::init_with_language_id(cx, "zh-CN"));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    preferences.update_in(cx, |preferences, window, cx| {
        let state = preferences.ai_preferences.as_ref().unwrap();
        assert_eq!(
            state.instruction.field.read(cx).value(),
            crate::ai::instructions::default_instruction(crate::ai::AiCommand::Ask, "zh-CN")
        );
        assert!(
            state.staged(cx).unwrap().quick_instructions.is_empty(),
            "切换语言的默认要求不能被误存为自定义指令"
        );
        state.instruction.field.update(cx, |field, cx| {
            field.set_value("保持自然，避免套话。", window, cx)
        });
    });
    cx.update(|_, cx| crate::i18n::I18nManager::init_with_language_id(cx, "en-US"));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    preferences.read_with(cx, |preferences, cx| {
        let state = preferences.ai_preferences.as_ref().unwrap();
        assert_eq!(
            state.instruction.field.read(cx).value(),
            "保持自然，避免套话。"
        );
        assert_eq!(
            state.staged(cx).unwrap().quick_instructions["conversation"],
            "保持自然，避免套话。"
        );
    });
    assert!(
        crate::ai::settings::AiSettings::load_at(&path)
            .unwrap()
            .quick_instructions
            .is_empty(),
        "取消设置不保存语言切换中的草稿"
    );
    cx.on_quit(move || std::fs::remove_dir_all(directory).unwrap());
}

#[gpui::test]
fn ai_connection_inputs_are_named_and_do_not_expose_the_key(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let directory = std::env::temp_dir().join(format!("velora-ai-inputs-{}", uuid::Uuid::new_v4()));
    let _config = crate::config::override_test_config_root(&directory);
    let mut settings = crate::ai::settings::AiSettings {
        enabled: true,
        ..Default::default()
    };
    let mut endpoint = crate::ai::providers::ProviderPreset::get("deepseek")
        .unwrap()
        .endpoint();
    endpoint.api_key = "test-secret".into();
    let model = endpoint.model.clone();
    settings.set_endpoint(endpoint).unwrap();
    settings
        .save_at(&crate::ai::settings::settings_path().unwrap())
        .unwrap();
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
    });
    let handle = cx.update(|cx| {
        super::super::open_preferences_window_with_state(
            cx,
            AppPreferences::default(),
            Vec::new(),
            "Preferences".into(),
        )
    });
    handle
        .update(cx, |preferences, window, cx| {
            preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx)
        })
        .unwrap();
    let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
    visual.update(|window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window
                .try_find("preferences-ai-model")
                .and_then(|input| input.role()),
            Some(gpui::Role::TextInput),
            "模型必须是具有名称的标准输入框"
        );
        let input = window.find("preferences-ai-model");
        assert_eq!(input.value(), Some(model.as_str()));
        assert_eq!(
            input.label(),
            Some(
                cx.global::<crate::i18n::I18nManager>()
                    .strings()
                    .preferences_ai_model
                    .as_str()
            )
        );
        let key = window.find("preferences-ai-key");
        assert_eq!(key.role(), Some(gpui::Role::PasswordInput));
        assert_eq!(key.value(), None, "密钥不得暴露到可访问性树");
    });
    cx.on_quit(move || std::fs::remove_dir_all(directory).unwrap());
}

#[gpui::test]
fn ai_instruction_selector_supports_keyboard_confirmation(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let directory =
        std::env::temp_dir().join(format!("velora-ai-selector-{}", uuid::Uuid::new_v4()));
    let _config = crate::config::override_test_config_root(&directory);
    crate::ai::settings::AiSettings {
        enabled: true,
        ..Default::default()
    }
    .save_at(&crate::ai::settings::settings_path().unwrap())
    .unwrap();
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        crate::config::EditorSettings::init(cx, true);
    });
    let handle = cx.update(|cx| {
        super::super::open_preferences_window_with_state(
            cx,
            AppPreferences::default(),
            Vec::new(),
            "Preferences".into(),
        )
    });
    handle
        .update(cx, |preferences, window, cx| {
            preferences.set_nav_ai(&gpui::ClickEvent::default(), window, cx);
            cx.notify();
        })
        .unwrap();
    let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
    visual.update(|window, cx| window.render_frame(cx));
    let tab = visual
        .debug_bounds("preferences-ai-connections-tab")
        .expect("连接分段应可见");
    visual.simulate_click(tab.center(), gpui::Modifiers::none());
    visual.update(|window, cx| window.press("right", cx));
    visual.run_until_parked();
    assert!(
        handle
            .read_with(cx, |preferences, _| preferences
                .ai_preferences
                .as_ref()
                .unwrap()
                .show_instructions)
            .unwrap(),
        "方向键应能切换到指令分段"
    );
    visual.update(|window, cx| {
        window.render_frame(cx);
        let tab = window.within("preferences-ai-tabs").find(1usize);
        assert_eq!(tab.role(), Some(gpui::Role::Tab));
        assert_eq!(tab.selected(), Some(true));
        assert!(handle.read_with(cx, |preferences, _| preferences.ai_preferences.as_ref().unwrap().instructions_focus.is_focused(window)).unwrap(),
            "键盘切换后实际窗口焦点应跟随所选分段");
        assert_eq!(
            window
                .try_find("preferences-ai-command-menu")
                .and_then(|input| input.role()),
            Some(gpui::Role::ComboBox),
            "指令选择应使用标准选择器"
        );
        window.click("preferences-ai-command-menu", cx);
        window.press("down", cx);
        window.press("enter", cx);
    });
    visual.run_until_parked();
    handle
        .read_with(cx, |preferences, _| {
            assert_eq!(
                preferences
                    .ai_preferences
                    .as_ref()
                    .unwrap()
                    .instruction
                    .command,
                crate::ai::AiCommand::Summarize
            );
        })
        .unwrap();
    cx.on_quit(move || std::fs::remove_dir_all(directory).unwrap());
}
