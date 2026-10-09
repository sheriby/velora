//! 应用内模态对话框。
//!
//! 用户要求：整个软件不使用系统原生弹窗（gpui 的 `window.prompt` 在 macOS 上就是
//! 原生 NSAlert）。原先 38 处确认/提示都走原生框，现在统一走这里的模态：外观沿用
//! 其它应用内对话框的 `dialog_*` token，行为是「一个模态 + 若干按钮 + 一次性回调」，
//! Esc 与点遮罩等价于按下取消按钮。

use gpui::*;

use crate::i18n::I18nManager;
use crate::theme::Theme;

use super::Editor;

/// 模态的显示内容 + 按钮语义。
pub(crate) struct ModalSpec {
    pub(crate) title: SharedString,
    pub(crate) detail: Option<SharedString>,
    pub(crate) buttons: Vec<SharedString>,
    /// 主按钮（高亮，回车触发）。
    pub(crate) default_index: usize,
    /// Esc / 点遮罩对应的按钮。
    pub(crate) cancel_index: usize,
}

/// 当前显示中的模态。同一时刻只允许一个（与其它 overlay 互斥）。
pub(crate) struct EditorModal {
    spec: ModalSpec,
    on_choice: Option<Box<dyn FnOnce(usize, &mut Editor, &mut Window, &mut Context<Editor>)>>,
}

