use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;

use super::Editor;
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::Theme;

pub(super) struct ImageFilePreview {
    pub(super) path: PathBuf,
    pub(super) image: Option<Arc<RenderImage>>,
    error: Option<String>,
    _load_task: Task<()>,
}

impl Editor {
    pub(super) fn load_image_file_preview(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let source = Resource::Path(path.clone().into());
        let (loaded, _) = cx.fetch_asset::<ImageAssetLoader>(&source);
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
    ) -> AnyElement {
        let Some(preview) = &self.image_preview else {
            return div().into_any_element();
        };
        let content = if let Some(image) = &preview.image {
            img(image.clone())
                .id(("image-file-preview-image", image.id.0))
                .debug_selector(|| "image-file-preview-image".into())
                .max_w(relative(1.0))
                .max_h(relative(1.0))
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
            .child(content)
            .into_any_element()
    }
}
