use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;
use gpui_kit::base::{ElementExt as _, FocusTrapElement as _};
use gpui_kit::component::{
    Icon, Sizable as _,
    button::{Button, ButtonVariants},
};

use super::Editor;
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::Theme;

const MERMAID_PREVIEW_MAX_SCALE: f32 = 2.0;

fn preview_wheel_zoom_factor(event: &ScrollWheelEvent) -> Option<f32> {
    let delta = f32::from(event.delta.pixel_delta(px(20.0)).y);
    (delta.is_finite() && delta != 0.0)
        .then(|| (delta / 100.0).exp().clamp(1.0 / 1.2, 1.2))
}

pub(super) struct ImageFilePreview {
    pub(super) path: PathBuf,
    pub(super) image: Option<Arc<RenderImage>>,
    error: Option<String>,
    zoom: f32,
    /// 放大后超出视口的平移量（拖拽产生），按缩放后的图片与视口钳制。
    pan: Point<Pixels>,
    drag_start: Option<(Point<Pixels>, Point<Pixels>)>,
    _load_task: Task<()>,
}

impl ImageFilePreview {
    fn clamp_pan(&mut self, viewport: Size<Pixels>) {
        let Some(image) = self.image.as_ref() else {
            self.pan = point(px(0.0), px(0.0));
            return;
        };
        // 元素被 max_w/max_h(relative(zoom)) 约束成「contain 缩放」：渲染尺寸 =
        // contain(原始, 视口) × zoom。钳制必须按渲染尺寸——按原始尺寸算，
        // zoom=1 就能拖出空白，大 zoom 能把图整体拖出视口。
        let natural_width = image.size(0).width.0 as f32;
        let natural_height = image.size(0).height.0 as f32;
        let fit = (f32::from(viewport.width) / natural_width)
            .min(f32::from(viewport.height) / natural_height)
            .min(1.0);
        let scaled_width = px(natural_width * fit * self.zoom);
        let scaled_height = px(natural_height * fit * self.zoom);
        let max_x = ((scaled_width - viewport.width) / 2.0).max(px(0.0));
        let max_y = ((scaled_height - viewport.height) / 2.0).max(px(0.0));
        self.pan.x = self.pan.x.min(max_x).max(-max_x);
        self.pan.y = self.pan.y.min(max_y).max(-max_y);
    }
}

pub(super) struct MermaidPreview {
    rendered: crate::components::MermaidSvgRender,
    source: crate::components::MermaidSource,
    dark: bool,
    scale: Option<f32>,
    viewport: Size<Pixels>,
    pan: Point<Pixels>,
    drag_start: Option<(Point<Pixels>, Point<Pixels>)>,
    /// 浮层自己的焦点：打开时把焦点从正文拿过来，Tab 与 Enter 才能在工具条上走。
    pub(super) focus: FocusHandle,
    /// 打开前窗口里那个焦点，关闭时还回去；`None` 表示还没挂载过。
    restore_focus: Option<Option<WeakFocusHandle>>,
}

impl MermaidPreview {
    fn fit_scale(&self) -> f32 {
        (f32::from(self.viewport.width) / self.rendered.display_width)
            .min(f32::from(self.viewport.height) / self.rendered.display_height)
            .min(1.0)
    }

    fn scale(&self) -> f32 {
        self.scale
            .unwrap_or_else(|| self.fit_scale())
            .clamp(self.fit_scale(), MERMAID_PREVIEW_MAX_SCALE)
    }

    fn clamp_pan(&mut self) {
        let scale = self.scale();
        let maximum_x =
            ((self.rendered.display_width * scale - f32::from(self.viewport.width)) / 2.0).max(0.0);
        let maximum_y = ((self.rendered.display_height * scale - f32::from(self.viewport.height))
            / 2.0)
            .max(0.0);
        self.pan.x = px(f32::from(self.pan.x).clamp(-maximum_x, maximum_x));
        self.pan.y = px(f32::from(self.pan.y).clamp(-maximum_y, maximum_y));
    }
}

