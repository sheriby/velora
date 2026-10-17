use super::{AiCommand, Editor, PanelMode, RunControl};
use gpui::{TestAppContext, VisualTestContext};

fn initialize(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear());
    cx.run_until_parked();
}

#[gpui::test]
fn editing_connection_inputs_cancels_the_old_probe_and_removes_its_status(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    let control = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.control = RunControl::default();
        session.activity = "old connection status".into();
        session
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.focus(window, cx));
        session.control.clone()
    });
    draw(cx);
    draw(cx);
    cx.simulate_input("changed-model");
    draw(cx);
    editor.read_with(cx, |editor, _| {
        assert!(
            editor.ai_session.as_ref().unwrap().activity.is_empty(),
            "修改连接后不能继续显示旧连接的测试状态"
        )
    });
    assert!(control.cancel.is_cancelled());
}

#[gpui::test]
fn editing_connection_inputs_does_not_cancel_a_running_document_task(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    let control = editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        let session = editor.ai_session.as_mut().unwrap();
        session.control = RunControl::default();
        session.saved.running = true;
        session.generation = 20;
        session.activity = "read_document".into();
        session
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.focus(window, cx));
        session.control.clone()
    });
    draw(cx);
    draw(cx);
    cx.simulate_input("changed-model");
    draw(cx);
    assert!(!control.cancel.is_cancelled());
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.ai_session.as_ref().unwrap().generation, 20)
    });
}

#[gpui::test]
fn a_background_connection_save_keeps_newer_input_in_the_form(cx: &mut TestAppContext) {
    initialize(cx);
    let (editor, cx) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(AiCommand::Ask, window, cx);
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.set_value("saved-model", cx));
        editor.save_ai_endpoint(cx);
        editor
            .ai_session
            .as_ref()
            .unwrap()
            .draft
            .as_ref()
            .unwrap()
            .model
            .update(cx, |field, cx| field.set_value("newer-unsaved-model", cx));
        editor.invalidate_ai_connection_test(cx);
    });
    cx.run_until_parked();
    draw(cx);
    editor.read_with(cx, |editor, cx| {
        let session = editor.ai_session.as_ref().unwrap();
        assert!(
            session.draft.is_some(),
            "异步保存完成不能丢弃随后输入的内容"
        );
        assert!(session.mode == PanelMode::Settings);
        assert_eq!(
            session.draft.as_ref().unwrap().model.read(cx).value(),
            "newer-unsaved-model"
        );
        assert_eq!(session.settings.endpoints[0].model, "saved-model");
    });
}