impl Editor {
    pub(crate) fn show_modal(
        &mut self,
        spec: ModalSpec,
        on_choice: impl FnOnce(usize, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let button_count = spec.buttons.len().max(1);
        let spec = ModalSpec {
            default_index: spec.default_index.min(button_count - 1),
            cancel_index: spec.cancel_index.min(button_count - 1),
            ..spec
        };
        self.modal = Some(EditorModal {
            spec,
            on_choice: Some(Box::new(on_choice)),
        });
        cx.notify();
    }

    /// 只有一个「好」按钮的提示框（取代 `PromptLevel::Info`/`Warning` 原生框）。
    pub(crate) fn show_message_modal(
        &mut self,
        title: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let ok: SharedString = cx.global::<I18nManager>().strings().info_dialog_ok.clone().into();
        self.show_modal(
            ModalSpec {
                title: title.into(),
                detail: Some(detail.into()),
                buttons: vec![ok],
                default_index: 0,
                cancel_index: 0,
            },
            move |_choice, _editor, _window, _cx| {},
            cx,
        );
    }

    #[cfg(test)]
    pub(crate) fn modal_spec(&self) -> Option<&ModalSpec> {
        self.modal.as_ref().map(|modal| &modal.spec)
    }

    pub(crate) fn modal_is_open(&self) -> bool {
        self.modal.is_some()
    }

    /// 按钮序号（含 Esc/遮罩映射的取消位）被触发：先关模态再跑回调，
    /// 回调里可以再开下一个模态（例如「保存后再关」链式流程）。
    pub(crate) fn dismiss_modal(
        &mut self,
        choice: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut modal) = self.modal.take() else {
            return;
        };
        let on_choice = modal.on_choice.take();
        drop(modal);
        if let Some(on_choice) = on_choice {
            on_choice(choice, self, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn cancel_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cancel_index) = self.modal.as_ref().map(|modal| modal.spec.cancel_index) else {
            return;
        };
        self.dismiss_modal(cancel_index, window, cx);
    }

    /// 当前模态的默认按钮下标（模态未打开时为 0）。
    pub(crate) fn modal_default_index(&self) -> usize {
        self.modal
            .as_ref()
            .map(|modal| modal.spec.default_index)
            .unwrap_or(0)
    }

    /// 模态打开时的按键处理（C13）。返回 true 表示按键已被模态消费。
    ///
    /// 模态自身不抢焦点（抢焦点会跟编辑器的焦点岛打架，C12 时实测按键
    /// 收不到）。也不能走元素级 key_down 监听：gpui 的 keymap 绑定先于
    /// 监听派发，回车会被焦点块的 Newline 绑定消费掉。所以调用点是
    /// `App::intercept_keystrokes`（绑定解析之前，stop_propagation 可
    /// 阻断 action 派发）——见 render.rs 的注册处。
    pub(crate) fn modal_handle_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.modal_is_open() {
            return false;
        }
        let modifiers = keystroke.modifiers;
        let plain = !modifiers.control
            && !modifiers.alt
            && !modifiers.platform
            && !modifiers.function
            && !modifiers.shift;
        match keystroke.key.as_str() {
            "enter" if plain => {
                let default_index = self.modal_default_index();
                cx.stop_propagation();
                self.dismiss_modal(default_index, window, cx);
                true
            }
            "escape" => {
                cx.stop_propagation();
                self.cancel_modal(window, cx);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn render_modal_overlay(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let modal = self.modal.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let title = modal.spec.title.clone();
        let detail = modal.spec.detail.clone();
        let default_index = modal.spec.default_index;

        let panel = div()
            .id("editor-modal")
            .w(px(d.dialog_width))
            .max_w(relative(1.0))
            .flex()
            .flex_col()
            .gap(px(d.dialog_gap))
            .p(px(d.dialog_padding))
            .bg(c.dialog_surface)
            .border(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .rounded(px(d.dialog_radius))
            .shadow_lg()
            .child(
                div()
                    .text_size(px(t.dialog_title_size))
                    .font_weight(t.dialog_title_weight.to_font_weight())
                    .text_color(c.dialog_title)
                    .child(title),
            );
        let panel = match detail {
            Some(detail) => panel.child(
                div()
                    .text_size(px(t.dialog_body_size))
                    .font_weight(t.dialog_body_weight.to_font_weight())
                    .line_height(rems(t.text_line_height))
                    .text_color(c.dialog_body)
                    .child(detail),
            ),
            None => panel,
        };

        let mut row = div()
            .flex()
            .flex_row()
            .justify_end()
            .gap(px(d.dialog_button_gap));
        for index in 0..modal.spec.buttons.len() {
            let label = modal.spec.buttons[index].clone();
            let prominent = index == default_index;
            let hover_bg = if prominent {
                c.dialog_primary_button_hover
            } else {
                c.dialog_secondary_button_hover
            };
            row = row.child(
                div()
                    .id(("editor-modal-button", index))
                    .debug_selector(move || format!("editor-modal-button-{index}"))
                    .h(px(d.dialog_button_height))
                    .px(px(d.dialog_button_padding_x))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px((d.dialog_radius - 4.0).max(0.0)))
                    .border(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .bg(if prominent {
                        c.dialog_primary_button_bg
                    } else {
                        c.dialog_secondary_button_bg
                    })
                    .text_size(px(t.dialog_button_size))
                    .font_weight(t.dialog_button_weight.to_font_weight())
                    .text_color(if prominent {
                        c.dialog_primary_button_text
                    } else {
                        c.dialog_secondary_button_text
                    })
                    .hover(move |this: StyleRefinement| this.bg(hover_bg))
                    .active(|this| this.opacity(0.92))
                    .cursor(gpui::CursorStyle::PointingHand)
                    .child(label)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |editor, _event: &MouseDownEvent, window, cx| {
                            // 不吃掉的话遮罩也会收到同一次按下，会把回调里
                            // 刚打开的下一个模态一起关掉。
                            cx.stop_propagation();
                            editor.dismiss_modal(index, window, cx);
                        }),
                    ),
            );
        }

        Some(
            div()
                .id("editor-modal-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(c.dialog_backdrop)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |editor, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        editor.cancel_modal(window, cx);
                    }),
                )
                .child(
                    div()
                        .w_full()
                        .px(px(d.editor_padding))
                        .flex()
                        .justify_center()
                        .child(panel.child(row)),
                ),
        )
    }
}
