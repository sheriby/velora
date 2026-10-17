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
