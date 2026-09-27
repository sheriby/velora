//! Editor-facing update-check flow.

use futures::FutureExt;
use futures::channel::oneshot;
use gpui::*;

use super::{Editor, InfoDialogKind, modal::ModalSpec};
use crate::i18n::I18nManager;
use crate::net::update::{self as update_check, UpdateCheckResult, UpdateVersionInfo};

impl Editor {
    pub(crate) fn request_check_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.show_unsaved_changes_dialog {
            return;
        }
        if self.update_check_in_progress {
            self.show_info_dialog(InfoDialogKind::CheckForUpdates, cx);
            return;
        }

        self.update_check_in_progress = true;
        self.show_info_dialog(InfoDialogKind::CheckForUpdates, cx);

        let weak_editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        let (tx, rx) = oneshot::channel();
        std::thread::spawn(move || {
            let result = update_check::check_latest_version(env!("CARGO_PKG_VERSION"));
            let _ = tx.send(result);
        });

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = rx
                .map(|result| {
                    result.unwrap_or_else(|_| {
                        Err(update_check::UpdateCheckError::ParseVersion(
                            "update check worker ended before returning a result".to_string(),
                        ))
                    })
                })
                .await;

            let _ = weak_editor.update(cx, |editor, cx| {
                editor.update_check_in_progress = false;
                editor.hide_info_dialog(cx);
            });

            let _ = window_handle;
            // 更新检查结果一律用应用内模态呈现（用户要求：不用系统原生弹窗）。
            let _ = weak_editor.update(cx, |editor, cx| match result {
                Ok(UpdateCheckResult::UpdateAvailable(info)) => {
                    show_update_available_prompt(editor, cx, &info);
                }
                Ok(UpdateCheckResult::UpToDate(info)) => {
                    show_up_to_date_prompt(editor, cx, &info);
                }
                Err(error) => {
                    show_update_failed_prompt(editor, cx, &error.to_string());
                }
            });
        })
        .detach();
    }
}

fn show_update_available_prompt(
    editor: &mut Editor,
    cx: &mut Context<Editor>,
    info: &UpdateVersionInfo,
) {
    let strings = cx.global::<I18nManager>().strings().clone();
    let detail = format_update_message(
        &strings.update_available_message_template,
        &info.current_version,
        &info.latest_version,
    );
    editor.show_modal(
        ModalSpec {
            title: strings.update_available_title.clone().into(),
            detail: Some(detail.into()),
            buttons: vec![
                strings.update_open_release.clone().into(),
                strings.update_later.clone().into(),
            ],
            default_index: 0,
            cancel_index: 1,
        },
        move |choice, _editor, _window, cx| {
            if choice == 0 {
                cx.open_url(update_check::RELEASES_URL);
            }
        },
        cx,
    );
}

fn show_up_to_date_prompt(editor: &mut Editor, cx: &mut Context<Editor>, info: &UpdateVersionInfo) {
    let strings = cx.global::<I18nManager>().strings().clone();
    let detail = format_update_message(
        &strings.update_up_to_date_message_template,
        &info.current_version,
        &info.latest_version,
    );
    editor.show_message_modal(strings.update_up_to_date_title.clone(), detail, cx);
}

fn show_update_failed_prompt(editor: &mut Editor, cx: &mut Context<Editor>, detail: &str) {
    let strings = cx.global::<I18nManager>().strings().clone();
    let message = strings
        .update_failed_message_template
        .replace("{error}", detail);
    editor.show_message_modal(strings.update_failed_title.clone(), message, cx);
}

fn format_update_message(template: &str, current_version: &str, latest_version: &str) -> String {
    template
        .replace("{current}", current_version)
        .replace("{latest}", latest_version)
}

#[cfg(test)]
mod tests {
    use super::format_update_message;

    #[test]
    fn update_message_templates_replace_versions() {
        assert_eq!(
            format_update_message("Current {current}, latest {latest}.", "0.2.1", "0.2.2"),
            "Current 0.2.1, latest 0.2.2."
        );
    }
}
