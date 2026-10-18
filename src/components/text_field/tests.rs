use gpui::{
    AppContext, EntityInputHandler, InteractiveElement, KeyDownEvent, ParentElement, Styled,
    TestAppContext,
};

// 不能 use super::*:text_field 里的 gpui::* 会把 `test` 宏一起带进来,
// 截胡 #[gpui::test] 展开出的裸 #[test],宏展开无限递归。按名导入。
use super::TextField;

fn init_theme(cx: &mut gpui::App) {
    crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
    crate::theme::ThemeManager::init_with_theme_id(cx, "velora-dark");
}

#[gpui::test]
fn ai_text_fields_show_a_caret_and_place_mouse_input_at_the_clicked_character(
    cx: &mut TestAppContext,
) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("placeholder", cx));
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value("abc中文🙂", cx);
        field.focus(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let target = field.read_with(cx, |field, _| {
        assert!(field.painted_caret, "聚焦输入框必须实际绘制光标");
        let bounds = field.last_bounds.unwrap();
        let line = field.last_line.as_ref().unwrap();
        gpui::point(bounds.left() + line.x_for_index(2), bounds.center().y)
    });
    cx.simulate_mouse_down(target, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::none());
    field.update_in(cx, |field, window, cx| {
        assert_eq!(field.selected_range, 2..2);
        field.replace_text_in_range(None, "X", window, cx);
        assert_eq!(field.value(), "abXc中文🙂");
    });
}

#[gpui::test]
fn ai_text_fields_keep_ime_selection_relative_to_the_inserted_composition(cx: &mut TestAppContext) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx));
    field.update_in(cx, |field, window, cx| {
        field.set_value("前文", cx);
        field.replace_and_mark_text_in_range(None, "中", Some(1..1), window, cx);
        assert_eq!(field.selected_range, "前文中".len().."前文中".len());
        assert_eq!(field.marked_range, Some("前文".len().."前文中".len()));
        field.replace_text_in_range(None, "中文", window, cx);
        assert_eq!(field.value(), "前文中文");
    });
}

fn key_event(key: &str, secondary: bool, shift: bool) -> KeyDownEvent {
    let mut keystroke = gpui::Keystroke::parse(key).expect("parse keystroke");
    if secondary {
        keystroke.modifiers = gpui::Modifiers::secondary_key();
    }
    keystroke.modifiers.shift = shift;
    KeyDownEvent {
        prefer_character_input: false,
        keystroke,
        is_held: false,
    }
}

// 纯函数测试放在不 glob 导入 gpui 的子模块里:否则 gpui::* 里的 `test`
// 宏会截胡裸 `#[test]`,把它当成 gpui::test 展开。
mod utf16_offsets {
    use crate::components::text_field::{utf8_to_utf16_offset, utf16_to_utf8_offset};

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
        assert_eq!(
            field.selected_range,
            field.value().len()..field.value().len()
        );

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
async fn single_line_enter_keeps_text_and_escape_reaches_the_host(cx: &mut TestAppContext) {
    cx.update(|cx| init_theme(cx));
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("p", cx));
    field.update_in(cx, |field, window, cx| {
        field.set_value("draft", cx);
        field.handle_key_down(&key_event("enter", false, false), window, cx);
        assert_eq!(field.value(), "draft");
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

#[gpui::test]
fn ai_text_fields_collapse_reversed_selections_at_the_expected_end(cx: &mut TestAppContext) {
    cx.update(init_theme);
    let field = cx.new(|cx| TextField::new("", cx));
    field.update(cx, |field, cx| {
        field.set_value("ab中文", cx);
        field.selected_range = 8..1;
        field.move_caret_left(false);
        assert_eq!(field.selected_range, 1..1);
        field.selected_range = 8..1;
        field.move_caret_right(false);
        assert_eq!(field.selected_range, 8..8);
    });
}

#[gpui::test]
fn ai_text_fields_render_reversed_selection_and_report_its_direction(cx: &mut TestAppContext) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx));
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value("ab中文", cx);
        field.focus(window, cx);
        field.selected_range = field.value.len()..0;
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    field.update_in(cx, |field, window, cx| {
        let selection = field.selected_text_range(false, window, cx).unwrap();
        assert_eq!(
            (
                field.painted_selection,
                field.painted_caret,
                selection.reversed
            ),
            (true, false, true),
            "反向选择必须有高亮，并向输入法报告真实方向"
        );
    });
}

#[gpui::test]
fn multiline_input_reserves_rows_and_preserves_newlines_on_paste(cx: &mut TestAppContext) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx).multiline(3, 8));
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.focus(window, cx);
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("甲\n乙🙂".into()));
    });
    cx.run_until_parked();
    field.read_with(cx, |field, _| {
        assert!(
            field.last_bounds.unwrap().size.height > gpui::px(40.0),
            "多行编辑器必须预留实际行高"
        )
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    });
    cx.simulate_keystrokes("enter");
    cx.simulate_input("中文");
    field.read_with(cx, |field, _| assert_eq!(field.value(), "甲\n乙🙂\n中文"));
}


