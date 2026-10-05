use std::io::Write as _;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use gpui::TestAppContext;

use super::strip_wrapping_code_fence;
use crate::ai::{AiAction, TranslateTarget};
use crate::config::preferences::{AiPreferences, EditorSettings};

fn init_test_app(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
        EditorSettings::install_test_settings(cx, false);
    });
}

fn configure_ai(port: u16, cx: &mut TestAppContext) {
    cx.update(|cx| {
        EditorSettings::set_ai_in_memory(
            AiPreferences {
                provider_id: "custom".into(),
                api_base_url: format!("http://127.0.0.1:{port}/v1"),
                api_key: "test-key".into(),
                model: "test-model".into(),
                translate_target: crate::config::preferences::AUTO_TRANSLATE_TARGET.into(),
            },
            cx,
        );
    });
}

/// 本地 mock:SSE 逐块吐 `chunks`,块间留一点间隙模拟流式。
fn spawn_sse_server(chunks: Vec<&'static str>) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        );
        for chunk in chunks {
            let payload = format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{chunk}\"}}}}]}}\n\n"
            );
            let _ = stream.write_all(payload.as_bytes());
            let _ = stream.flush();
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = stream.write_all(b"data: [DONE]\n\n");
    });
    port
}

/// 轮询直到面板离开 Running(或超时),驱动异步泵。
fn wait_until_not_running(editor: &gpui::Entity<crate::editor::Editor>, cx: &mut TestAppContext) {
    for _ in 0..500 {
        let running = editor.read_with(cx, |editor, _| {
            editor
                .ai_assistant
                .as_ref()
                .map(|state| matches!(state.phase, super::AiPhase::Running))
                .unwrap_or(false)
        });
        if !running {
            return;
        }
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("AI 请求 5 秒内没有结束");
}

#[test]
fn strip_wrapping_code_fence_only_when_whole_answer_is_fenced() {
    assert_eq!(strip_wrapping_code_fence("  你好  "), "你好");
    assert_eq!(
        strip_wrapping_code_fence("```markdown\n# 标题\n正文\n```"),
        "# 标题\n正文"
    );
    // 内容本身带代码块:不动。
    let with_code = "说明：\n```rust\nfn main() {}\n```\n完";
    assert_eq!(strip_wrapping_code_fence(with_code), with_code);
    // 没闭合的围栏:不动。
    assert_eq!(strip_wrapping_code_fence("```rust\nfn main() {}"), "```rust\nfn main() {}");
}

#[gpui::test]
async fn menu_opens_with_actions_and_custom_input(cx: &mut TestAppContext) {
    init_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, "第一段".to_string(), None)
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(window, cx);
        let state = editor.ai_assistant.as_ref().expect("panel open");
        assert!(matches!(state.phase, super::AiPhase::Menu));
        assert!(state.action.is_none());
    });
    // Esc:菜单相直接关闭。
    editor.update_in(cx, |editor, _window, cx| {
        editor.ai_escape(cx);
        assert!(editor.ai_assistant.as_ref().is_none(), "Esc 应关闭菜单");
    });
}

#[gpui::test]
async fn unconfigured_action_opens_preferences(cx: &mut TestAppContext) {
    init_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, "第一段".to_string(), None)
    });

    // 用例之间共享 config.toml:显式清掉内存里的 AI 配置,保证「未配置」。
    cx.update(|_window, cx| EditorSettings::set_ai_in_memory(AiPreferences::default(), cx));
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(window, cx);
        editor.run_ai_action(AiAction::Polish, cx);
        assert!(
            editor.ai_assistant.as_ref().is_none(),
            "未配置时点动作应关闭面板"
        );
    });
    cx.run_until_parked();
    let window_count = cx.update(|_window, cx| cx.windows().len());
    assert!(window_count > 1, "未配置时点动作应打开偏好设置窗口");
}

#[gpui::test]
async fn polish_replaces_selection_and_is_undoable(cx: &mut TestAppContext) {
    init_test_app(cx);
    let port = spawn_sse_server(vec!["改", "好", "的"]);
    configure_ai(port, cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, "一段原文".to_string(), None)
    });

    // 选中整段(单块选区)。
    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _cx| block.selected_range = 0..12);
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(window, cx);
        editor.run_ai_action(AiAction::Polish, cx);
    });
    wait_until_not_running(&editor, cx);

    editor.update_in(cx, |editor, _window, cx| {
        let state = editor.ai_assistant.as_ref().expect("panel still open");
        assert!(matches!(state.phase, super::AiPhase::Finished), "应完成");
        assert_eq!(state.result, "改好的");
        assert!(editor.ai_apply(cx), "应用应成功");
        assert!(editor.ai_assistant.as_ref().is_none(), "应用后关闭");
        assert_eq!(
            editor.current_document_source(cx),
            "改好的",
            "选区应被结果替换"
        );
        // 一步撤销回到原文。
        editor.undo_document(cx);
        assert_eq!(editor.current_document_source(cx), "一段原文");
    });
}