impl Editor {
    pub(super) fn open_mermaid_preview(&mut self, markdown: &str, cx: &mut Context<Self>) {
        let dark = cx.global::<crate::theme::ThemeManager>().current().is_dark();
        let result = crate::components::parse_mermaid_fence_source(markdown)
            .ok_or_else(|| anyhow::anyhow!("无法读取图表源码"))
            .and_then(|source| {
                crate::components::render_mermaid_svg_for_theme(&source, f32::MAX, dark).map(|rendered| (source, rendered))
            });
        match result {
            Ok((source, rendered)) => {
                self.mermaid_preview = Some(MermaidPreview {
                    rendered,
                    source,
                    dark,
                    scale: None,
                    viewport: size(px(1.0), px(1.0)),
                    pan: point(px(0.0), px(0.0)),
                    drag_start: None,
                    focus: cx.focus_handle(),
                    restore_focus: None,
                });
                cx.notify();
            }
            Err(error) => self.show_message_modal("Mermaid", error.to_string(), cx),
        }
    }

    /// 关掉预览并把焦点还给打开前那个控件（正文块）；浮层占走焦点，退出就得交回。
    pub(super) fn close_mermaid_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = self.mermaid_preview.take() else {
            return;
        };
        if let Some(Some(previous)) = preview.restore_focus
            && let Some(previous) = previous.upgrade()
        {
            previous.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn sync_mermaid_preview_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = cx.global::<crate::theme::ThemeManager>().current().is_dark();
        let Some(preview) = self.mermaid_preview.as_mut().filter(|preview| preview.dark != dark) else {
            return;
        };
        match crate::components::render_mermaid_svg_for_theme(&preview.source, f32::MAX, dark) {
            Ok(rendered) => {
                preview.rendered = rendered;
                preview.dark = dark;
                preview.clamp_pan();
            }
            Err(error) => {
                // 关闭走正式出口：浮层已经把焦点从正文抢走，直接置空会让焦点
                // 悬空，用户得用鼠标点回正文才能继续打字。
                self.close_mermaid_preview(window, cx);
                self.show_message_modal("Mermaid", error.to_string(), cx);
            }
        }
    }

    fn zoom_mermaid_preview(&mut self, factor: f32, cx: &mut Context<Self>) {
        let Some(preview) = self.mermaid_preview.as_mut() else {
            return;
        };
        let old_scale = preview.scale();
        let scale = (old_scale * factor).clamp(preview.fit_scale(), MERMAID_PREVIEW_MAX_SCALE);
        preview.scale = Some(scale);
        preview.pan = point(
            preview.pan.x * (scale / old_scale),
            preview.pan.y * (scale / old_scale),
        );
        preview.drag_start = None;
        preview.clamp_pan();
        cx.notify();
    }

