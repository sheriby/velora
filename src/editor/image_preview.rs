use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;

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
    _load_task: Task<()>,
}

pub(super) struct MermaidPreview {
    rendered: crate::components::MermaidSvgRender,
    scale: Option<f32>,
    viewport: Size<Pixels>,
    pan: Point<Pixels>,
    drag_start: Option<(Point<Pixels>, Point<Pixels>)>,
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
        let result = crate::components::parse_mermaid_fence_source(markdown)
            .ok_or_else(|| anyhow::anyhow!("无法读取图表源码"))
            .and_then(|source| {
                crate::components::render_mermaid_svg_for_display(&source, f32::MAX)
            });
        match result {
            Ok(rendered) => {
                self.mermaid_preview = Some(MermaidPreview {
                    rendered,
                    scale: None,
                    viewport: size(px(1.0), px(1.0)),
                    pan: point(px(0.0), px(0.0)),
                    drag_start: None,
                });
                cx.notify();
            }
            Err(error) => self.show_message_modal("Mermaid", error.to_string(), cx),
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
        cx: &mut Context<Self>,
    ) -> bool {
        if self.mermaid_preview.is_none() {
            return false;
        }
        match keystroke.key.as_str() {
            "escape" => {
                self.mermaid_preview = None;
                cx.notify();
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
        let c = &theme.colors;
        let reset_label = cx
            .global::<I18nManager>()
            .strings()
            .command_zoom_reset
            .clone();
        let button = |id: &'static str, label: SharedString| {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .h(px(32.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(label)
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
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                button("mermaid-preview-zoom-out", "−".into())
                    .text_size(px(18.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|editor, _, _, cx| {
                            cx.stop_propagation();
                            editor.zoom_mermaid_preview(1.0 / 1.2, cx);
                        }),
                    ),
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
                button("mermaid-preview-zoom-in", "+".into())
                    .text_size(px(18.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|editor, _, _, cx| {
                            cx.stop_propagation();
                            editor.zoom_mermaid_preview(1.2, cx);
                        }),
                    ),
            )
            .child(div().w(px(1.0)).h(px(16.0)).mx(px(6.0)).bg(c.dialog_border))
            .child(
                button("mermaid-preview-original", "100%".into()).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|editor, _, _, cx| {
                        cx.stop_propagation();
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
                button("mermaid-preview-fit", reset_label.into()).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|editor, _, _, cx| {
                        cx.stop_propagation();
                        editor.reset_mermaid_preview(cx);
                    }),
                ),
            );
        let close = div()
            .id("mermaid-preview-close")
            .debug_selector(|| "mermaid-preview-close".into())
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
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|editor, _, _, cx| {
                    cx.stop_propagation();
                    editor.mermaid_preview = None;
                    cx.notify();
                }),
            )
            .child(
                svg()
                    .path("icon/workspace/tab-close.svg")
                    .size(px(14.0))
                    .text_color(c.dialog_muted),
            );
        let canvas = div()
            .id("mermaid-preview-viewport")
            .debug_selector(|| "mermaid-preview-viewport".into())
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .overflow_hidden()
            .bg(white())
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
            .on_scroll_wheel(cx.listener(|editor, event: &ScrollWheelEvent, _, cx| {
                cx.stop_propagation();
                if let Some(factor) = preview_wheel_zoom_factor(event)
                    && let Some(preview) = editor.image_preview.as_mut()
                    && preview.image.is_some()
                {
                    preview.zoom = (preview.zoom * factor).clamp(0.1, 8.0);
                    cx.notify();
                }
            }))
            .child(content)
            .into_any_element()
    }
}
