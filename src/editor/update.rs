//! 已发布版本的通知、下载和保存后安装。

use gpui::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use super::{Editor, InfoDialogKind};
use crate::config::EditorSettings;
use crate::config::preferences::update_app_preferences;
use crate::i18n::{I18nManager, I18nStrings};
use crate::net::update::{self as update_check, InstallPlan, UpdateCheckResult, UpdateVersionInfo};
use crate::theme::Theme;

#[derive(Default)]
struct UpdateSession {
    startup_checked: bool,
    download_active: bool,
    download_cancel: Option<Arc<AtomicBool>>,
    download_window: Option<WindowId>,
    pending_install: Option<InstallPlan>,
    fallback_notification: Option<UpdateVersionInfo>,
    closed_subscription: Option<Subscription>,
}
impl Global for UpdateSession {}

fn init_update_session(cx: &mut App) {
    if cx.try_global::<UpdateSession>().is_some() {
        return;
    }
    cx.set_global(UpdateSession::default());
    let subscription = cx.on_window_closed(|cx| {
        let closed_download = cx
            .global::<UpdateSession>()
            .download_window
            .is_some_and(|owner| {
                cx.windows()
                    .iter()
                    .all(|window| window.window_id() != owner)
            });
        if closed_download {
            cx.update_global::<UpdateSession, _>(|session, _| {
                if let Some(cancelled) = session.download_cancel.take() {
                    cancelled.store(true, Ordering::Relaxed);
                }
                session.download_active = false;
                session.download_window = None;
            });
        }
        if cx
            .try_global::<UpdateSession>()
            .is_some_and(|session| session.pending_install.is_some())
        {
            crate::app_menu::request_quit_application(cx);
        }
    });
    cx.update_global::<UpdateSession, _>(|session, _| {
        session.closed_subscription = Some(subscription)
    });
}

