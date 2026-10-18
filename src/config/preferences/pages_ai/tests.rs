use super::{AppPreferences, PreferencesWindow, RunControl};
use gpui::TestAppContext;

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
        preferences.select_ai_preferences_endpoint(None, cx);
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
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
    cx.simulate_input("changed-model");
    cx.update(|window, cx| window.draw(cx).clear());
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
        cx.update(|window, cx| window.draw(cx).clear());
        cx.run_until_parked();
    }
    assert!(cx.debug_bounds("preferences-ai-model").is_none());
    let toggle = cx
        .debug_bounds("preferences-ai-enabled")
        .expect("AI 设置应从启用开关开始");
    cx.simulate_click(toggle.center(), gpui::Modifiers::none());
    cx.update(|window, cx| window.draw(cx).clear());
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
        preferences.select_ai_preferences_endpoint(None, cx);
        preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.set_value("saved-model", cx));
        preferences.save_ai_preferences(cx).unwrap();
        preferences
            .ai_preferences
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.set_value("newer-unsaved-model", cx));
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