    fn reset_mermaid_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(preview) = self.mermaid_preview.as_mut() {
            preview.scale = None;
            preview.pan = point(px(0.0), px(0.0));
            preview.drag_start = None;
            cx.notify();
        }
    }

    pub(super) fn mermaid_preview_key_down(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.mermaid_preview.is_none() {
            return false;
        }
        match keystroke.key.as_str() {
            "escape" => {
                self.close_mermaid_preview(window, cx);
            }
            "+" | "=" => self.zoom_mermaid_preview(1.2, cx),
            "-" => self.zoom_mermaid_preview(1.0 / 1.2, cx),
            "0" => self.reset_mermaid_preview(cx),
            _ => {}
        }
        cx.stop_propagation();
        true
    }

    pub(super) fn render_mermaid_preview_overlay(
        &mut self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let preview = self.mermaid_preview.as_mut()?;
        let viewport = size(
            (window.viewport_size().width - px(48.0)).max(px(1.0)),
            (window.viewport_size().height - px(104.0)).max(px(1.0)),
        );
        preview.viewport = viewport;
        preview.clamp_pan();
        let scale = preview.scale();
        let width = px(preview.rendered.display_width * scale);
        let height = px(preview.rendered.display_height * scale);
        let origin = point(
            (viewport.width - width) / 2.0 + preview.pan.x,
            (viewport.height - height) / 2.0 + preview.pan.y,
        );
        let path = preview.rendered.path.clone();
        let overlay_focus = preview.focus.clone();
        let owner = cx.entity().downgrade();
        let c = &theme.colors;
        let strings = cx.global::<I18nManager>().strings();
        let reset_label = strings.command_zoom_reset.clone();
        let zoom_in_label = strings.command_zoom_in.clone();
        let zoom_out_label = strings.command_zoom_out.clone();
        let close_label = strings.file_history_close.clone();
        // 文字与图标只是画出来的字形，可访问名称和提示才是“放大 / 缩小”的语义来源。
        let text_button = |id: &'static str, label: SharedString, name: SharedString| {
            Button::new(id)
                .debug_selector(move || id.to_string())
                .ghost()
                .small()
                .compact()
                .rounded(px(999.0))
                .label(label)
                .accessibility_label(name.clone())
                .tooltip(name)
        };
        let toolbar = div()
            .id("mermaid-preview-controls")
            .debug_selector(|| "mermaid-preview-controls".into())
            .h(px(44.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .rounded_full()
            .border_1()
            .border_color(c.dialog_border)
            .bg(c.dialog_surface)
            .shadow_md()
            .text_size(px(12.0))
            .text_color(c.dialog_title)
            // 工具条整体吃掉按下事件：底下的预览要拿同一次按下拖动图像。
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                text_button("mermaid-preview-zoom-out", "−".into(), zoom_out_label.into())
                    .on_click(cx.listener(|editor, _, _, cx| {
                        editor.zoom_mermaid_preview(1.0 / 1.2, cx);
                    })),
            )
            .child(
                div()
                    .w(px(50.0))
                    .flex()
                    .justify_center()
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("{:.0}%", scale * 100.0)),
            )
            .child(
                text_button("mermaid-preview-zoom-in", "+".into(), zoom_in_label.into()).on_click(
                    cx.listener(|editor, _, _, cx| {
                        editor.zoom_mermaid_preview(1.2, cx);
                    }),
                ),
            )
            .child(div().w(px(1.0)).h(px(16.0)).mx(px(6.0)).bg(c.dialog_border))
            .child(
                text_button("mermaid-preview-original", "100%".into(), "100%".into()).on_click(
                    cx.listener(|editor, _, _, cx| {
                        if let Some(preview) = editor.mermaid_preview.as_mut() {
                            preview.scale = Some(1.0);
                            preview.pan = point(px(0.0), px(0.0));
                            preview.clamp_pan();
                            cx.notify();
                        }
                    }),
                ),
            )
            .child(
                text_button(
                    "mermaid-preview-fit",
                    reset_label.clone().into(),
                    reset_label.into(),
                ).on_click(
                    cx.listener(|editor, _, _, cx| {
                        editor.reset_mermaid_preview(cx);
                    }),
                ),
            );
        let close = div()
            .absolute()
            .top(px(20.0))
            .right(px(20.0))
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(c.dialog_border)
            .bg(c.dialog_surface)
            .shadow_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new("mermaid-preview-close")
                    .debug_selector(|| "mermaid-preview-close".into())
                    .ghost()
                    .small()
                    .compact()
                    .rounded(px(999.0))
                    .icon(
                        Icon::empty()
                            .path("icon/workspace/tab-close.svg")
                            .text_color(c.dialog_muted),
                    )
                    .accessibility_label(close_label.clone())
                    .tooltip(close_label)
                    .on_click(cx.listener(|editor, _, window, cx| {
                        editor.close_mermaid_preview(window, cx);
                    })),
            );
        // 深色主题的 SVG 里连线和子图都是为深色背景配的；预览画布必须跟着
        // 图表主题走，白色画布会让浅色连线几乎看不见。
        let canvas_background = if theme.is_dark() {
            c.editor_background
        } else {
            white()
        };
        let canvas = div()
            .id("mermaid-preview-viewport")
            .debug_selector(|| "mermaid-preview-viewport".into())
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .overflow_hidden()
            .bg(canvas_background)
            .cursor(CursorStyle::OpenHand)
            .on_scroll_wheel(cx.listener(|editor, event: &ScrollWheelEvent, _, cx| {
                cx.stop_propagation();
                if let Some(factor) = preview_wheel_zoom_factor(event) {
                    editor.zoom_mermaid_preview(factor, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(preview) = editor.mermaid_preview.as_mut() {
                        preview.drag_start = Some((event.position, preview.pan));
                    }
                }),
            )
            .on_mouse_move(cx.listener(|editor, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                if let Some(preview) = editor.mermaid_preview.as_mut()
                    && let Some((start, pan)) = preview.drag_start
                {
                    preview.pan = pan + (event.position - start);
                    preview.clamp_pan();
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|editor, _, _, _| {
                    if let Some(preview) = editor.mermaid_preview.as_mut() {
                        preview.drag_start = None;
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|editor, _, _, _| {
                    if let Some(preview) = editor.mermaid_preview.as_mut() {
                        preview.drag_start = None;
                    }
                }),
            )
            .child(
                img(path)
                    .debug_selector(|| "mermaid-preview-image".into())
                    .absolute()
                    .left(origin.x)
                    .top(origin.y)
                    .w(width)
                    .h(height),
            );
        Some(
            div()
                .id("mermaid-preview")
                .debug_selector(|| "mermaid-preview".into())
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .p(px(24.0))
                .flex()
                .flex_col()
                .bg(c.dialog_surface)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                // 浮层开着时按键不能落到正文：焦点进来后这些键由这一层接手。
                .on_key_down(cx.listener(|editor, event: &KeyDownEvent, window, cx| {
                    let handled = match event.keystroke.key.as_str() {
                        "escape" => {
                            editor.close_mermaid_preview(window, cx);
                            true
                        }
                        "+" | "=" => {
                            editor.zoom_mermaid_preview(1.2, cx);
                            true
                        }
                        "-" => {
                            editor.zoom_mermaid_preview(1.0 / 1.2, cx);
                            true
                        }
                        "0" => {
                            editor.reset_mermaid_preview(cx);
                            true
                        }
                        _ => false,
                    };
                    if handled {
                        cx.stop_propagation();
                    }
                }))
                .on_prepaint(move |_, window, cx| {
                    if let Err(error) = owner.update(cx, |editor, cx| {
                        let Some(preview) = editor.mermaid_preview.as_mut() else {
                            return;
                        };
                        if preview.restore_focus.is_some() {
                            return;
                        }
                        // 先记住正文里那个焦点，再把焦点交给浮层：Tab / Enter
                        // 才能落在工具条上，Esc 关闭后再还回原处。
                        preview.restore_focus =
                            Some(window.focused(cx).map(|focus| focus.downgrade()));
                        let focus = preview.focus.clone();
                        focus.focus(window, cx);
                    }) {
                        eprintln!("聚焦预览浮层失败：{error}");
                    }
                })
                .focus_trap("mermaid-preview-focus-trap", &overlay_focus)
                .child(canvas)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(24.0))
                        .flex()
                        .justify_center()
                        .child(toolbar),
                )
                .child(close)
                .into_any_element(),
        )
    }

    pub(super) fn load_image_file_preview(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let source = Resource::Path(path.clone().into());
        let load = ImageAssetLoader::load(source, cx);
        let loaded = cx.background_spawn(load);
        let task = cx.spawn({
            let path = path.clone();
            async move |this, cx| {
                let result = loaded
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|image| {
                        if image.frame_count() > 0 {
                            Ok(image)
                        } else {
                            Err("图片不包含可显示的帧".to_string())
                        }
                    });
                if let Err(error) = this.update(cx, |editor, cx| {
                    let Some(preview) = editor
                        .image_preview
                        .as_mut()
                        .filter(|preview| preview.path == path)
                    else {
                        return;
                    };
                    match result {
                        Ok(image) => preview.image = Some(image),
                        Err(detail) => {
                            preview.error = Some(detail.clone());
                            let title = cx
                                .global::<I18nManager>()
                                .strings()
                                .image_load_failed
                                .clone();
                            editor.show_message_modal(
                                title,
                                format!("{}\n{detail}", path.display()),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                }) {
                    eprintln!("图片预览视图已关闭：{error}");
                }
            }
        });
        self.image_preview = Some(ImageFilePreview {
            path,
            image: None,
            error: None,
            zoom: 1.0,
            pan: point(px(0.0), px(0.0)),
            drag_start: None,
            _load_task: task,
        });
        self.active_entity_id = None;
        self.pending_focus = None;
        cx.notify();
    }

    pub(super) fn render_image_file_preview(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(preview) = &self.image_preview else {
            return div().into_any_element();
        };
        let content = if let Some(image) = &preview.image {
            img(image.clone())
                .w(px(image.size(0).width.0 as f32 * preview.zoom))
                .h(px(image.size(0).height.0 as f32 * preview.zoom))
                .id(("image-file-preview-image", image.id.0))
                .debug_selector(|| "image-file-preview-image".into())
                .max_w(relative(preview.zoom))
                .max_h(relative(preview.zoom))
                .flex_shrink_0()
                .object_fit(ObjectFit::Contain)
                .relative()
                .left(preview.pan.x)
                .top(preview.pan.y)
                .into_any_element()
        } else if preview.error.is_some() {
            div()
                .debug_selector(|| "image-file-preview-message".into())
                .child(strings.image_load_failed.clone())
                .into_any_element()
        } else {
            // 冷缓存会跨帧解码，立即显示文字占位会让每张图片首次打开时闪一下。
            div().into_any_element()
        };
        div()
            .id("image-file-preview")
            .debug_selector(|| "image-file-preview".into())
            .w_full()
            .h_full()
            .min_h(px(0.0))
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.0))
            .overflow_hidden()
            .text_color(theme.colors.dialog_muted)
            .on_scroll_wheel(cx.listener(|editor, event: &ScrollWheelEvent, window, cx| {
                cx.stop_propagation();
                let viewport = window.viewport_size();
                if let Some(factor) = preview_wheel_zoom_factor(event)
                    && let Some(preview) = editor.image_preview.as_mut()
                    && preview.image.is_some()
                {
                    preview.zoom = (preview.zoom * factor).clamp(0.1, 8.0);
                    preview.clamp_pan(viewport);
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|editor, event: &MouseDownEvent, _window, cx| {
                    let Some(preview) = editor.image_preview.as_mut() else {
                        return;
                    };
                    if preview.image.is_none() {
                        return;
                    }
                    preview.drag_start = Some((event.position, preview.pan));
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(|editor, event: &MouseMoveEvent, window, cx| {
                if !event.dragging() {
                    return;
                }
                let viewport = window.viewport_size();
                let Some(preview) = editor.image_preview.as_mut() else {
                    return;
                };
                let Some((start_position, start_pan)) = preview.drag_start else {
                    return;
                };
                preview.pan = start_pan + (event.position - start_position);
                preview.clamp_pan(viewport);
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|editor, _event: &MouseUpEvent, _window, _cx| {
                    if let Some(preview) = editor.image_preview.as_mut() {
                        preview.drag_start = None;
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|editor, _event: &MouseUpEvent, _window, _cx| {
                    if let Some(preview) = editor.image_preview.as_mut() {
                        preview.drag_start = None;
                    }
                }),
            )
            .child(content)
            .into_any_element()
    }
}

#[cfg(test)]
mod theme_tests {
    use super::Editor;
    use crate::theme::ThemeManager;
    use gpui::{BorrowAppContext as _, TestAppContext};

    #[gpui::test]
    fn mermaid_preview_theme_change_keeps_its_zoom(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init_with_language_id(cx, "en-US");
            ThemeManager::init_with_theme_id(cx, "velora-dark");
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        editor.update(cx, |editor, cx| {
            editor.open_mermaid_preview("```mermaid\nflowchart LR\n A-->B\n```", cx)
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        let path = editor.update(cx, |editor, cx| {
            editor.zoom_mermaid_preview(2.0, cx);
            editor
                .mermaid_preview
                .as_ref()
                .unwrap()
                .rendered
                .path
                .clone()
        });
        cx.update(|_, cx| {
            cx.update_global::<ThemeManager, _>(|manager, _| {
                assert!(manager.set_theme_by_id("forest"))
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            let preview = editor.mermaid_preview.as_ref().unwrap();
            assert_ne!(
                preview.rendered.path, path,
                "预览已打开时也要按新外观更新图表"
            );
            assert_eq!(preview.scale(), 2.0, "换主题不能清除用户缩放");
            assert_eq!(editor.buffer.text(), "正文");
        });
    }
}