pub(crate) fn cancel_pending_install(cx: &mut App) {
    if cx
        .try_global::<UpdateSession>()
        .is_some_and(|session| session.pending_install.is_some())
    {
        cx.update_global::<UpdateSession, _>(|session, _| {
            session.pending_install = None;
            session.download_active = false;
            session.download_cancel = None;
            session.download_window = None;
        });
        cx.defer(|cx| {
            for handle in cx.windows() {
                let Some(handle) = handle.downcast::<Editor>() else {
                    continue;
                };
                if let Err(error) = handle.update(cx, |editor, _, cx| {
                    if let Some(cancelled) = editor.update_download_cancel.take() {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                    editor.update_download_progress = None;
                    cx.notify();
                }) {
                    eprintln!("取消更新状态失败：{error}");
                }
            }
        });
    }
}

pub(crate) fn install_pending_update(cx: &mut App) -> bool {
    let plan = if cx.try_global::<UpdateSession>().is_some() {
        cx.update_global::<UpdateSession, _>(|session, _| session.pending_install.take())
    } else {
        None
    };
    let Some(plan) = plan else {
        return true;
    };
    match update_check::launch_install_helper(&plan) {
        Ok(()) => true,
        Err(error) => {
            show_failure_on_app(cx, &error.to_string());
            false
        }
    }
}

fn show_failure_on_app(cx: &mut App, detail: &str) {
    for handle in cx.windows() {
        let Some(handle) = handle.downcast::<Editor>() else {
            continue;
        };
        match handle.update(cx, |editor, _, cx| {
            editor.update_download_progress = None;
            editor.update_download_cancel = None;
            editor.show_update_failure(detail, cx);
        }) {
            Ok(()) => return,
            Err(error) => eprintln!("显示更新错误失败：{error}"),
        }
    }
    eprintln!("更新失败：{detail}");
}

impl Editor {
    pub(crate) fn maybe_check_updates_on_startup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        init_update_session(cx);
        if let Some(info) =
            cx.update_global::<UpdateSession, _>(|session, _| session.fallback_notification.take())
        {
            if EditorSettings::updates(cx).ignored_version != info.latest_version {
                self.update_notification = Some(info);
            }
        }
        if cx.global::<UpdateSession>().startup_checked {
            return;
        }
        cx.update_global::<UpdateSession, _>(|session, _| session.startup_checked = true);
        // 开机路径一律不打扰用户：上次安装留下的失败原因只进日志（用户报修：连不上
        // GitHub 的机器每次启动都被「更新失败」拦一下）。用户自己点的路径另说。
        match update_check::take_install_error() {
            Ok(Some(detail)) => {
                eprintln!("上次更新安装失败：{detail}");
                return;
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("读取更新安装结果失败：{error}");
                return;
            }
        }
        if EditorSettings::updates(cx).check_on_startup {
            self.start_update_check(false, window, cx);
        }
    }

    pub(crate) fn request_check_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.show_unsaved_changes_dialog {
            return;
        }
        init_update_session(cx);
        if self.update_download_progress.is_some()
            || (cx.global::<UpdateSession>().download_active
                || cx.global::<UpdateSession>().pending_install.is_some())
        {
            let message = cx
                .global::<I18nManager>()
                .strings()
                .update_preparing
                .clone();
            self.show_message_modal(message.clone(), message, cx);
            return;
        }
        if self.update_check_in_progress {
            self.show_info_dialog(InfoDialogKind::CheckForUpdates, cx);
            return;
        }
        self.start_update_check(true, window, cx);
    }

    fn start_update_check(&mut self, manual: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.update_check_in_progress = true;
        if manual {
            self.show_info_dialog(InfoDialogKind::CheckForUpdates, cx);
        }
        let preferences = EditorSettings::updates(cx);
        let weak_editor = cx.entity().downgrade();
        let worker = cx.background_executor().spawn(async move {
            if preferences.include_prereleases {
                update_check::check_latest_version_with_options(env!("CARGO_PKG_VERSION"), true)
            } else {
                update_check::check_latest_version(env!("CARGO_PKG_VERSION"))
            }
        });
        self.update_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let result = worker.await;
                let available = match &result {
                    Ok(UpdateCheckResult::UpdateAvailable(info)) => Some(info.clone()),
                    _ => None,
                };
                if let Err(error) = weak_editor.update(cx, |editor, cx| {
                    editor.update_check_in_progress = false;
                    if manual {
                        editor.hide_info_dialog(cx);
                    }
                    editor.apply_update_result(result, manual, cx);
                }) {
                    eprintln!("更新检查窗口已关闭：{error}");
                    if let Some(info) = available {
                        if let Err(error) = cx.update_global::<UpdateSession, _>(|session, _| {
                            session.fallback_notification = Some(info)
                        }) {
                            eprintln!("保留更新通知失败：{error}");
                        }
                    }
                }
            },
        ));
    }

    fn apply_update_result(
        &mut self,
        result: Result<UpdateCheckResult, update_check::UpdateCheckError>,
        manual: bool,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(UpdateCheckResult::UpdateAvailable(info)) => {
                if manual || EditorSettings::updates(cx).ignored_version != info.latest_version {
                    self.update_notification = Some(info);
                }
            }
            Ok(UpdateCheckResult::UpToDate(info)) if manual => {
                let strings = cx.global::<I18nManager>().strings().clone();
                let detail = format_update_message(
                    &strings.update_up_to_date_message_template,
                    &info.current_version,
                    &info.latest_version,
                );
                self.show_message_modal(strings.update_up_to_date_title, detail, cx);
            }
            Ok(UpdateCheckResult::UpToDate(_)) => {}
            // 自动检查失败静默，只记日志（用户报修：连不上 GitHub 的机器每次启动都弹
            // 一次「更新失败」）；手动检查是用户自己问的，要回话。
            Err(error) if manual => self.show_update_failure(&error.to_string(), cx),
            Err(error) => eprintln!("检查更新失败（自动检查，不打扰用户）：{error}"),
        }
        cx.notify();
    }

    fn show_update_failure(&mut self, detail: &str, cx: &mut Context<Self>) {
        let strings = cx.global::<I18nManager>().strings().clone();
        self.show_message_modal(
            strings.update_failed_title,
            strings
                .update_failed_message_template
                .replace("{error}", detail),
            cx,
        );
    }

    fn dismiss_update_notice(&mut self, cx: &mut Context<Self>) {
        let cancelled = self.update_download_cancel.take();
        if let Some(cancelled) = cancelled.as_ref() {
            cancelled.store(true, Ordering::Relaxed);
            cancel_pending_install(cx);
        }
        self.update_task = None;
        self.update_download_progress = None;
        self.update_notification = None;
        // 其它窗口也能检查更新并关闭通知，只有下载所属窗口能解除全局互斥。
        if cancelled.is_some() && cx.try_global::<UpdateSession>().is_some() {
            cx.update_global::<UpdateSession, _>(|session, _| {
                session.download_active = false;
                session.download_cancel = None;
                session.download_window = None;
            });
        }
        cx.notify();
    }

    fn skip_update_version(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.update_notification.as_ref() else {
            return;
        };
        let version = info.latest_version.clone();
        match update_app_preferences(|preferences| preferences.updates.ignored_version = version) {
            Ok(preferences) => {
                EditorSettings::set_updates_in_memory(cx, preferences.updates);
                self.dismiss_update_notice(cx);
            }
            Err(error) => self.show_update_failure(&error.to_string(), cx),
        }
    }

    fn download_update(&mut self, cx: &mut Context<Self>) {
        init_update_session(cx);
        if cx.global::<UpdateSession>().download_active
            || cx.global::<UpdateSession>().pending_install.is_some()
        {
            let message = cx
                .global::<I18nManager>()
                .strings()
                .update_preparing
                .clone();
            self.show_message_modal(message.clone(), message, cx);
            return;
        }
        let Some(info) = self.update_notification.clone() else {
            return;
        };
        let Some(package) = info.package.clone() else {
            self.show_update_failure("缺少当前系统的安装包", cx);
            return;
        };
        cx.update_global::<UpdateSession, _>(|session, _| session.download_active = true);
        let progress = Arc::new(AtomicU64::new(0));
        let cancelled = Arc::new(AtomicBool::new(false));
        self.update_download_progress = Some(progress.clone());
        self.update_download_cancel = Some(cancelled.clone());
        cx.update_global::<UpdateSession, _>(|session, _| {
            session.download_cancel = Some(cancelled.clone());
            session.download_window = self.window_handle.map(|window| window.window_id());
        });
        let worker = cx.background_executor().spawn({
            let progress = progress.clone();
            let cancelled = cancelled.clone();
            let package = package.clone();
            async move {
                update_check::download_package(&package, &cancelled, |bytes| {
                    progress.store(bytes, Ordering::Relaxed)
                })
            }
        });
        let weak_editor = cx.entity().downgrade();
        let timer = cx.background_executor().clone();
        self.update_task = Some(cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            use futures::FutureExt;
            let worker = worker.fuse();
            futures::pin_mut!(worker);
            let result = loop {
                let tick = timer.timer(std::time::Duration::from_millis(100)).fuse();
                futures::pin_mut!(tick);
                futures::select! {
                    result = worker => break result,
                    _ = tick => {
                        if let Err(error) = weak_editor.update(cx, |_, cx| cx.notify()) { eprintln!("更新下载窗口已关闭：{error}"); }
                    }
                }
            };
            if cancelled.load(Ordering::Relaxed) { return; }
            if let Err(error) = weak_editor.update(cx, |editor, cx| {
                cx.update_global::<UpdateSession,_>(|session,_| { session.download_active = false; session.download_cancel = None;
            session.download_window = None; });
                match result {
                    Ok(path) => {
                        cx.defer(move |cx| {
                            let mut arguments = Vec::new();
                            for window in cx.windows() {
                                let Some(window) = window.downcast::<Editor>() else { continue; };
                                match window.update(cx, |editor, _, cx| {
                                    editor.persist_session(cx);
                                    editor.workspace.root.clone().or_else(|| editor.file_path.clone())
                                }) {
                                    Ok(Some(path)) => { let argument = path.to_string_lossy().into_owned(); if !arguments.contains(&argument) { arguments.push(argument); } }
                                    Ok(None) => {}
                                    Err(error) => eprintln!("记录更新前的文档失败：{error}"),
                                }
                            }
                            match update_check::prepare_install(&package,&path,arguments) {
                                Ok(plan) => {
                                    cx.update_global::<UpdateSession,_>(|session,_| session.pending_install = Some(plan));
                                    crate::app_menu::request_quit_application(cx);
                                }
                                Err(error) => show_failure_on_app(cx,&error.to_string()),
                            }
                        });
                    }
                    Err(error) => {
                        editor.update_download_progress = None;
                        editor.update_download_cancel = None;
                        editor.show_update_failure(&error.to_string(),cx);
                    }
                }
                cx.notify();
            }) { eprintln!("更新下载结束的窗口已关闭：{error}"); }
        }));
        cx.notify();
    }

    pub(crate) fn render_update_notification(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let info = self.update_notification.as_ref()?;
        let busy = self.update_download_progress.is_some();
        let description = if let Some(progress) = self.update_download_progress.as_ref() {
            let total = info
                .package
                .as_ref()
                .map_or(1, |package| package.size)
                .max(1);
            let percent = (progress.load(Ordering::Relaxed).min(total) * 100 / total).to_string();
            if percent == "100" {
                strings.update_preparing.clone()
            } else {
                strings.update_downloading.replace("{percent}", &percent)
            }
        } else {
            format_update_message(
                &strings.update_available_message_template,
                &info.current_version,
                &info.latest_version,
            )
        };
        let c = &theme.colors;
        let mut card = div()
            .id("update-notification")
            .debug_selector(|| "update-notification".into())
            .absolute()
            .right(px(16.0))
            .bottom(px(theme.dimensions.status_bar_height + 12.0))
            .w(px(350.0))
            .max_w(relative(0.95))
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .rounded(px(10.0))
            .bg(c.dialog_surface)
            .border_1()
            .border_color(c.dialog_border)
            .shadow_lg()
            .occlude()
            .text_size(px(13.0))
            .text_color(c.dialog_body)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().font_weight(FontWeight::BOLD).child(format!(
                "{} {}",
                strings.update_available_title, info.latest_version
            )))
            .child(description)
            .child(strings.update_restart_notice.clone());
        let mut actions = div().flex().gap(px(8.0)).justify_end();
        if !busy {
            actions = actions.child(
                div()
                    .id("update-confirm")
                    .debug_selector(|| "update-confirm".into())
                    .px(px(12.0))
                    .py(px(5.0))
                    .rounded(px(5.0))
                    .bg(c.dialog_primary_button_bg)
                    .text_color(c.dialog_primary_button_text)
                    .cursor_pointer()
                    .child(strings.update_install.clone())
                    .on_click(cx.listener(|editor, _, _, cx| editor.download_update(cx))),
            );
        }
        actions = actions.child(
            div()
                .id("update-cancel")
                .debug_selector(|| "update-cancel".into())
                .px(px(12.0))
                .py(px(5.0))
                .rounded(px(5.0))
                .border_1()
                .border_color(c.dialog_border)
                .cursor_pointer()
                .child(strings.preferences_cancel.clone())
                .on_click(cx.listener(|editor, _, _, cx| editor.dismiss_update_notice(cx))),
        );
        card = card.child(actions);
        if !busy {
            card = card.child(
                div()
                    .id("update-skip")
                    .debug_selector(|| "update-skip".into())
                    .text_size(px(11.0))
                    .text_color(c.dialog_muted)
                    .cursor_pointer()
                    .child(strings.update_skip_version.clone())
                    .on_click(cx.listener(|editor, _, _, cx| editor.skip_update_version(cx))),
            );
        }
        Some(card.into_any_element())
    }
}

