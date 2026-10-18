use gpui::{
    Entity, InteractiveElement as _, IntoElement as _, Orientation, ParentElement as _, Pixels,
    Role, StatefulInteractiveElement as _, Styled as _, TestSupportExt as _, div,
    prelude::FluentBuilder as _, px,
};
use gpui_kit::base::{HandleEdge, ResizeHandleRenderer};
use gpui_kit::component::{
    ThemeStyled as _,
    resizable::{ResizableState, resize_handle_appearance},
};

pub(crate) struct PanelResizeHandle {
    pub id: &'static str,
    pub label: &'static str,
    pub state: Entity<ResizableState>,
    pub focus: gpui::FocusHandle,
    pub panel_index: usize,
    pub edge: HandleEdge,
    pub minimum: Pixels,
    pub maximum: Pixels,
    pub initial: Pixels,
}

impl PanelResizeHandle {
    pub fn appearance(self) -> ResizeHandleRenderer {
        let native = resize_handle_appearance();
        std::rc::Rc::new(move |handle, window, cx| {
            let line = native(handle, window, cx);
            let horizontal = handle.axis() == gpui::Axis::Horizontal;
            let current = self
                .state
                .read(cx)
                .sizes()
                .get(self.panel_index)
                .copied()
                .unwrap_or(self.initial);
            let state = self.state.clone();
            let panel_index = self.panel_index;
            let minimum = self.minimum;
            let maximum = self.maximum;
            let initial = self.initial;
            let edge = self.edge;
            // Kit 0.7 负责指针和尺寸算法；应用只补齐键盘及可访问语义。
            Some(
                div()
                    .id(self.id)
                    .debug_selector(move || self.id.to_owned())
                    .test_support()
                    // 外部句柄不继承元素的 Tab 属性，必须配置句柄自身。
                    .track_focus(&self.focus.clone().tab_index(0).tab_stop(true))
                    .role(Role::Splitter)
                    .aria_label(self.label)
                    .aria_orientation(if horizontal {
                        Orientation::Vertical
                    } else {
                        Orientation::Horizontal
                    })
                    .aria_numeric_value(f64::from(f32::from(current)))
                    .aria_min_numeric_value(f64::from(f32::from(minimum)))
                    .aria_max_numeric_value(f64::from(f32::from(maximum)))
                    // 与库的单像素分隔线共享边界，焦点环在边界外绘制。
                    .relative()
                    .map(|line| {
                        if horizontal {
                            line.w(px(1.0)).h_full()
                        } else {
                            line.h(px(1.0)).w_full()
                        }
                    })
                    .when(
                        self.focus.is_focused(window) && window.last_input_was_keyboard(),
                        |line| line.focus_ring_style(window, cx),
                    )
                    .on_key_down(
                        move |event: &gpui::KeyDownEvent,
                              window: &mut gpui::Window,
                              cx: &mut gpui::App| {
                            let modifiers = event.keystroke.modifiers;
                            if modifiers.control
                                || modifiers.platform
                                || modifiers.alt
                                || modifiers.function
                            {
                                return;
                            }
                            let step = window.rem_size() * if modifiers.shift { 4.0 } else { 1.0 };
                            let current = state
                                .read(cx)
                                .sizes()
                                .get(panel_index)
                                .copied()
                                .unwrap_or(initial);
                            let negative = if horizontal { "left" } else { "up" };
                            let positive = if horizontal { "right" } else { "down" };
                            let grow = if edge == HandleEdge::Leading {
                                negative
                            } else {
                                positive
                            };
                            let key = event.keystroke.key.as_str();
                            let size = match key {
                                "home" => minimum,
                                "end" => maximum,
                                key if key == grow => current + step,
                                key if key == negative || key == positive => current - step,
                                _ => return,
                            };
                            state.update(cx, |state, cx| {
                                state.resize_panel(panel_index, size, window, cx)
                            });
                            cx.stop_propagation();
                        },
                    )
                    .children(line)
                    .into_any_element(),
            )
        })
    }
}