#[gpui::test]
fn multiline_input_up_arrow_moves_between_rows_without_changing_text(cx: &mut TestAppContext) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx).multiline(3, 6));
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value("abc\nxyz", cx);
        field.focus(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("up");
    field.read_with(cx, |field, _| {
        assert!(field.selected_range.end <= 3, "向上应回到第一行");
        assert_eq!(field.value(), "abc\nxyz");
    });
}

#[gpui::test]
fn multiline_input_undo_keeps_an_ime_composition_as_one_edit(cx: &mut TestAppContext) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx).multiline(3, 6));
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value("前", cx);
        field.focus(window, cx);
        field.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
        field.replace_and_mark_text_in_range(None, "你", Some(1..1), window, cx);
        field.replace_text_in_range(None, "你好", window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    });
    field.read_with(cx, |field, _| {
        assert_eq!(
            field.value(),
            "前",
            "一次撤销应移除整个组词，不留下拼音或候选字"
        )
    });
}

#[gpui::test]
fn multiline_input_mouse_and_ime_follow_the_second_row(cx: &mut TestAppContext) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx).multiline(3, 3));
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value("ab\n中文🙂\nz", cx);
        field.focus(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let target = field.read_with(cx, |field, _| {
        let state = field.multiline.as_ref().unwrap();
        let layout = state.layout.as_ref().unwrap();
        field.last_bounds.unwrap().origin
            + layout.position("ab\n中".len())
            + gpui::point(gpui::px(0.0), layout.line_height * 0.5)
    });
    cx.simulate_mouse_down(target, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::none());
    field.update_in(cx, |field, window, cx| {
        assert_eq!(field.selected_range.end, "ab\n中".len());
        let bounds = field.last_bounds.unwrap();
        let ime = field.bounds_for_range(4..4, bounds, window, cx).unwrap();
        assert!(ime.top() > bounds.top(), "输入法候选框不能留在第一行");
        field.replace_text_in_range(None, "X", window, cx);
        assert_eq!(field.value(), "ab\n中X文🙂\nz");
    });
}

#[gpui::test]
fn multiline_input_wraps_without_changing_text_and_limits_visible_rows(cx: &mut TestAppContext) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx).multiline(1, 2));
    let text = "word ".repeat(200);
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value(&text, cx);
        field.focus(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    field.read_with(cx, |field, _| {
        let state = field.multiline.as_ref().unwrap();
        let layout = state.layout.as_ref().unwrap();
        assert!(
            !layout
                .rows
                .first()
                .unwrap()
                .line
                .wrap_boundaries()
                .is_empty()
        );
        assert!(field.last_bounds.unwrap().size.height <= layout.line_height * 2);
        assert!(
            state.scroll_y > gpui::px(0.0),
            "长输入应在控件内部滚动显示光标"
        );
        assert_eq!(field.value(), text);
    });
}

// 观察输入框 notify 的宿主视图:无头测试里帧只在显式 draw 时绘制,
// 「unmark 后下一帧下划线消失」只能钉成「unmark 触发 notify(调度重绘)
// 且随后那一帧不再画下划线 quad」,notify 本身是会红的那一半。
struct UnmarkObserverHarness {
    field: gpui::Entity<TextField>,
    notifications: std::rc::Rc<std::cell::Cell<u32>>,
    _subscription: gpui::Subscription,
}

impl UnmarkObserverHarness {
    fn new(cx: &mut gpui::Context<Self>) -> Self {
        let field = cx.new(|cx| TextField::new("", cx));
        let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = notifications.clone();
        let _subscription =
            cx.observe(&field, move |_, _, _| counter.set(counter.get() + 1));
        Self {
            field,
            notifications,
            _subscription,
        }
    }
}

impl gpui::Render for UnmarkObserverHarness {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        self.field.clone()
    }
}