fn format_update_message(template: &str, current: &str, latest: &str) -> String {
    template
        .replace("{current}", current)
        .replace("{latest}", latest)
}

#[cfg(test)]
mod tests {
    use super::{
        Editor, UpdateCheckResult, UpdateSession, UpdateVersionInfo, format_update_message,
    };
    use crate::net::update::UpdateSource;
    use gpui::{BorrowAppContext, TestAppContext};
    #[gpui::test]
    async fn dismissing_another_windows_notice_keeps_the_download_active(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
        });
        let (owner, _) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        let (other, visual) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "另一个窗口".into(), None));
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        owner.update(visual, |editor, cx| {
            editor.update_download_cancel = Some(cancelled.clone());
            cx.update_global::<UpdateSession, _>(|session, _| {
                session.download_active = true;
                session.download_cancel = Some(cancelled.clone());
            });
        });
        other.update(visual, |editor, cx| editor.dismiss_update_notice(cx));
        visual.update(|_, cx| {
            assert!(cx.global::<UpdateSession>().download_active,
                "另一窗口取消通知不能解除进行中的下载保护");
        });
        assert!(!cancelled.load(std::sync::atomic::Ordering::Relaxed));
        owner.update(visual, |editor, cx| editor.dismiss_update_notice(cx));
        assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
        visual.update(|_, cx| assert!(!cx.global::<UpdateSession>().download_active));
    }
    #[test]
    fn update_message_templates_replace_versions() {
        assert_eq!(
            format_update_message("{current} → {latest}", "0.2.4", "0.2.5"),
            "0.2.4 → 0.2.5"
        );
    }
    #[gpui::test]
    async fn startup_toggle_and_ignored_versions_do_not_block_manual_checks(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            let mut settings = crate::config::preferences::UpdatePreferences::default();
            settings.check_on_startup = false;
            settings.ignored_version = "0.2.5".into();
            crate::config::EditorSettings::set_updates_in_memory(cx, settings);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        editor.update_in(cx, |editor, window, cx| {
            editor.maybe_check_updates_on_startup(window, cx);
            assert!(!editor.update_check_in_progress);
            assert!(cx.global::<UpdateSession>().startup_checked);
            let info = UpdateVersionInfo {
                current_version: "0.2.4".into(),
                latest_version: "0.2.5".into(),
                source: UpdateSource::GitHub,
                release_url: crate::net::update::RELEASES_URL.into(),
                package: None,
            };
            editor.apply_update_result(
                Ok(UpdateCheckResult::UpdateAvailable(info.clone())),
                false,
                cx,
            );
            assert!(editor.update_notification.is_none());
            editor.apply_update_result(Ok(UpdateCheckResult::UpdateAvailable(info)), true, cx);
            assert!(editor.update_notification.is_some());
        });
        cx.update(|window, cx| window.draw(cx).clear());
        assert!(cx.debug_bounds("update-notification").is_some());
        assert!(cx.debug_bounds("update-confirm").is_some());
        assert!(cx.debug_bounds("update-cancel").is_some());
        assert!(cx.debug_bounds("update-skip").is_some());
    }

    #[gpui::test]
    async fn installation_waits_for_unsaved_documents_and_cancel_aborts_it(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "未保存正文".into(), None));
        editor.update(cx, |editor, _| editor.document_dirty = true);
        cx.update(|_, cx| {
            let path =
                std::env::temp_dir().join(format!("missing-update-{}", uuid::Uuid::new_v4()));
            cx.update_global::<UpdateSession, _>(|session, _| {
                session.pending_install = Some(crate::net::update::InstallPlan {
                    package_path: path.clone(),
                    platform: crate::net::update::UpdatePlatform::MacOsArm64,
                    process_id: std::process::id(),
                    install_dir: path.clone(),
                    launch_path: path.clone(),
                    previous_executable: path.clone(),
                    launch_arguments: String::new(),
                    launch_args: Vec::new(),
                    error_path: path,
                })
            });
            crate::app_menu::request_quit_application(cx);
        });
        cx.run_until_parked();
        editor.read_with(cx, |editor, cx| {
            assert!(editor.show_unsaved_changes_dialog, "必须先处理未保存文档");
            assert!(
                cx.global::<UpdateSession>().pending_install.is_some(),
                "保存守卫期间不能开始安装"
            );
        });
        cx.update(|_, cx| super::cancel_pending_install(cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(cx.global::<UpdateSession>().pending_install.is_none()));
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "未保存正文"
        );
    }

    #[gpui::test]
    async fn closing_the_download_window_cancels_the_worker_and_releases_the_session(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
        });
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (editor, visual) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        visual.update(|window, cx| {
            let id = window.window_handle().window_id();
            editor.update(cx, |editor, cx| {
                editor.update_download_cancel = Some(cancelled.clone());
                cx.update_global::<UpdateSession, _>(|session, _| {
                    session.download_active = true;
                    session.download_cancel = Some(cancelled.clone());
                    session.download_window = Some(id);
                });
            });
            window.remove_window();
        });
        cx.run_until_parked();
        assert!(
            cancelled.load(std::sync::atomic::Ordering::Relaxed),
            "关闭窗口时应立即取消下载，不依赖实体释放时机"
        );
        cx.update(|cx| assert!(!cx.global::<UpdateSession>().download_active));
        drop(editor);
    }

    /// 自动检查更新失败必须静默：用户报修——连不上 GitHub 的机器每次启动都弹一次
    /// 「更新失败」，关掉下次启动还来，很打扰人。手动点「检查更新」失败仍然要说一声：
    /// 那是用户自己问的，静默等于点了没反应。
    #[gpui::test]
    async fn automatic_update_check_failures_are_silent_while_manual_ones_speak(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
            let mut settings = crate::config::preferences::UpdatePreferences::default();
            settings.check_on_startup = false;
            crate::config::EditorSettings::set_updates_in_memory(cx, settings);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        let failure = || crate::net::update::UpdateCheckError::Fetch("连不上 GitHub".into());
        editor.update(cx, |editor, cx| {
            editor.apply_update_result(Err(failure()), false, cx);
            assert!(
                !editor.modal_is_open(),
                "开机自动检查失败不该弹窗：连不上 GitHub 的机器每次启动都会被拦一下"
            );
            editor.apply_update_result(Err(failure()), true, cx);
            assert!(
                editor.modal_is_open(),
                "手动点「检查更新」失败要告诉用户，静默等于点了没反应"
            );
        });
    }

    /// 上次安装/自检留在磁盘上的失败原因，也不能在开机路径上变成弹窗。
    #[gpui::test]
    async fn a_leftover_install_failure_does_not_greet_the_user_on_startup(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!("velora-update-silent-{}", uuid::Uuid::new_v4()));
        let _config_root = crate::config::override_test_config_root(root.clone());
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
            let mut settings = crate::config::preferences::UpdatePreferences::default();
            settings.check_on_startup = false;
            crate::config::EditorSettings::set_updates_in_memory(cx, settings);
        });
        let updates = crate::net::update::updates_dir().expect("更新目录");
        std::fs::create_dir_all(&updates).expect("建更新目录");
        std::fs::write(updates.join("install-error.txt"), "安装包校验失败").expect("写安装失败记录");

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        editor.update_in(cx, |editor, window, cx| {
            editor.maybe_check_updates_on_startup(window, cx);
            assert!(
                !editor.modal_is_open(),
                "上次安装失败的原因只在日志里说，不该在开机路径上弹窗"
            );
            assert!(
                !editor.update_check_in_progress,
                "前置：有安装失败记录时开机路径不该同时去发网络请求"
            );
        });
        let _ = std::fs::remove_dir_all(root);
    }
}