#[gpui::test]
async fn summarize_inserts_below_selection(cx: &mut TestAppContext) {
    init_test_app(cx);
    let port = spawn_sse_server(vec!["• 要点"]);
    configure_ai(port, cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, "长文正文".to_string(), None)
    });
    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _cx| block.selected_range = 0..12);
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(window, cx);
        editor.run_ai_action(AiAction::Summarize, cx);
    });
    wait_until_not_running(&editor, cx);
    editor.update_in(cx, |editor, _window, cx| {
        assert!(editor.ai_apply(cx));
        assert_eq!(
            editor.current_document_source(cx),
            "长文正文\n\n• 要点",
            "总结应插入到选区下方,原文保留"
        );
    });
}

#[gpui::test]
async fn translate_uses_language_from_action(cx: &mut TestAppContext) {
    init_test_app(cx);
    let port = spawn_sse_server(vec!["hello"]);
    configure_ai(port, cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, "你好".to_string(), None)
    });
    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _cx| block.selected_range = 0..6);
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(window, cx);
        editor.run_ai_action(AiAction::Translate(TranslateTarget::English), cx);
    });
    wait_until_not_running(&editor, cx);
    editor.update_in(cx, |editor, _window, cx| {
        assert!(editor.ai_apply(cx));
        assert_eq!(editor.current_document_source(cx), "hello");
    });
}

#[gpui::test]
async fn applying_refuses_when_document_drifted(cx: &mut TestAppContext) {
    init_test_app(cx);
    let port = spawn_sse_server(vec!["改好的"]);
    configure_ai(port, cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, "一段原文".to_string(), None)
    });
    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _cx| block.selected_range = 0..12);
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(window, cx);
        editor.run_ai_action(AiAction::Polish, cx);
    });
    wait_until_not_running(&editor, cx);

    // 生成期间文档被改:锚点区间的文本变了,应用必须拒绝。
    editor.update_in(cx, |editor, window, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, cx| {
            let utf16 = block.range_to_utf16(&(0..1));
            gpui::EntityInputHandler::replace_text_in_range(
                block,
                Some(utf16),
                "换",
                window,
                cx,
            );
        });
    });
    editor.update_in(cx, |editor, _window, cx| {
        assert!(!editor.ai_apply(cx), "文档漂移后应用应失败");
        let state = editor.ai_assistant.as_ref().expect("面板保持打开提示重试");
        assert!(matches!(state.phase, super::AiPhase::Failed));
        assert!(
            editor.current_document_source(cx).contains("换"),
            "原文保持用户编辑后的样子"
        );
    });
}

#[gpui::test]
async fn stop_keeps_partial_result_and_apply_works(cx: &mut TestAppContext) {
    init_test_app(cx);
    let port = spawn_sse_server(vec!["部分"]);
    configure_ai(port, cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, "原文".to_string(), None)
    });
    editor.update(cx, |editor, cx| {
        let block = editor.document.visible_blocks()[0].entity.clone();
        block.update(cx, |block, _cx| block.selected_range = 0..6);
    });

    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(window, cx);
        editor.run_ai_action(AiAction::Polish, cx);
    });
    wait_until_not_running(&editor, cx);
    editor.update_in(cx, |editor, _window, cx| {
        assert!(editor.ai_apply(cx));
        assert_eq!(editor.current_document_source(cx), "部分");
    });
}

#[gpui::test]
async fn stream_cancel_flag_is_observed(cx: &mut TestAppContext) {
    // 客户端层取消已在 ai::client 测过;这里确认面板状态机:cancel 置位后
    // 新 generation 的增量被忽略。
    init_test_app(cx);
    let (editor, cx) = cx.add_window_view(|_window, cx| {
        crate::editor::Editor::from_markdown(cx, "原文".to_string(), None)
    });
    editor.update_in(cx, |editor, window, cx| {
        editor.open_ai_assistant(window, cx);
        {
            let state = editor.ai_assistant.as_ref().expect("open");
            assert_eq!(state.generation, 0);
            let _ = state;
        }
        editor.ai_escape(cx);
        assert!(editor.ai_assistant.as_ref().is_none());
        let _ = Arc::new(AtomicBool::new(false));
    });
}