#[gpui::test]
fn unmark_text_requests_the_repaint_that_clears_the_composition_underline(
    cx: &mut TestAppContext,
) {
    cx.update(init_theme);
    let (harness, cx) = cx.add_window_view(|_, cx| UnmarkObserverHarness::new(cx));
    let field = harness.read_with(cx, |harness, _| harness.field.clone());
    field.update_in(cx, |field, window, cx| {
        field.set_value("hello", cx);
        field.replace_and_mark_text_in_range(Some(0..5), "mark", None, window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let before = harness.read_with(cx, |harness, _| harness.notifications.get());
    field.update_in(cx, |field, window, cx| {
        assert!(field.painted_marked, "组词期必须画出下划线 quad");
        field.unmark_text(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    harness.read_with(cx, |harness, _| {
        assert!(
            harness.notifications.get() > before,
            "unmark 必须请求重绘,否则下划线要等下一次无关重绘才消失"
        );
    });
    field.update_in(cx, |field, _, _| {
        assert!(!field.painted_marked, "unmark 之后的帧不能再画组词下划线");
    });
}

#[gpui::test]
fn multiline_input_down_arrow_on_the_last_row_keeps_the_caret_at_the_end(
    cx: &mut TestAppContext,
) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx).multiline(3, 6));
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value("abc\nxyz", cx);
        field.focus(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("down");
    field.read_with(cx, |field, _| {
        assert_eq!(
            field.selected_range.end,
            "abc\nxyz".len(),
            "末行行尾按 Down 应停在文本末尾,不能落回末行行首"
        );
    });
}

#[gpui::test]
fn multiline_input_click_below_the_text_lands_at_the_end_of_the_last_row(
    cx: &mut TestAppContext,
) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx).multiline(3, 6));
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value("abc\nxyz", cx);
        field.focus(window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let target = field.read_with(cx, |field, _| {
        let state = field.multiline.as_ref().unwrap();
        let layout = state.layout.as_ref().unwrap();
        let end = layout.position(field.value().len());
        field.last_bounds.unwrap().origin
            + gpui::point(end.x + gpui::px(4.0), layout.line_height * 2.5)
    });
    cx.simulate_mouse_down(target, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::none());
    field.read_with(cx, |field, _| {
        assert_eq!(
            field.selected_range.end,
            "abc\nxyz".len(),
            "点击末行下方的留白区应落到文本末尾,不能落回末行行首"
        );
    });
}

// 输入框上方带一块空白区域的宿主视图:模拟「框内按下 → 框外释放」,
// 再从空白区发起一次拖动路过输入框,验证选区不被改写。
struct DragOutsideHarness {
    field: gpui::Entity<TextField>,
}

impl gpui::Render for DragOutsideHarness {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                gpui::div()
                    .id("outside-area")
                    .h(gpui::px(120.0))
                    .w_full()
                    .flex_none(),
            )
            .child(gpui::div().py(gpui::px(8.0)).child(self.field.clone()))
    }
}

#[gpui::test]
fn releasing_a_drag_outside_the_field_does_not_hijack_later_drags(cx: &mut TestAppContext) {
    cx.update(init_theme);
    let (harness, cx) = cx.add_window_view(|_, cx| DragOutsideHarness {
        field: cx.new(|cx| TextField::new("", cx)),
    });
    let field = harness.read_with(cx, |harness, _| harness.field.clone());
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value("0123456789", cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let (inside_two, inside_eight, inside_four, outside) = field.read_with(cx, |field, _| {
        let bounds = field.last_bounds.unwrap();
        let line = field.last_line.as_ref().unwrap();
        let at = |offset: usize| {
            gpui::point(bounds.left() + line.x_for_index(offset), bounds.center().y)
        };
        (
            at(2),
            at(8),
            at(4),
            gpui::point(bounds.center().x, bounds.top() - gpui::px(60.0)),
        )
    });
    // 框内按下并拖出选区后,在框外释放:输入框收不到 on_mouse_up。
    cx.simulate_mouse_down(inside_two, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(inside_eight, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(outside, gpui::MouseButton::Left, gpui::Modifiers::none());
    field.read_with(cx, |field, _| {
        assert_eq!(field.selected_range, 2..8, "框内拖出选区应成立");
    });
    // 从框外另起一次拖动,路过本框:本框选区不能被这次路过的拖动改写。
    cx.simulate_mouse_down(outside, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(inside_four, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_up(inside_four, gpui::MouseButton::Left, gpui::Modifiers::none());
    field.read_with(cx, |field, _| {
        assert_eq!(
            field.selected_range, 2..8,
            "框外释放后,别的拖动路过本框不能改写本框选区"
        );
    });
}

#[gpui::test]
fn multiline_input_undo_collapses_sub_range_edits_inside_one_composition(
    cx: &mut TestAppContext,
) {
    cx.update(init_theme);
    let (field, cx) = cx.add_window_view(|_, cx| TextField::new("", cx).multiline(3, 6));
    field.update_in(cx, |field, window, cx| {
        window.activate_window();
        field.set_value("前", cx);
        field.focus(window, cx);
        // 组词「nihao」整段上屏。
        field.replace_and_mark_text_in_range(None, "nihao", None, window, cx);
        // 分段确认:只把组词前段「ni」(UTF-16 1..3)换成「你」。
        field.replace_and_mark_text_in_range(Some(1..3), "你", None, window, cx);
        assert_eq!(field.value(), "前你hao");
        // 整段提交组词区域(UTF-16 1..6 = 「你hao」)。
        field.replace_text_in_range(Some(1..6), "你好", window, cx);
        assert_eq!(field.value(), "前你好");
        field.handle_key_down(&key_event("z", true, false), window, cx);
        assert_eq!(
            field.value(),
            "前",
            "一次撤销应回到组词前,组词内的子区间替换不能拆成多步撤销"
        );
    });
}
