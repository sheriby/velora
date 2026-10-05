use gpui::{EntityInputHandler, KeyDownEvent, TestAppContext};

// 不能 use super::*:text_field 里的 gpui::* 会把 `test` 宏一起带进来,
// 截胡 #[gpui::test] 展开出的裸 #[test],宏展开无限递归。按名导入。
use super::TextField;

fn init_theme(cx: &mut gpui::App) {
    crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
    crate::theme::ThemeManager::init_with_theme_id(cx, "velora-dark");
}

fn key_event(key: &str, secondary: bool, shift: bool) -> KeyDownEvent {
    let mut keystroke = gpui::Keystroke::parse(key).expect("parse keystroke");
    if secondary {
        keystroke.modifiers = gpui::Modifiers::secondary_key();
    }
    keystroke.modifiers.shift = shift;
    KeyDownEvent {
        keystroke,
        is_held: false,
    }
}

// 纯函数测试放在不 glob 导入 gpui 的子模块里:否则 gpui::* 里的 `test`
// 宏会截胡裸 `#[test]`,把它当成 gpui::test 展开。
mod utf16_offsets {
    use crate::components::text_field::{utf16_to_utf8_offset, utf8_to_utf16_offset};

    #[test]
    fn utf16_offsets_round_trip_across_multibyte_text() {
        let text = "a中文🙂b";
        // 'a'(1) 中(1) 文(1) 🙂(2 surrogate units) b(1) → UTF-16 长度 6。
        assert_eq!(utf8_to_utf16_offset(text, text.len()), 6);
        assert_eq!(utf16_to_utf8_offset(text, 0), 0);
        assert_eq!(utf16_to_utf8_offset(text, 1), 1); // a
        assert_eq!(utf16_to_utf8_offset(text, 2), 4); // 中
        assert_eq!(utf16_to_utf8_offset(text, 3), 7); // 文 → 🙂 首
        assert_eq!(utf16_to_utf8_offset(text, 6), 12); // 末尾
        // 超界夹回末尾;劈开代理对的偏移向后夹到下一个字符边界。
        assert_eq!(utf16_to_utf8_offset(text, 99), text.len());
        assert_eq!(utf16_to_utf8_offset(text, 4), 11); // 🙂 代理对中间
        assert_eq!(utf16_to_utf8_offset(text, 5), 11); // b 首
    }
}

#[gpui::test]
async fn editing_ops_are_grapheme_and_caret_safe(cx: &mut TestAppContext) {
    cx.update(|cx| init_theme(cx));
    let (field, cx) = cx.add_window_view(|_window, cx| TextField::new("placeholder", cx));
    field.update_in(cx, |field, _window, cx| {
        field.replace_range(0..0, "中文abc", cx);
        assert_eq!(field.value(), "中文abc");
        assert_eq!(field.selected_range, field.value().len()..field.value().len());

        // 从末尾左移 2 次:依次落在「c」「b」之前(逐字素移动)。
        field.move_caret_left(false);
        field.move_caret_left(false);
        assert_eq!(field.selected_range.start, 7);

        field.replace_range(7..7, "!", cx);
        assert_eq!(field.value(), "中文a!bc");
        assert_eq!(field.selected_range.start, 8);

        // 退格删除的是「!」整个字符,不会切进多字节中间。
        field.delete_backwards(cx);
        assert_eq!(field.value(), "中文abc");

        // 选区替换:选中 "abc"(字节 6..9)换成 "x"。
        field.selected_range = 6..9;
        field.replace_range(6..9, "x", cx);
        assert_eq!(field.value(), "中文x");
        assert_eq!(field.selected_range.start, 7);

        // 前向删除:光标在「x」前,删的是「x」。
        field.move_to(6, false);
        field.delete_forwards(cx);
        assert_eq!(field.value(), "中文");
    });
}

#[gpui::test]
async fn select_all_and_shift_extend_move_caret(cx: &mut TestAppContext) {
    cx.update(|cx| init_theme(cx));
    let (field, cx) = cx.add_window_view(|_window, cx| TextField::new("p", cx));
    field.update_in(cx, |field, _window, cx| {
        field.set_value("hello 世界", cx);
        field.select_all();
        assert_eq!(field.selected_text(), "hello 世界");
        // shift+左:锚(起点)不动,头(end)向左退一个字素——选区从右端
        // 缩短,与主流文本框一致。
        field.move_caret_left(true);
        assert_eq!(field.selected_text(), "hello 世");
        // 非扩展左移:有选区时收拢到选区起点。
        field.move_caret_left(false);
        assert_eq!(field.selected_range, 0..0);
    });
}

#[gpui::test]
async fn input_handler_routes_insert_and_replacement(cx: &mut TestAppContext) {
    cx.update(|cx| init_theme(cx));
    let (field, cx) = cx.add_window_view(|_window, cx| TextField::new("p", cx));
    field.update_in(cx, |field, window, cx| {
        // 无 range 的替换 = 在选区处插入(IME 上屏与普通输入同路径)。
        field.replace_text_in_range(None, "你好", window, cx);
        assert_eq!(field.value(), "你好");
        // IME 组词:marked 区间先落,再被正式文本替换。
        field.replace_and_mark_text_in_range(None, "n", None, window, cx);
        assert_eq!(field.marked_text_range(window, cx), Some(2..3));
        field.replace_text_in_range(None, "你", window, cx);
        assert_eq!(field.value(), "你好你");
        assert_eq!(field.marked_text_range(window, cx), None);
        // UTF-16 区间替换:替换第 2 个「你」(CJK 每字符 1 个 code unit)。
        field.replace_text_in_range(Some(2..3), "它", window, cx);
        assert_eq!(field.value(), "你好它");
    });
}

#[gpui::test]
async fn enter_key_invokes_host_callback(cx: &mut TestAppContext) {
    cx.update(|cx| init_theme(cx));
    let (field, cx) = cx.add_window_view(|_window, cx| {
        TextField::new("p", cx).on_enter(|field, _window, _cx| {
            field.set_value("submitted", _cx);
        })
    });
    field.update_in(cx, |field, window, cx| {
        field.set_value("draft", cx);
        field.handle_key_down(&key_event("enter", false, false), window, cx);
        assert_eq!(field.value(), "submitted");
        // escape 不被输入框吞掉:留给宿主面板关闭。
        field.handle_key_down(&key_event("escape", false, false), window, cx);
    });
}

#[gpui::test]
async fn clipboard_shortcuts_copy_cut_and_paste(cx: &mut TestAppContext) {
    cx.update(|cx| init_theme(cx));
    let (field, cx) = cx.add_window_view(|_window, cx| TextField::new("p", cx));
    field.update_in(cx, |field, window, cx| {
        field.set_value("hello", cx);
        field.select_all();
        field.handle_key_down(&key_event("c", true, false), window, cx);
        assert_eq!(field.value(), "hello");
        field.handle_key_down(&key_event("x", true, false), window, cx);
        assert_eq!(field.value(), "");
        // 粘贴内容里的换行折成空格(单行输入不截断)。
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("a\nb".to_string()));
        field.handle_key_down(&key_event("v", true, false), window, cx);
        assert_eq!(field.value(), "a b");
    });
}
